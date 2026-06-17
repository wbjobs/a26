<script>
  import { createEventDispatcher } from "svelte";
  const dispatch = createEventDispatcher();

  export let tool = null;
  export let toolColor = "#e94560";
  export let strokeWidth = 3;

  const colors = [
    "#e94560", "#ef4444", "#f97316", "#f59e0b", "#fbbf24",
    "#84cc16", "#22c55e", "#10b981", "#14b8a6", "#06b6d4",
    "#3b82f6", "#6366f1", "#8b5cf6", "#a855f7", "#ec4899",
    "#ffffff", "#94a3b8", "#64748b", "#000000",
  ];

  const widths = [1, 2, 3, 5, 8];

  function setTool(t) {
    tool = tool === t ? null : t;
  }
</script>

<div class="annotation-toolbar">
  <div class="toolbar-section">
    <div class="section-label">标注工具</div>
    <div class="tool-grid">
      <button
        class="tool-btn {tool === 'rect' ? 'active' : ''}"
        on:click={() => setTool('rect')}
        title="矩形框 (标记区域)"
      >
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="3" y="5" width="18" height="14" rx="1"/>
        </svg>
        <span>矩形</span>
      </button>
      <button
        class="tool-btn {tool === 'arrow' ? 'active' : ''}"
        on:click={() => setTool('arrow')}
        title="箭头 (指向标注)"
      >
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="4" y1="20" x2="20" y2="4"/>
          <polyline points="9,4 20,4 20,15"/>
        </svg>
        <span>箭头</span>
      </button>
      <button
        class="tool-btn {tool === 'text' ? 'active' : ''}"
        on:click={() => setTool('text')}
        title="文字 (文字说明)"
      >
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="4,7 4,4 20,4 20,7"/>
          <line x1="9" y1="20" x2="15" y2="20"/>
          <line x1="12" y1="4" x2="12" y2="20"/>
        </svg>
        <span>文字</span>
      </button>
    </div>
    {#if tool}
      <div class="current-tool">
        当前工具：<strong>{tool === 'rect' ? '矩形框' : tool === 'arrow' ? '箭头' : '文字'}</strong>
        <button class="cancel-btn" on:click={() => tool = null}>取消</button>
      </div>
    {:else}
      <div class="tool-hint">选择工具后在画面上绘制</div>
    {/if}
  </div>

  <div class="toolbar-section">
    <div class="section-label">颜色</div>
    <div class="color-grid">
      {#each colors as c}
        <button
          class="color-swatch {c === toolColor ? 'selected' : ''}"
          style="background: {c};"
          on:click={() => toolColor = c}
          title={c}
        >
          {#if c === toolColor}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={isLight(c) ? '#000' : '#fff'} stroke-width="3">
              <polyline points="5,12 10,17 20,7"/>
            </svg>
          {/if}
        </button>
      {/each}
    </div>
  </div>

  <div class="toolbar-section">
    <div class="section-label">线宽 / 字号</div>
    <div class="width-grid">
      {#each widths as w}
        <button
          class="width-btn {strokeWidth === w ? 'active' : ''}"
          on:click={() => strokeWidth = w}
          title={`${w}px`}
        >
          <div class="width-preview" style="height: {w}px;"></div>
          <span>{w}px</span>
        </button>
      {/each}
    </div>
  </div>

  <div class="toolbar-section">
    <button class="delete-btn" on:click={() => dispatch('delete')}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polyline points="3,6 5,6 21,6"/>
        <path d="M19,6v14a2,2,0,0,1-2,2H7a2,2,0,0,1-2-2V6M8,6V4a2,2,0,0,1,2-2h4a2,2,0,0,1,2,2v2"/>
      </svg>
      删除当前帧标注
    </button>
  </div>

  <div class="toolbar-tips">
    <div class="tip-title">💡 使用说明</div>
    <ul>
      <li>矩形：拖拽选择区域</li>
      <li>箭头：拖拽起点到终点</li>
      <li>文字：点击位置后输入</li>
      <li>标注自动随帧时间保存</li>
    </ul>
  </div>
</div>

<style>
  .annotation-toolbar {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .toolbar-section {
    padding-bottom: 12px;
    border-bottom: 1px solid var(--border);
  }

  .toolbar-section:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .section-label {
    font-size: 11px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
    font-weight: 600;
  }

  .tool-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }

  .tool-btn {
    background: var(--bg-primary);
    border: 1.5px solid var(--border);
    border-radius: 8px;
    padding: 8px 4px;
    color: var(--text-secondary);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-weight: 500;
  }

  .tool-btn:hover {
    border-color: var(--accent);
    color: var(--text-primary);
  }

  .tool-btn.active {
    background: linear-gradient(135deg, var(--accent), #ff6b81);
    border-color: transparent;
    color: white;
    box-shadow: 0 2px 8px rgba(233, 69, 96, 0.3);
  }

  .current-tool {
    margin-top: 8px;
    padding: 6px 8px;
    background: rgba(233, 69, 96, 0.1);
    border-radius: 6px;
    font-size: 11px;
    color: var(--text-secondary);
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .current-tool strong {
    color: var(--accent);
  }

  .cancel-btn {
    background: transparent;
    color: var(--text-secondary);
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 4px;
  }

  .cancel-btn:hover {
    background: var(--bg-primary);
    color: var(--accent);
  }

  .tool-hint {
    margin-top: 8px;
    font-size: 11px;
    color: var(--text-secondary);
    text-align: center;
    font-style: italic;
  }

  .color-grid {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 5px;
  }

  .color-swatch {
    aspect-ratio: 1;
    border-radius: 6px;
    border: 2px solid transparent;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s;
  }

  .color-swatch:hover {
    transform: scale(1.1);
  }

  .color-swatch.selected {
    border-color: var(--text-primary);
    box-shadow: 0 0 0 2px var(--accent);
  }

  .width-grid {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 4px;
  }

  .width-btn {
    background: var(--bg-primary);
    border: 1.5px solid var(--border);
    border-radius: 6px;
    padding: 6px 2px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    color: var(--text-secondary);
    font-size: 10px;
  }

  .width-btn:hover {
    border-color: var(--accent);
    color: var(--text-primary);
  }

  .width-btn.active {
    background: var(--bg-tertiary);
    border-color: var(--accent);
    color: var(--accent);
  }

  .width-preview {
    width: 70%;
    background: var(--accent);
    border-radius: 2px;
  }

  .delete-btn {
    width: 100%;
    padding: 8px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 8px;
    color: #f87171;
    font-size: 12px;
    font-weight: 500;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .delete-btn:hover {
    background: rgba(239, 68, 68, 0.2);
    border-color: #ef4444;
  }

  .toolbar-tips {
    background: var(--bg-tertiary);
    border-radius: 8px;
    padding: 10px;
  }

  .tip-title {
    font-size: 11px;
    color: var(--accent);
    font-weight: 600;
    margin-bottom: 6px;
  }

  .toolbar-tips ul {
    list-style: none;
    font-size: 10px;
    color: var(--text-secondary);
    line-height: 1.8;
  }

  .toolbar-tips li::before {
    content: "• ";
    color: var(--accent);
  }
</style>

<script context="module">
  function isLight(hex) {
    const c = hex.replace('#', '');
    const r = parseInt(c.substring(0, 2), 16);
    const g = parseInt(c.substring(2, 4), 16);
    const b = parseInt(c.substring(4, 6), 16);
    return (r * 299 + g * 587 + b * 114) / 1000 > 128;
  }
</script>
