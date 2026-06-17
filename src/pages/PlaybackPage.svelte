<script>
  import { invoke } from "@tauri-apps/api/tauri";
  import { save } from "@tauri-apps/api/dialog";
  import { onMount } from "svelte";
  import { params, navigate } from "svelte-router-spa";
  import Timeline from "../components/Timeline.svelte";
  import AnnotationToolbar from "../components/AnnotationToolbar.svelte";

  export let recording = null;
  export let frames = [];
  export let annotations = [];

  let recordingId = $params.id;
  let canvas = null;
  let annCanvas = null;
  let cursorCanvas = null;
  let ctx = null;
  let annCtx = null;
  let cursorCtx = null;

  let loading = true;
  let frameBuffers = [];
  let frameRects = [];
  let currentFrameIdx = 0;
  let isPlaying = false;
  let playTimer = null;
  let playbackSpeed = 1;

  let tool = null;
  let toolColor = "#e94560";
  let strokeWidth = 3;
  let isDrawing = false;
  let drawStart = null;
  let drawCurrent = null;
  let pendingText = "";
  let showTextInput = false;

  let canvasScale = 1;
  let scaleFactor = 1.0;

  let collabPanelOpen = false;
  let clipPanelOpen = false;
  let exportPanelOpen = false;
  let userName = "用户" + Math.floor(Math.random() * 1000);
  let roomCode = "";
  let joinRoomCode = "";
  let peers = [];
  let remoteCursors = {};
  let collabPollTimer = null;

  let clipQuery = "";
  let clipSearching = false;
  let clipResults = [];
  let clipIndexed = false;
  let clipIndexing = false;

  let exporting = false;
  let exportProgress = null;

  function logicalToPhysical(lx, ly) {
    return { x: lx * scaleFactor, y: ly * scaleFactor };
  }

  function physicalToLogical(px, py) {
    return { x: px / scaleFactor, y: py / scaleFactor };
  }

  onMount(async () => {
    await loadRecording();
  });

  async function loadRecording() {
    loading = true;
    try {
      const recResult = await invoke("get_recording", { recordingId });
      if (!recResult || !recResult.success) throw new Error(recResult?.error || "获取录制失败");
      recording = recResult.data;
      scaleFactor = recording.scale_factor || 1.0;

      const framesResult = await invoke("get_frames_range", {
        recordingId,
        startMs: 0,
        endMs: recording.duration_ms + 1,
      });
      if (framesResult && framesResult.success) {
        frames = framesResult.data || [];
        frameRects = frames.map((f) => f.rects || []);
      }

      const annResult = await invoke("get_annotations", { recordingId });
      if (annResult && annResult.success) {
        annotations = annResult.data || [];
      }

      const idxResult = await invoke("clip_is_indexed", { recordingId });
      if (idxResult && idxResult.success) {
        clipIndexed = !!idxResult.data;
      }

      initCanvases();
      if (frames.length > 0) {
        await renderFrame(0);
      }
    } catch (e) {
      console.error(e);
      alert("加载录制失败: " + e);
    } finally {
      loading = false;
    }
  }

  function initCanvases() {
    if (!canvas || !annCanvas || !cursorCanvas) return;
    const w = recording?.width || 800;
    const h = recording?.height || 500;
    canvas.width = w;
    canvas.height = h;
    annCanvas.width = w;
    annCanvas.height = h;
    cursorCanvas.width = w;
    cursorCanvas.height = h;
    ctx = canvas.getContext("2d");
    annCtx = annCanvas.getContext("2d");
    cursorCtx = cursorCanvas.getContext("2d");
    ctx.imageSmoothingEnabled = false;
    annCtx.imageSmoothingEnabled = false;
    cursorCtx.imageSmoothingEnabled = false;

    const parent = canvas.parentElement;
    const pw = parent.clientWidth;
    const ph = parent.clientHeight;
    const scaleX = (pw - 40) / w;
    const scaleY = (ph - 40) / h;
    canvasScale = Math.min(scaleX, scaleY, 1);
  }

  async function renderFrame(idx) {
    if (!ctx || idx < 0 || idx >= frames.length) return;
    currentFrameIdx = idx;

    ctx.fillStyle = "#000000";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    for (let i = 0; i <= idx; i++) {
      const frame = frames[i];
      if (!frame || !frame.rects || frame.rects.length === 0) continue;

      if (!frameBuffers[i]) {
        try {
          frameBuffers[i] = await loadFrameImages(frame);
        } catch (e) {
          console.error("加载帧失败", i, e);
          continue;
        }
      }

      const imgs = frameBuffers[i];
      for (let j = 0; j < frame.rects.length && j < imgs.length; j++) {
        const rect = frame.rects[j];
        const img = imgs[j];
        if (img) {
          ctx.putImageData(img, rect.x, rect.y);
        }
      }
    }

    renderAnnotations();
    renderRemoteCursors();
  }

  async function loadFrameImages(frame) {
    const result = [];
    if (!frame.block_images) return result;
    for (const b64 of frame.block_images) {
      try {
        const img = await base64ToImageData(b64);
        result.push(img);
      } catch (e) {
        result.push(null);
      }
    }
    return result;
  }

  function base64ToImageData(b64) {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        const c = document.createElement("canvas");
        c.width = img.width;
        c.height = img.height;
        const cx = c.getContext("2d");
        cx.drawImage(img, 0, 0);
        resolve(cx.getImageData(0, 0, img.width, img.height));
      };
      img.onerror = reject;
      img.src = "data:image/png;base64," + b64;
    });
  }

  function renderAnnotations() {
    if (!annCtx) return;
    annCtx.clearRect(0, 0, annCanvas.width, annCanvas.height);

    const ts = frames[currentFrameIdx]?.timestamp_ms || 0;
    const activeLayers = annotations.filter((a) => a.timestamp_ms <= ts);

    for (const layer of activeLayers) {
      if (!layer.items) continue;
      for (const item of layer.items) {
        drawAnnotation(item);
      }
    }

    if (isDrawing && drawStart && drawCurrent) {
      drawPreviewShape();
    }
  }

  function drawAnnotation(item) {
    annCtx.strokeStyle = item.color || toolColor;
    annCtx.fillStyle = item.color || toolColor;
    annCtx.lineWidth = (item.stroke_width || strokeWidth) * scaleFactor;
    annCtx.lineCap = "round";
    annCtx.lineJoin = "round";

    if (item.type === "rect") {
      const px = item.x * scaleFactor;
      const py = item.y * scaleFactor;
      const pw = (item.width || 0) * scaleFactor;
      const ph = (item.height || 0) * scaleFactor;
      annCtx.strokeRect(px, py, pw, ph);
    } else if (item.type === "arrow") {
      const x = item.x * scaleFactor;
      const y = item.y * scaleFactor;
      const end_x = (item.end_x || 0) * scaleFactor;
      const end_y = (item.end_y || 0) * scaleFactor;
      const headLen = 12 + (item.stroke_width || strokeWidth) * 2 * scaleFactor;
      const angle = Math.atan2(end_y - y, end_x - x);
      annCtx.beginPath();
      annCtx.moveTo(x, y);
      annCtx.lineTo(end_x, end_y);
      annCtx.stroke();
      annCtx.beginPath();
      annCtx.moveTo(end_x, end_y);
      annCtx.lineTo(
        end_x - headLen * Math.cos(angle - Math.PI / 6),
        end_y - headLen * Math.sin(angle - Math.PI / 6)
      );
      annCtx.lineTo(
        end_x - headLen * Math.cos(angle + Math.PI / 6),
        end_y - headLen * Math.sin(angle + Math.PI / 6)
      );
      annCtx.closePath();
      annCtx.fill();
    } else if (item.type === "text") {
      const fs = (14 + (item.stroke_width || strokeWidth) * 2) * scaleFactor;
      const px = item.x * scaleFactor;
      const py = item.y * scaleFactor;
      annCtx.font = `bold ${fs}px sans-serif`;
      const padding = 4 * scaleFactor;
      const tw = annCtx.measureText(item.text || "").width;
      annCtx.fillStyle = "rgba(0,0,0,0.7)";
      annCtx.fillRect(px - padding, py - fs, tw + padding * 2, fs + padding * 2);
      annCtx.fillStyle = item.color || toolColor;
      annCtx.fillText(item.text || "", px, py);
    }
  }

  function drawPreviewShape() {
    annCtx.strokeStyle = toolColor;
    annCtx.fillStyle = toolColor;
    annCtx.lineWidth = strokeWidth * scaleFactor;
    annCtx.setLineDash([6, 4]);
    annCtx.lineCap = "round";

    const x1 = Math.min(drawStart.x, drawCurrent.x) * scaleFactor;
    const y1 = Math.min(drawStart.y, drawCurrent.y) * scaleFactor;
    const w = Math.abs(drawCurrent.x - drawStart.x) * scaleFactor;
    const h = Math.abs(drawCurrent.y - drawStart.y) * scaleFactor;

    if (tool === "rect") {
      annCtx.strokeRect(x1, y1, w, h);
    } else if (tool === "arrow") {
      const sx = drawStart.x * scaleFactor;
      const sy = drawStart.y * scaleFactor;
      const ex = drawCurrent.x * scaleFactor;
      const ey = drawCurrent.y * scaleFactor;
      const headLen = 12 + strokeWidth * 2 * scaleFactor;
      const angle = Math.atan2(ey - sy, ex - sx);
      annCtx.beginPath();
      annCtx.moveTo(sx, sy);
      annCtx.lineTo(ex, ey);
      annCtx.stroke();
      annCtx.setLineDash([]);
      annCtx.beginPath();
      annCtx.moveTo(ex, ey);
      annCtx.lineTo(
        ex - headLen * Math.cos(angle - Math.PI / 6),
        ey - headLen * Math.sin(angle - Math.PI / 6)
      );
      annCtx.lineTo(
        ex - headLen * Math.cos(angle + Math.PI / 6),
        ey - headLen * Math.sin(angle + Math.PI / 6)
      );
      annCtx.closePath();
      annCtx.fill();
    }
    annCtx.setLineDash([]);
  }

  function canvasCoords(e) {
    const rect = annCanvas.getBoundingClientRect();
    const physX = (e.clientX - rect.left) / canvasScale;
    const physY = (e.clientY - rect.top) / canvasScale;
    return { x: physX, y: physY };
  }

  function canvasCoordsLogical(e) {
    const phys = canvasCoords(e);
    return physicalToLogical(phys.x, phys.y);
  }

  function onCanvasMouseDown(e) {
    const p = canvasCoordsLogical(e);
    broadcastCursor(p);
    if (!tool) return;
    if (tool === "text") {
      pendingText = "";
      showTextInput = true;
      drawStart = p;
      setTimeout(() => {
        const input = document.getElementById("text-input-popup");
        if (input) input.focus();
      }, 10);
      return;
    }
    isDrawing = true;
    drawStart = p;
    drawCurrent = p;
    renderAnnotations();
  }

  function onCanvasMouseMove(e) {
    const p = canvasCoordsLogical(e);
    broadcastCursor(p);
    if (!isDrawing) return;
    drawCurrent = p;
    renderAnnotations();
  }

  async function onCanvasMouseUp(e) {
    if (!isDrawing) return;
    drawCurrent = canvasCoordsLogical(e);
    isDrawing = false;

    if (tool === "rect") {
      const x = Math.min(drawStart.x, drawCurrent.x);
      const y = Math.min(drawStart.y, drawCurrent.y);
      const w = Math.abs(drawCurrent.x - drawStart.x);
      const h = Math.abs(drawCurrent.y - drawStart.y);
      if (w > 3 / scaleFactor && h > 3 / scaleFactor) {
        await addAnnotation({
          type: "rect",
          x, y, width: w, height: h,
          color: toolColor,
          stroke_width: strokeWidth,
        });
      }
    } else if (tool === "arrow") {
      const dist = Math.hypot(drawCurrent.x - drawStart.x, drawCurrent.y - drawStart.y);
      if (dist > 5 / scaleFactor) {
        await addAnnotation({
          type: "arrow",
          x: drawStart.x,
          y: drawStart.y,
          end_x: drawCurrent.x,
          end_y: drawCurrent.y,
          color: toolColor,
          stroke_width: strokeWidth,
        });
      }
    }
    drawStart = null;
    drawCurrent = null;
    renderAnnotations();
  }

  async function addAnnotation(itemData) {
    const ts = frames[currentFrameIdx]?.timestamp_ms || 0;
    const id = crypto.randomUUID();
    const item = { id, ...itemData };

    let layer = annotations.find((a) => a.timestamp_ms === ts);
    if (!layer) {
      layer = { timestamp_ms: ts, items: [] };
      annotations.push(layer);
      annotations.sort((a, b) => a.timestamp_ms - b.timestamp_ms);
    }
    layer.items.push(item);
    await saveAnnotationsToBackend();
    if (roomCode) {
      await sendCollabEvent({
        type: "Annotation",
        data: {
          user_id: "local",
          timestamp_ms: ts,
          item,
        },
      });
    }
  }

  async function confirmTextInput() {
    if (!pendingText || !drawStart) {
      showTextInput = false;
      pendingText = "";
      drawStart = null;
      return;
    }
    await addAnnotation({
      type: "text",
      x: drawStart.x,
      y: drawStart.y,
      text: pendingText,
      color: toolColor,
      stroke_width: strokeWidth,
    });
    showTextInput = false;
    pendingText = "";
    drawStart = null;
    renderAnnotations();
  }

  async function saveAnnotationsToBackend() {
    try {
      await invoke("save_annotations", { recordingId, layers: annotations });
    } catch (e) {
      console.error("保存标注失败", e);
    }
  }

  async function deleteCurrentFrameAnnotations() {
    const ts = frames[currentFrameIdx]?.timestamp_ms;
    if (!ts) return;
    if (!confirm("确定删除当前时间点的所有标注？")) return;
    annotations = annotations.filter((a) => a.timestamp_ms !== ts);
    await saveAnnotationsToBackend();
    renderAnnotations();
  }

  function togglePlay() {
    isPlaying = !isPlaying;
    if (isPlaying) startPlayback();
    else stopPlayback();
    if (roomCode) {
      sendCollabEvent({
        type: isPlaying ? "Play" : "Pause",
        data: {
          user_id: "local",
          timestamp_ms: frames[currentFrameIdx]?.timestamp_ms || 0,
        },
      });
    }
  }

  function startPlayback() {
    stopPlayback();
    playTimer = setInterval(() => {
      let next = currentFrameIdx + 1;
      if (next >= frames.length) {
        next = 0;
      }
      renderFrame(next);
    }, 100 / playbackSpeed);
  }

  function stopPlayback() {
    if (playTimer) {
      clearInterval(playTimer);
      playTimer = null;
    }
  }

  function stepBackward() {
    if (isPlaying) togglePlay();
    if (currentFrameIdx > 0) renderFrame(currentFrameIdx - 1);
  }

  function stepForward() {
    if (isPlaying) togglePlay();
    if (currentFrameIdx < frames.length - 1) renderFrame(currentFrameIdx + 1);
  }

  function onSeek(idx) {
    if (isPlaying) togglePlay();
    renderFrame(idx);
    if (roomCode) {
      sendCollabEvent({
        type: "Seek",
        data: {
          user_id: "local",
          target_ms: frames[idx]?.timestamp_ms || 0,
        },
      });
    }
  }

  function currentTimestamp() {
    return frames[currentFrameIdx]?.timestamp_ms || 0;
  }

  function formatTime(ms) {
    const totalSec = Math.floor(ms / 1000);
    const m = Math.floor(totalSec / 60);
    const s = totalSec % 60;
    const cs = Math.floor((ms % 1000) / 100);
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${cs}`;
  }

  async function broadcastCursor(p) {
    if (!roomCode) return;
    await sendCollabEvent({
      type: "Cursor",
      data: {
        user_id: "local",
        x: p.x,
        y: p.y,
        timestamp_ms: frames[currentFrameIdx]?.timestamp_ms || 0,
      },
    });
  }

  function renderRemoteCursors() {
    if (!cursorCtx) return;
    cursorCtx.clearRect(0, 0, cursorCanvas.width, cursorCanvas.height);
    for (const userId of Object.keys(remoteCursors)) {
      const cur = remoteCursors[userId];
      if (!cur || cur.x == null || cur.y == null) continue;
      const { x, y, color, name } = cur;
      const px = x * scaleFactor;
      const py = y * scaleFactor;
      cursorCtx.save();
      cursorCtx.fillStyle = color || "#4ECDC4";
      cursorCtx.strokeStyle = "white";
      cursorCtx.lineWidth = 2;
      cursorCtx.beginPath();
      cursorCtx.moveTo(px, py);
      cursorCtx.lineTo(px + 16, py + 6);
      cursorCtx.lineTo(px + 6, py + 16);
      cursorCtx.closePath();
      cursorCtx.fill();
      cursorCtx.stroke();
      if (name) {
        cursorCtx.font = "12px sans-serif";
        cursorCtx.fillStyle = color || "#4ECDC4";
        cursorCtx.fillText(name, px + 20, py + 14);
      }
      cursorCtx.restore();
    }
  }

  async function createCollabRoom() {
    try {
      const r = await invoke("create_collab_room", { recordingId, userName });
      if (r.success) {
        roomCode = r.data;
        startCollabPolling();
      } else {
        alert(r.error || "创建房间失败");
      }
    } catch (e) {
      alert("创建房间失败: " + e);
    }
  }

  async function joinCollabRoom() {
    if (!joinRoomCode.trim()) {
      alert("请输入房间码");
      return;
    }
    try {
      const r = await invoke("join_collab_room", { roomCode: joinRoomCode.trim().toUpperCase(), userName });
      if (r.success) {
        roomCode = joinRoomCode.trim().toUpperCase();
        const info = r.data;
        peers = info.peers || [];
        if (info.recording_id && info.recording_id !== recordingId) {
          navigate(`/playback/${info.recording_id}`);
          return;
        }
        if (typeof info.current_ms === "number" && info.current_ms >= 0) {
          const idx = findFrameIndexByTs(info.current_ms);
          if (idx >= 0) renderFrame(idx);
        }
        if (typeof info.is_playing === "boolean" && info.is_playing !== isPlaying) {
          togglePlay();
        }
        startCollabPolling();
      } else {
        alert(r.error || "加入房间失败");
      }
    } catch (e) {
      alert("加入房间失败: " + e);
    }
  }

  async function leaveCollabRoom() {
    try {
      await invoke("leave_collab_room", { roomCode });
    } catch (e) {
      console.error(e);
    }
    roomCode = "";
    peers = [];
    remoteCursors = {};
    stopCollabPolling();
  }

  async function sendCollabEvent(event) {
    if (!roomCode) return;
    try {
      await invoke("send_collab_event", { roomCode, event });
    } catch (e) {
      console.error("发送协作事件失败", e);
    }
  }

  function startCollabPolling() {
    stopCollabPolling();
    collabPollTimer = setInterval(async () => {
      if (!roomCode) return;
      try {
        const r = await invoke("get_collab_room", { roomCode });
        if (r.success && r.data) {
          const info = r.data;
          peers = info.peers || [];
          for (const p of peers) {
            if (p.cursor_x != null && p.cursor_y != null) {
              remoteCursors[p.user_id] = {
                x: p.cursor_x,
                y: p.cursor_y,
                color: p.color,
                name: p.name,
              };
            }
          }
          renderRemoteCursors();
        }
      } catch (e) {
        console.error(e);
      }
    }, 200);
  }

  function stopCollabPolling() {
    if (collabPollTimer) {
      clearInterval(collabPollTimer);
      collabPollTimer = null;
    }
  }

  function findFrameIndexByTs(ts) {
    let best = 0;
    let bestDiff = Infinity;
    for (let i = 0; i < frames.length; i++) {
      const d = Math.abs((frames[i]?.timestamp_ms || 0) - ts);
      if (d < bestDiff) {
        bestDiff = d;
        best = i;
      }
    }
    return best;
  }

  async function buildClipIndex() {
    if (clipIndexing) return;
    clipIndexing = true;
    try {
      const sampleFrames = [];
      const step = Math.max(1, Math.floor(frames.length / 60));
      for (let i = 0; i < frames.length; i += step) {
        const f = frames[i];
        if (!f || !f.block_images || !f.rects) continue;
        const thumb = await renderFrameToThumbnail(f);
        if (thumb) {
          sampleFrames.push({
            timestamp_ms: f.timestamp_ms,
            rgba_base64: thumb,
            width: recording.width,
            height: recording.height,
          });
        }
      }
      const r = await invoke("clip_index", { recordingId, frames: sampleFrames });
      if (r.success) {
        clipIndexed = true;
        alert(`已索引 ${r.data} 个关键帧`);
      } else {
        alert(r.error || "索引失败");
      }
    } catch (e) {
      alert("索引失败: " + e);
    } finally {
      clipIndexing = false;
    }
  }

  async function renderFrameToThumbnail(frame) {
    if (!recording) return null;
    const c = document.createElement("canvas");
    c.width = Math.max(1, Math.floor(recording.width / 4));
    c.height = Math.max(1, Math.floor(recording.height / 4));
    const cx = c.getContext("2d");
    cx.fillStyle = "#000";
    cx.fillRect(0, 0, c.width, c.height);
    if (frame.block_images && frame.rects) {
      for (let j = 0; j < frame.rects.length && j < frame.block_images.length; j++) {
        const rect = frame.rects[j];
        const b64 = frame.block_images[j];
        if (!b64) continue;
        try {
          const img = await base64ToImage(b64);
          cx.drawImage(img, Math.floor(rect.x / 4), Math.floor(rect.y / 4),
            Math.max(1, Math.floor(rect.width / 4)), Math.max(1, Math.floor(rect.height / 4)));
        } catch (e) {
          // skip
        }
      }
    }
    const dataUrl = c.toDataURL("image/png");
    return dataUrl.split(",")[1];
  }

  function base64ToImage(b64) {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = reject;
      img.src = "data:image/png;base64," + b64;
    });
  }

  async function runClipSearch() {
    if (!clipQuery.trim()) {
      clipResults = [];
      return;
    }
    if (!clipIndexed) {
      alert("请先构建索引");
      return;
    }
    clipSearching = true;
    try {
      const r = await invoke("clip_search", { recordingId, query: clipQuery, topK: 8 });
      if (r.success) {
        clipResults = r.data || [];
      } else {
        alert(r.error || "搜索失败");
      }
    } catch (e) {
      alert("搜索失败: " + e);
    } finally {
      clipSearching = false;
    }
  }

  function jumpToClipResult(ts) {
    const idx = findFrameIndexByTs(ts);
    onSeek(idx);
    clipPanelOpen = false;
  }

  async function startExport() {
    if (exporting) return;
    try {
      const path = await save({
        defaultPath: `${recording?.title || "recording"}.webm`,
        filters: [{ name: "WebM Video", extensions: ["webm"] }],
      });
      if (!path) return;
      exporting = true;
      exportProgress = null;
      const result = await invoke("export_webm", { recordingId, outputPath: path });
      if (result.success) {
        alert(`导出完成: ${result.data[0]} 帧, ${Math.round(result.data[1] / 1024)} KB`);
      } else {
        alert(result.error || "导出失败");
      }
    } catch (e) {
      alert("导出失败: " + e);
    } finally {
      exporting = false;
      exportProgress = null;
    }
  }

  $: currentTs = currentTimestamp();
  $: duration = recording?.duration_ms || 0;
</script>

<div class="playback-page">
  <header class="page-header">
    <button class="back-btn" on:click={() => navigate("/library")}>
      ← 返回列表
    </button>
    <div class="header-info">
      <h1>{recording?.title || "加载中..."}</h1>
      {#if recording}
        <p class="subtitle">
          {recording.width}x{recording.height} · {frames.length} 帧 ·
          总时长 {formatTime(recording.duration_ms)} ·
          {annotations.reduce((s, a) => s + (a.items?.length || 0), 0)} 个标注
        </p>
      {/if}
    </div>
    <div class="header-actions">
      <button class="action-btn {collabPanelOpen ? 'active' : ''}" on:click={() => { collabPanelOpen = !collabPanelOpen; clipPanelOpen = false; exportPanelOpen = false; }}>
        👥 协作 {roomCode ? `(${roomCode})` : ""}
      </button>
      <button class="action-btn {clipPanelOpen ? 'active' : ''}" on:click={() => { clipPanelOpen = !clipPanelOpen; collabPanelOpen = false; exportPanelOpen = false; }}>
        🔍 智能剪辑
      </button>
      <button class="action-btn {exportPanelOpen ? 'active' : ''}" on:click={() => { exportPanelOpen = !exportPanelOpen; collabPanelOpen = false; clipPanelOpen = false; }}>
        ⬇ 导出
      </button>
    </div>
  </header>

  {#if loading}
    <div class="loading-overlay">
      <div class="spinner"></div>
      <p>正在加载录制数据...</p>
    </div>
  {/if}

  <div class="main-area">
    <div class="canvas-area">
      <div class="canvas-wrap" style="transform: scale({canvasScale});">
        <canvas bind:this={canvas} class="main-canvas" />
        <canvas bind:this={annCanvas} class="annotation-canvas"
          on:mousedown={onCanvasMouseDown}
          on:mousemove={onCanvasMouseMove}
          on:mouseup={onCanvasMouseUp}
          on:mouseleave={onCanvasMouseUp}
        />
        <canvas bind:this={cursorCanvas} class="cursor-canvas" />
        {#if showTextInput}
          <div class="text-input-popup" style="left: {drawStart?.x * canvasScale * scaleFactor}px; top: {drawStart?.y * canvasScale * scaleFactor}px;">
            <input id="text-input-popup" type="text" bind:value={pendingText} placeholder="输入文字..."
              on:keydown={(e) => { if (e.key === "Enter") confirmTextInput(); if (e.key === "Escape") { showTextInput = false; pendingText = ""; drawStart = null; } }}
            />
            <button class="confirm-btn" on:click={confirmTextInput}>✓</button>
          </div>
        {/if}
      </div>

      {#if collabPanelOpen}
        <div class="side-panel collab-panel">
          <h3>👥 协作观看</h3>
          <div class="form-row">
            <label>你的昵称</label>
            <input bind:value={userName} placeholder="输入昵称" />
          </div>
          {#if !roomCode}
            <div class="panel-section">
              <h4>创建房间</h4>
              <button class="primary-btn" on:click={createCollabRoom}>创建协作房间</button>
            </div>
            <div class="panel-section">
              <h4>加入房间</h4>
              <input bind:value={joinRoomCode} placeholder="输入6位房间码" maxlength="6" style="text-transform: uppercase;" />
              <button class="primary-btn" on:click={joinCollabRoom}>加入</button>
            </div>
          {:else}
            <div class="room-info">
              <div class="room-code-display">
                <span class="label">房间码</span>
                <span class="code">{roomCode}</span>
              </div>
              <button class="leave-btn" on:click={leaveCollabRoom}>离开房间</button>
            </div>
            <div class="panel-section">
              <h4>在线用户 ({peers.length})</h4>
              <div class="peers-list">
                {#each peers as p}
                  <div class="peer-item">
                    <span class="peer-dot" style="background: {p.color};"></span>
                    <span class="peer-name">{p.name}</span>
                  </div>
                {/each}
              </div>
            </div>
            <p class="hint">所有用户的播放、暂停、跳转、标注操作会实时同步</p>
          {/if}
        </div>
      {/if}

      {#if clipPanelOpen}
        <div class="side-panel clip-panel">
          <h3>🔍 智能剪辑</h3>
          <p class="hint">用文字描述你想找的场景（如"打开Chrome的画面"），系统会自动定位到对应时间点。</p>
          {#if !clipIndexed}
            <button class="primary-btn" disabled={clipIndexing} on:click={buildClipIndex}>
              {clipIndexing ? "正在构建索引..." : "先构建索引"}
            </button>
          {:else}
            <div class="form-row">
              <input bind:value={clipQuery} placeholder="描述你要找的场景..." on:keydown={(e) => e.key === "Enter" && runClipSearch()} />
              <button class="primary-btn" disabled={clipSearching} on:click={runClipSearch}>
                {clipSearching ? "搜索中..." : "搜索"}
              </button>
            </div>
            {#if clipResults.length > 0}
              <div class="clip-results">
                {#each clipResults as res}
                  <button class="clip-result-item" on:click={() => jumpToClipResult(res.timestamp_ms)}>
                    <span class="clip-time">{formatTime(res.timestamp_ms)}</span>
                    <span class="clip-score">{Math.round(res.score * 100)}%</span>
                  </button>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {/if}

      {#if exportPanelOpen}
        <div class="side-panel export-panel">
          <h3>⬇ 导出 WebM 视频</h3>
          <p class="hint">基于差异块合成渲染，比传统编码快约 10 倍。</p>
          <div class="export-info">
            <div><span>分辨率</span><strong>{recording?.width}x{recording?.height}</strong></div>
            <div><span>帧数</span><strong>{frames.length}</strong></div>
            <div><span>时长</span><strong>{formatTime(duration)}</strong></div>
          </div>
          {#if exporting}
            <div class="export-progress">
              <div class="progress-bar"><div class="progress-fill"></div></div>
              <p>正在导出...</p>
            </div>
          {/if}
          <button class="primary-btn" disabled={exporting || !recording} on:click={startExport}>
            {exporting ? "导出中..." : "导出为 WebM"}
          </button>
        </div>
      {/if}
    </div>

    <aside class="tools-panel">
      <AnnotationToolbar
        bind:tool
        bind:toolColor
        bind:strokeWidth
        on:delete={deleteCurrentFrameAnnotations}
      />

      <div class="playback-controls">
        <div class="time-display">
          <span class="time-current">{formatTime(currentTs)}</span>
          <span class="time-sep">/</span>
          <span class="time-total">{formatTime(duration)}</span>
        </div>
        <div class="frame-info">
          帧 {currentFrameIdx + 1} / {frames.length}
        </div>
        <div class="play-buttons">
          <button class="play-btn" on:click={stepBackward} title="上一帧">⏮</button>
          <button class="play-btn play-main" on:click={togglePlay} title="播放/暂停">
            {isPlaying ? "⏸" : "▶"}
          </button>
          <button class="play-btn" on:click={stepForward} title="下一帧">⏭</button>
        </div>
        <div class="speed-control">
          <label>播放速度</label>
          <select bind:value={playbackSpeed} on:change={() => { if (isPlaying) { stopPlayback(); startPlayback(); } }}>
            <option value={0.25}>0.25x</option>
            <option value={0.5}>0.5x</option>
            <option value={1}>1x (100ms/帧)</option>
            <option value={2}>2x</option>
            <option value={4}>4x</option>
          </select>
        </div>
      </div>
    </aside>
  </div>

  <Timeline
    {frames}
    {annotations}
    bind:currentFrameIdx
    on:seek={(e) => onSeek(e.detail)}
    duration={recording?.duration_ms || 0}
  />
</div>

<style>
  .playback-page {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 16px 24px;
    overflow: hidden;
    position: relative;
  }

  .page-header {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 12px;
    flex-shrink: 0;
  }

  .header-actions {
    margin-left: auto;
    display: flex;
    gap: 8px;
  }

  .action-btn {
    padding: 8px 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
  }
  .action-btn:hover { color: var(--text-primary); border-color: var(--accent); }
  .action-btn.active { background: var(--accent); color: white; border-color: var(--accent); }

  .back-btn {
    padding: 8px 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
  }

  .back-btn:hover {
    color: var(--text-primary);
    border-color: var(--accent);
  }

  .header-info h1 {
    font-size: 20px;
    font-weight: 700;
    margin-bottom: 2px;
  }

  .subtitle {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .loading-overlay {
    position: absolute;
    inset: 0;
    background: rgba(26, 26, 46, 0.95);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    z-index: 100;
    gap: 16px;
    color: var(--text-secondary);
  }

  .spinner {
    width: 48px;
    height: 48px;
    border: 4px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .main-area {
    flex: 1;
    display: flex;
    gap: 16px;
    min-height: 0;
    margin-bottom: 12px;
  }

  .canvas-area {
    flex: 1;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: auto;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    position: relative;
  }

  .canvas-wrap {
    position: relative;
    transform-origin: center center;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
  }

  .main-canvas {
    display: block;
    background: #000;
  }

  .annotation-canvas {
    position: absolute;
    inset: 0;
    cursor: crosshair;
  }

  .cursor-canvas {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 5;
  }

  .text-input-popup {
    position: absolute;
    z-index: 10;
    display: flex;
    gap: 4px;
    background: var(--bg-primary);
    border: 2px solid var(--accent);
    border-radius: 6px;
    padding: 4px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }

  .text-input-popup input {
    background: var(--bg-secondary);
    border: none;
    padding: 6px 8px;
    border-radius: 4px;
    color: white;
    font-size: 13px;
    width: 180px;
  }

  .confirm-btn {
    padding: 0 10px;
    background: var(--success);
    color: white;
    border-radius: 4px;
    font-weight: 700;
  }

  .side-panel {
    position: absolute;
    top: 12px;
    right: 12px;
    width: 280px;
    max-height: calc(100% - 24px);
    overflow-y: auto;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 16px;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow: 0 8px 32px rgba(0,0,0,0.5);
  }

  .side-panel h3 {
    font-size: 15px;
    font-weight: 700;
    margin: 0;
  }

  .side-panel h4 {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    margin: 0 0 8px 0;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .hint {
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.5;
    margin: 0;
  }

  .panel-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }

  .form-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .form-row label {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .side-panel input {
    padding: 8px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .primary-btn {
    padding: 8px 14px;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .primary-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .leave-btn {
    padding: 6px 12px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 12px;
  }

  .room-info {
    background: var(--bg-secondary);
    border-radius: 8px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .room-code-display {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .room-code-display .label { font-size: 12px; color: var(--text-secondary); }
  .room-code-display .code { font-size: 22px; font-weight: 800; letter-spacing: 3px; color: var(--accent); }

  .peers-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .peer-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    background: var(--bg-secondary);
    border-radius: 6px;
    font-size: 13px;
  }

  .peer-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .clip-results {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 240px;
    overflow-y: auto;
  }

  .clip-result-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-family: monospace;
    font-size: 13px;
    cursor: pointer;
    text-align: left;
  }
  .clip-result-item:hover { border-color: var(--accent); }
  .clip-score { color: var(--accent); font-weight: 600; }

  .export-info {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--bg-secondary);
    border-radius: 8px;
    padding: 12px;
  }
  .export-info > div {
    display: flex;
    justify-content: space-between;
    font-size: 13px;
  }
  .export-info span { color: var(--text-secondary); }

  .export-progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: center;
  }
  .progress-bar {
    width: 100%;
    height: 8px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    overflow: hidden;
  }
  .progress-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 4px;
    animation: pulse 1.5s ease-in-out infinite;
    width: 60%;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.7; }
    50% { opacity: 1; }
  }

  .tools-panel {
    width: 260px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex-shrink: 0;
  }

  .playback-controls {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .time-display {
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .time-current {
    font-size: 28px;
    font-weight: 700;
    color: var(--accent);
  }

  .time-sep {
    color: var(--text-secondary);
    margin: 0 6px;
  }

  .time-total {
    font-size: 18px;
    color: var(--text-secondary);
  }

  .frame-info {
    text-align: center;
    font-size: 12px;
    color: var(--text-secondary);
    padding: 6px;
    background: var(--bg-primary);
    border-radius: 6px;
  }

  .play-buttons {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
  }

  .play-btn {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
    border: none;
    cursor: pointer;
  }

  .play-btn:hover {
    background: var(--bg-primary);
    border: 1px solid var(--accent);
  }

  .play-btn.play-main {
    width: 60px;
    height: 60px;
    font-size: 20px;
    background: linear-gradient(135deg, var(--accent), #ff6b81);
    box-shadow: 0 4px 16px rgba(233, 69, 96, 0.4);
  }

  .speed-control {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .speed-control label {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .speed-control select {
    padding: 8px 10px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
  }
</style>
