<script>
  import { invoke } from "@tauri-apps/api/tauri";
  import { onMount } from "svelte";
  import { navigate } from "svelte-router-spa";

  let status = "idle"; // idle, recording, paused, auto_paused
  let windowTitle = "";
  let elapsed = 0;
  let frameCount = 0;
  let autoPausedReason = null;
  let elapsedTimer = null;
  let livePreview = null;

  onMount(() => {
    checkStatus();
  });

  async function checkStatus() {
    try {
      const result = await invoke("get_recording_status");
      if (result && result.success) {
        status = result.data.status;
        elapsed = result.data.elapsed_ms || 0;
        frameCount = result.data.frame_count || 0;
        windowTitle = result.data.title || "";
        autoPausedReason = result.data.auto_paused_reason || null;
        if (status === "auto_paused" && !autoPausedReason) {
          autoPausedReason = "unknown";
        }
      }
    } catch (e) {
      console.error(e);
    }
  }

  async function startRecording() {
    try {
      const title = windowTitle || `录制_${new Date().toLocaleString("zh-CN")}`;
      const result = await invoke("start_recording", { title });
      if (result && result.success) {
        status = "recording";
        elapsed = 0;
        frameCount = 0;
        autoPausedReason = null;
        startElapsedTimer();
      }
    } catch (e) {
      alert("启动录制失败: " + e);
    }
  }

  async function pauseRecording() {
    try {
      const result = await invoke("pause_recording");
      if (result && result.success) {
        status = "paused";
        stopElapsedTimer();
      }
    } catch (e) {
      alert("暂停失败: " + e);
    }
  }

  async function resumeRecording() {
    try {
      const result = await invoke("resume_recording");
      if (result && result.success) {
        status = "recording";
        startElapsedTimer();
      }
    } catch (e) {
      alert("恢复失败: " + e);
    }
  }

  async function stopRecording() {
    try {
      stopElapsedTimer();
      const result = await invoke("stop_recording");
      if (result && result.success) {
        status = "idle";
        const id = result.data.id;
        setTimeout(() => {
          navigate(`/playback/${id}`);
        }, 500);
      } else {
        status = "idle";
      }
    } catch (e) {
      status = "idle";
      alert("停止录制失败: " + e);
    }
  }

  function startElapsedTimer() {
    stopElapsedTimer();
    elapsedTimer = setInterval(async () => {
      elapsed += 100;
      try {
        const result = await invoke("get_recording_status");
        if (result && result.success) {
          frameCount = result.data.frame_count || frameCount;
          if (result.data.status === "auto_paused") {
            status = "auto_paused";
            autoPausedReason = result.data.auto_paused_reason || "unknown";
            stopElapsedTimer();
          }
        }
      } catch (e) {}
    }, 100);
  }

  function stopElapsedTimer() {
    if (elapsedTimer) {
      clearInterval(elapsedTimer);
      elapsedTimer = null;
    }
  }

  function formatTime(ms) {
    const totalSec = Math.floor(ms / 1000);
    const h = Math.floor(totalSec / 3600);
    const m = Math.floor((totalSec % 3600) / 60);
    const s = totalSec % 60;
    const cs = Math.floor((ms % 1000) / 100);
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${cs}`;
  }

  $: elapsedStr = formatTime(elapsed);
</script>

<div class="recorder-page">
  <header class="page-header">
    <h1>屏幕录制</h1>
    <p class="subtitle">像素级增量录制 · 每隔 100ms 采样 · 仅存储差异区域</p>
  </header>

  <div class="content-grid">
    <div class="preview-panel">
      <div class="preview-container">
        <canvas
          bind:this={livePreview}
          class="preview-canvas"
          width={800}
          height={500}
        />
        {#if status === "idle"}
          <div class="preview-placeholder">
            <div class="placeholder-icon">📷</div>
            <p>准备就绪，点击下方按钮开始录制</p>
            <p class="hint">系统将捕获整个屏幕并以增量方式存储</p>
          </div>
        {/if}
        {#if status === "recording"}
          <div class="recording-indicator">
            <span class="pulse-dot"></span>
            <span>录制中</span>
          </div>
        {/if}
        {#if status === "paused"}
          <div class="paused-overlay">
            <span class="pause-icon">⏸</span>
            <span>已暂停</span>
          </div>
        {/if}
        {#if status === "auto_paused"}
          <div class="auto-paused-overlay">
            <span class="auto-icon">💤</span>
            <span>自动暂停</span>
            <span class="auto-reason">{autoPausedReason === "system_suspend" ? "系统休眠" : autoPausedReason === "display_off" ? "显示器关闭" : autoPausedReason === "blank_frame" ? "画面为空" : "系统事件"}</span>
            <span class="auto-hint">系统恢复后自动继续录制</span>
          </div>
        {/if}
      </div>
    </div>

    <div class="control-panel">
      <div class="control-section">
        <label class="field-label">录制标题</label>
        <input
          class="title-input"
          type="text"
          bind:value={windowTitle}
          placeholder="可选：为本次录制命名"
          disabled={status !== "idle"}
        />
      </div>

      <div class="stats-card">
        <div class="stat-item">
          <div class="stat-value">{elapsedStr}</div>
          <div class="stat-label">录制时长</div>
        </div>
        <div class="stat-item">
          <div class="stat-value">{frameCount}</div>
          <div class="stat-label">已采集帧</div>
        </div>
        <div class="stat-item">
          <div class="stat-value">{status === "idle" ? "空闲" : status === "recording" ? "录制" : status === "auto_paused" ? "自动暂停" : "暂停"}</div>
          <div class="stat-label">当前状态</div>
        </div>
      </div>

      <div class="control-section">
        <div class="button-group">
          {#if status === "idle"}
            <button class="btn btn-primary btn-large" on:click={startRecording}>
              <span class="btn-icon">●</span>
              开始录制
            </button>
          {/if}
          {#if status === "recording"}
            <button class="btn btn-warning" on:click={pauseRecording}>
              <span class="btn-icon">⏸</span>
              暂停
            </button>
            <button class="btn btn-danger btn-large" on:click={stopRecording}>
              <span class="btn-icon">■</span>
              停止并保存
            </button>
          {/if}
          {#if status === "paused"}
            <button class="btn btn-primary" on:click={resumeRecording}>
              <span class="btn-icon">▶</span>
              继续录制
            </button>
            <button class="btn btn-danger" on:click={stopRecording}>
              <span class="btn-icon">■</span>
              停止并保存
            </button>
          {/if}
          {#if status === "auto_paused"}
            <div class="auto-paused-info">
              💤 录制已自动暂停：{autoPausedReason === "system_suspend" ? "系统休眠" : autoPausedReason === "display_off" ? "显示器关闭" : autoPausedReason === "blank_frame" ? "画面为空" : "系统事件"}
            </div>
            <button class="btn btn-danger" on:click={stopRecording}>
              <span class="btn-icon">■</span>
              停止并保存
            </button>
          {/if}
        </div>
      </div>

      <div class="info-card">
        <h4>💡 使用提示</h4>
        <ul>
          <li>每 100ms 截取一帧画面</li>
          <li>OpenCV 运动检测，仅记录运动区域</li>
          <li>系统休眠/显示器关闭自动暂停</li>
          <li>支持不同 DPI 缩放的坐标映射</li>
          <li>仅存储变化区域，节省空间 90%+</li>
          <li>支持回放时逐帧叠加差异块</li>
          <li>可在回放中添加标注（框/箭头/文字）</li>
        </ul>
      </div>
    </div>
  </div>
</div>

<style>
  .recorder-page {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 24px;
    overflow: auto;
  }

  .page-header {
    margin-bottom: 24px;
  }

  .page-header h1 {
    font-size: 28px;
    font-weight: 700;
    margin-bottom: 4px;
  }

  .subtitle {
    color: var(--text-secondary);
    font-size: 14px;
  }

  .content-grid {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 340px;
    gap: 24px;
    min-height: 0;
  }

  .preview-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .preview-container {
    flex: 1;
    background: var(--bg-secondary);
    border-radius: 16px;
    border: 2px solid var(--border);
    position: relative;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 500px;
  }

  .preview-canvas {
    max-width: 100%;
    max-height: 100%;
    display: none;
  }

  .preview-placeholder {
    text-align: center;
    color: var(--text-secondary);
  }

  .placeholder-icon {
    font-size: 80px;
    margin-bottom: 16px;
    opacity: 0.5;
  }

  .preview-placeholder p {
    font-size: 16px;
    margin-bottom: 8px;
  }

  .preview-placeholder .hint {
    font-size: 13px;
    opacity: 0.7;
  }

  .recording-indicator {
    position: absolute;
    top: 16px;
    left: 16px;
    background: var(--accent);
    color: white;
    padding: 8px 14px;
    border-radius: 20px;
    font-size: 13px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
    box-shadow: 0 4px 12px rgba(233, 69, 96, 0.4);
  }

  .pulse-dot {
    width: 8px;
    height: 8px;
    background: white;
    border-radius: 50%;
    animation: pulse 1s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(0.8); }
  }

  .paused-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: white;
    font-size: 24px;
    font-weight: 600;
  }

  .pause-icon {
    font-size: 64px;
  }

  .auto-paused-overlay {
    position: absolute;
    inset: 0;
    background: rgba(245, 158, 11, 0.15);
    backdrop-filter: blur(4px);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: white;
    font-size: 24px;
    font-weight: 600;
    border: 2px solid rgba(245, 158, 11, 0.5);
    border-radius: 16px;
  }

  .auto-icon {
    font-size: 64px;
    animation: float 2s ease-in-out infinite;
  }

  @keyframes float {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-8px); }
  }

  .auto-reason {
    font-size: 16px;
    color: #fbbf24;
    font-weight: 500;
    background: rgba(0, 0, 0, 0.4);
    padding: 4px 12px;
    border-radius: 8px;
  }

  .auto-hint {
    font-size: 12px;
    color: var(--text-secondary);
    font-weight: 400;
  }

  .auto-paused-info {
    font-size: 13px;
    color: #fbbf24;
    text-align: center;
    padding: 10px;
    background: rgba(245, 158, 11, 0.1);
    border-radius: 8px;
    border: 1px solid rgba(245, 158, 11, 0.3);
  }

  .control-panel {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .control-section {
    background: var(--bg-secondary);
    border-radius: 12px;
    padding: 16px;
    border: 1px solid var(--border);
  }

  .field-label {
    display: block;
    font-size: 13px;
    color: var(--text-secondary);
    margin-bottom: 8px;
    font-weight: 500;
  }

  .title-input {
    width: 100%;
    padding: 12px 14px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
    transition: border-color 0.2s;
  }

  .title-input:focus {
    border-color: var(--accent);
  }

  .title-input:disabled {
    opacity: 0.5;
  }

  .stats-card {
    background: var(--bg-secondary);
    border-radius: 12px;
    padding: 16px;
    border: 1px solid var(--border);
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }

  .stat-item {
    text-align: center;
  }

  .stat-value {
    font-size: 20px;
    font-weight: 700;
    color: var(--accent);
    margin-bottom: 4px;
    font-variant-numeric: tabular-nums;
  }

  .stat-label {
    font-size: 11px;
    color: var(--text-secondary);
  }

  .button-group {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .btn {
    padding: 12px 20px;
    border-radius: 10px;
    font-size: 14px;
    font-weight: 600;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .btn-large {
    padding: 16px 20px;
    font-size: 16px;
  }

  .btn-primary {
    background: linear-gradient(135deg, var(--accent), #ff6b81);
    color: white;
    box-shadow: 0 4px 12px rgba(233, 69, 96, 0.3);
  }

  .btn-primary:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 6px 16px rgba(233, 69, 96, 0.4);
  }

  .btn-danger {
    background: linear-gradient(135deg, #ef4444, #f87171);
    color: white;
  }

  .btn-warning {
    background: linear-gradient(135deg, #f59e0b, #fbbf24);
    color: white;
  }

  .btn-icon {
    font-size: 12px;
  }

  .info-card {
    background: linear-gradient(135deg, rgba(233, 69, 96, 0.05), rgba(15, 52, 96, 0.3));
    border-radius: 12px;
    padding: 16px;
    border: 1px solid rgba(233, 69, 96, 0.2);
  }

  .info-card h4 {
    margin-bottom: 10px;
    font-size: 14px;
    color: var(--accent);
  }

  .info-card ul {
    list-style: none;
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 2;
  }

  .info-card li::before {
    content: "✓ ";
    color: var(--success);
    font-weight: 700;
  }
</style>
