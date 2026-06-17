<script>
  import { createEventDispatcher, onMount, tick } from "svelte";
  const dispatch = createEventDispatcher();

  export let frames = [];
  export let annotations = [];
  export let currentFrameIdx = 0;
  export let duration = 0;

  let timelineEl = null;
  let hoverIdx = -1;
  let showThumbnail = false;
  let thumbX = 0;
  let isDragging = false;
  let thumbnailCache = new Map();

  $: totalFrames = frames.length;
  $: annTimestamps = new Set(annotations.map((a) => a.timestamp_ms));

  function getFrameIdxFromX(clientX) {
    if (!timelineEl || totalFrames === 0) return 0;
    const rect = timelineEl.getBoundingClientRect();
    const x = Math.max(0, Math.min(clientX - rect.left, rect.width));
    const ratio = x / rect.width;
    return Math.max(0, Math.min(totalFrames - 1, Math.floor(ratio * totalFrames)));
  }

  function onMouseDown(e) {
    isDragging = true;
    const idx = getFrameIdxFromX(e.clientX);
    seekTo(idx);
  }

  function onMouseMove(e) {
    const idx = getFrameIdxFromX(e.clientX);
    hoverIdx = idx;
    if (timelineEl) {
      const rect = timelineEl.getBoundingClientRect();
      thumbX = Math.max(60, Math.min(e.clientX - rect.left, rect.width - 160));
    }
    showThumbnail = true;
    if (isDragging) {
      seekTo(idx);
    }
  }

  function onMouseUp(e) {
    if (isDragging) {
      const idx = getFrameIdxFromX(e.clientX);
      seekTo(idx);
    }
    isDragging = false;
  }

  function onMouseLeave() {
    isDragging = false;
    showThumbnail = false;
    hoverIdx = -1;
  }

  function seekTo(idx) {
    if (idx !== currentFrameIdx) {
      dispatch("seek", idx);
    }
  }

  function formatTime(ms) {
    const totalSec = Math.floor(ms / 1000);
    const m = Math.floor(totalSec / 60);
    const s = totalSec % 60;
    const cs = Math.floor((ms % 1000) / 100);
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${cs}`;
  }

  function onTimelineClick(e) {
    if (!isDragging) {
      const idx = getFrameIdxFromX(e.clientX);
      seekTo(idx);
    }
  }

  $: playheadPct = totalFrames > 0 ? (currentFrameIdx / totalFrames) * 100 : 0;
  $: hoverPct = totalFrames > 0 ? (hoverIdx / totalFrames) * 100 : 0;
  $: hoverTime = frames[hoverIdx]?.timestamp_ms || 0;
  $: thumbRects = frames[hoverIdx]?.rects || [];

  function annMarkerPct(ts) {
    return duration > 0 ? (ts / duration) * 100 : 0;
  }
</script>

<div class="timeline-container">
  <div class="timeline-header">
    <div class="time-marks">
      {#if duration > 0}
        {#each [0, 0.25, 0.5, 0.75, 1] as p}
          <div class="time-mark" style="left: {p * 100}%;">
            <span class="time-mark-text">{formatTime(Math.floor(duration * p))}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>

  <div
    class="timeline-track"
    bind:this={timelineEl}
    on:mousedown={onMouseDown}
    on:mousemove={onMouseMove}
    on:mouseup={onMouseUp}
    on:mouseleave={onMouseLeave}
    on:click={onTimelineClick}
    class:dragging={isDragging}
  >
    <div class="track-bg">
      {#each frames as f, i}
        <div
          class="frame-cell"
          style="left: {(i / totalFrames) * 100}%; width: {100 / totalFrames}%;"
          class:has-change={f.rects && f.rects.length > 0}
          class:current={i === currentFrameIdx}
        >
          {#if f.rects && f.rects.length > 0}
            <div class="change-bar" style="height: {Math.min(100, f.rects.length * 10)}%;"></div>
          {/if}
        </div>
      {/each}
    </div>

    {#each Array.from(annTimestamps) as ts}
      <div
        class="annotation-marker"
        style="left: {annMarkerPct(ts)}%;"
        title={`标注 @ ${formatTime(ts)}`}
      >
        <div class="marker-dot"></div>
        <div class="marker-line"></div>
      </div>
    {/each}

    <div class="hover-indicator" style="left: {hoverPct}%;" class:visible={showThumbnail && !isDragging}>
      <div class="hover-line"></div>
    </div>

    <div class="playhead" style="left: {playheadPct}%;">
      <div class="playhead-line"></div>
      <div class="playhead-handle">
        <div class="handle-inner"></div>
      </div>
    </div>

    {#if showThumbnail && hoverIdx >= 0}
      <div class="thumbnail-popup" style="left: {thumbX}px;">
        <div class="thumb-frame">
          <div class="thumb-frame-label">帧 {hoverIdx + 1}</div>
          <svg class="thumb-canvas" viewBox="0 0 160 90" preserveAspectRatio="xMidYMid meet">
            <rect x="0" y="0" width="160" height="90" fill="#1a1a2e" stroke="#2a2a4a" stroke-width="1"/>
            {#each thumbRects as r}
              <rect
                x={(r.x / (frames[hoverIdx]?.recording_width || 1920)) * 160}
                y={(r.y / (frames[hoverIdx]?.recording_height || 1080)) * 90}
                width={Math.max(2, (r.width / (frames[hoverIdx]?.recording_width || 1920)) * 160)}
                height={Math.max(2, (r.height / (frames[hoverIdx]?.recording_height || 1080)) * 90)}
                fill="#e94560"
                fill-opacity="0.7"
                stroke="#ff6b81"
                stroke-width="0.5"
              />
            {/each}
          </svg>
          <div class="thumb-changes">
            {thumbRects.length} 个变化块
          </div>
        </div>
        <div class="thumb-time">{formatTime(hoverTime)}</div>
      </div>
    {/if}
  </div>
</div>

<style>
  .timeline-container {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 12px 16px 16px;
    flex-shrink: 0;
    user-select: none;
  }

  .timeline-header {
    margin-bottom: 6px;
    position: relative;
    height: 18px;
  }

  .time-marks {
    position: relative;
    width: 100%;
    height: 100%;
  }

  .time-mark {
    position: absolute;
    transform: translateX(-50%);
  }

  .time-mark-text {
    font-size: 10px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .timeline-track {
    position: relative;
    height: 56px;
    background: var(--bg-primary);
    border-radius: 8px;
    cursor: pointer;
    overflow: visible;
  }

  .timeline-track.dragging {
    cursor: grabbing;
  }

  .track-bg {
    position: absolute;
    inset: 8px 0;
    display: flex;
    align-items: flex-end;
  }

  .frame-cell {
    position: absolute;
    height: 100%;
    display: flex;
    align-items: flex-end;
  }

  .change-bar {
    width: 100%;
    background: linear-gradient(180deg, #4ade80, #22c55e);
    border-radius: 1px 1px 0 0;
    min-height: 2px;
    opacity: 0.7;
  }

  .frame-cell.has-change:hover .change-bar {
    background: var(--accent);
    opacity: 1;
  }

  .frame-cell.current .change-bar {
    background: var(--accent);
    opacity: 1;
  }

  .annotation-marker {
    position: absolute;
    top: 0;
    height: 100%;
    transform: translateX(-50%);
    pointer-events: none;
    z-index: 2;
  }

  .marker-dot {
    position: absolute;
    top: 2px;
    left: 50%;
    transform: translateX(-50%);
    width: 8px;
    height: 8px;
    background: #fbbf24;
    border-radius: 50%;
    border: 1.5px solid var(--bg-secondary);
    box-shadow: 0 0 6px rgba(251, 191, 36, 0.6);
  }

  .marker-line {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    width: 1.5px;
    height: calc(100% - 12px);
    background: rgba(251, 191, 36, 0.4);
  }

  .hover-indicator {
    position: absolute;
    top: 0;
    height: 100%;
    transform: translateX(-50%);
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.1s;
    z-index: 3;
  }

  .hover-indicator.visible {
    opacity: 1;
  }

  .hover-line {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 1px;
    height: 100%;
    background: rgba(255, 255, 255, 0.4);
  }

  .playhead {
    position: absolute;
    top: -4px;
    height: calc(100% + 8px);
    transform: translateX(-50%);
    pointer-events: none;
    z-index: 5;
  }

  .playhead-line {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 2px;
    height: 100%;
    background: var(--accent);
    box-shadow: 0 0 8px rgba(233, 69, 96, 0.8);
  }

  .playhead-handle {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 18px;
    height: 14px;
    background: var(--accent);
    clip-path: polygon(50% 100%, 0 0, 100% 0);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .handle-inner {
    margin-top: 3px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: white;
  }

  .thumbnail-popup {
    position: absolute;
    top: -148px;
    transform: translateX(-50%);
    z-index: 10;
    pointer-events: none;
    filter: drop-shadow(0 8px 20px rgba(0, 0, 0, 0.6));
  }

  .thumb-frame {
    background: var(--bg-tertiary);
    border: 1.5px solid var(--accent);
    border-radius: 8px;
    padding: 8px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .thumb-frame-label {
    font-size: 10px;
    color: var(--accent);
    font-weight: 600;
  }

  .thumb-canvas {
    width: 160px;
    height: 90px;
    border-radius: 4px;
    background: var(--bg-primary);
  }

  .thumb-changes {
    font-size: 10px;
    color: var(--success);
    font-weight: 500;
  }

  .thumb-time {
    text-align: center;
    margin-top: 4px;
    font-size: 11px;
    color: white;
    font-weight: 600;
    background: var(--accent);
    padding: 3px 8px;
    border-radius: 4px;
    font-variant-numeric: tabular-nums;
  }
</style>
