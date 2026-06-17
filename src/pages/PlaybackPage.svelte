<script>
  import { invoke } from "@tauri-apps/api/tauri";
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
  let ctx = null;
  let annCtx = null;

  let loading = true;
  let frameBuffers = []; // 缓存已加载的帧图像块
  let frameRects = []; // 每帧的差异矩形
  let currentFrameIdx = 0;
  let isPlaying = false;
  let playTimer = null;
  let playbackSpeed = 1;

  let tool = null; // null | 'rect' | 'arrow' | 'text'
  let toolColor = "#e94560";
  let strokeWidth = 3;
  let isDrawing = false;
  let drawStart = null;
  let drawCurrent = null;
  let pendingText = "";
  let showTextInput = false;

  let canvasScale = 1;
  let canvasOffset = { x: 0, y: 0 };
  let scaleFactor = 1.0;

  let baseFrameData = null;

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
    if (!canvas || !annCanvas) return;
    const w = recording?.width || 800;
    const h = recording?.height || 500;
    canvas.width = w;
    canvas.height = h;
    annCanvas.width = w;
    annCanvas.height = h;
    ctx = canvas.getContext("2d");
    annCtx = annCanvas.getContext("2d");
    ctx.imageSmoothingEnabled = false;
    annCtx.imageSmoothingEnabled = false;

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
    if (!tool) return;
    const p = canvasCoordsLogical(e);
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
    if (!isDrawing) return;
    drawCurrent = canvasCoordsLogical(e);
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
          {recording.width}x{recording.height} (物理) / {Math.round(recording.logical_width)}x{Math.round(recording.logical_height)} (逻辑, {recording.scale_factor}x DPI) · {frames.length} 帧 ·
          总时长 {formatTime(recording.duration_ms)} ·
          {annotations.reduce((s, a) => s + (a.items?.length || 0), 0)} 个标注
        </p>
      {/if}
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
        <canvas
          bind:this={canvas}
          class="main-canvas"
        />
        <canvas
          bind:this={annCanvas}
          class="annotation-canvas"
          on:mousedown={onCanvasMouseDown}
          on:mousemove={onCanvasMouseMove}
          on:mouseup={onCanvasMouseUp}
          on:mouseleave={onCanvasMouseUp}
        />
        {#if showTextInput}
          <div
            class="text-input-popup"
            style="left: {drawStart?.x * canvasScale}px; top: {drawStart?.y * canvasScale}px;"
          >
            <input
              id="text-input-popup"
              type="text"
              bind:value={pendingText}
              placeholder="输入文字..."
              on:keydown={(e) => { if (e.key === "Enter") confirmTextInput(); if (e.key === "Escape") { showTextInput = false; pendingText = ""; drawStart = null; } }}
            />
            <button class="confirm-btn" on:click={confirmTextInput}>✓</button>
          </div>
        {/if}
      </div>
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
