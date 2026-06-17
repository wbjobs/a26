<script>
  import { invoke } from "@tauri-apps/api/tauri";
  import { onMount } from "svelte";
  import { navigate } from "svelte-router-spa";

  let recordings = [];
  let filtered = [];
  let searchTitle = "";
  let searchDate = "";
  let loading = true;

  onMount(async () => {
    await loadList();
  });

  async function loadList() {
    loading = true;
    try {
      const result = await invoke("list_recordings");
      if (result && result.success) {
        recordings = result.data || [];
        applyFilter();
      }
    } catch (e) {
      console.error(e);
    } finally {
      loading = false;
    }
  }

  async function doSearch() {
    if (!searchTitle && !searchDate) {
      await loadList();
      return;
    }
    loading = true;
    try {
      const result = await invoke("search_recordings", {
        titleQuery: searchTitle || null,
        dateQuery: searchDate || null,
      });
      // NOTE: Tauri auto-converts snake_case args, both formats work
      if (result && result.success) {
        recordings = result.data || [];
        applyFilter();
      }
    } catch (e) {
      console.error(e);
    } finally {
      loading = false;
    }
  }

  function applyFilter() {
    filtered = recordings;
  }

  function resetFilters() {
    searchTitle = "";
    searchDate = "";
    loadList();
  }

  function openPlayback(id) {
    navigate(`/playback/${id}`);
  }

  async function deleteRecording(id, ev) {
    ev.stopPropagation();
    if (!confirm("确定删除此录制及其所有标注数据？此操作不可恢复。")) return;
    try {
      const result = await invoke("delete_recording", { recordingId: id });
      if (result && result.success) {
        recordings = recordings.filter((r) => r.id !== id);
        applyFilter();
      }
    } catch (e) {
      alert("删除失败: " + e);
    }
  }

  function formatTime(ms) {
    const totalSec = Math.floor(ms / 1000);
    const h = Math.floor(totalSec / 3600);
    const m = Math.floor((totalSec % 3600) / 60);
    const s = totalSec % 60;
    if (h > 0) return `${h}h${m}m${s}s`;
    if (m > 0) return `${m}m${s}s`;
    return `${s}s`;
  }

  function formatDateTime(tsSec) {
    const d = new Date(tsSec * 1000);
    return d.toLocaleString("zh-CN", { hour12: false });
  }

  function formatDate(tsSec) {
    const d = new Date(tsSec * 1000);
    return d.toLocaleDateString("zh-CN");
  }

  function humanSize(bytes) {
    if (!bytes) return "-";
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
    return (bytes / 1024 / 1024).toFixed(2) + " MB";
  }
</script>

<div class="library-page">
  <header class="page-header">
    <div class="header-left">
      <h1>录制库</h1>
      <p class="subtitle">共 {recordings.length} 条录制记录</p>
    </div>
  </header>

  <div class="search-bar">
    <div class="search-field">
      <span class="search-icon">🔍</span>
      <input
        type="text"
        bind:value={searchTitle}
        placeholder="按窗口标题搜索..."
        on:keydown={(e) => e.key === "Enter" && doSearch()}
      />
    </div>
    <div class="search-field">
      <span class="search-icon">📅</span>
      <input
        type="date"
        bind:value={searchDate}
        on:change={doSearch}
      />
    </div>
    <button class="btn btn-primary" on:click={doSearch}>搜索</button>
    <button class="btn btn-secondary" on:click={resetFilters}>重置</button>
  </div>

  <div class="recordings-container">
    {#if loading}
      <div class="loading-state">
        <div class="spinner"></div>
        <p>加载中...</p>
      </div>
    {:else if filtered.length === 0}
      <div class="empty-state">
        <div class="empty-icon">📭</div>
        <h3>暂无录制记录</h3>
        <p>{searchTitle || searchDate ? "没有找到匹配的录制" : "去录制页面创建你的第一个屏幕录制吧"}</p>
        {#if !searchTitle && !searchDate}
          <button class="btn btn-primary" on:click={() => navigate("/")}>
            前往录制
          </button>
        {/if}
      </div>
    {:else}
      <div class="recordings-grid">
        {#each filtered as r (r.id)}
          <div class="recording-card" on:click={() => openPlayback(r.id)}>
            <div class="card-thumbnail">
              {#if r.thumbnail}
                <img src={"data:image/png;base64," + r.thumbnail} alt={r.title} />
              {:else}
                <div class="thumbnail-placeholder">
                  <span>🎬</span>
                </div>
              {/if}
              <div class="card-badges">
                <span class="badge badge-primary">{r.frame_count} 帧</span>
                <span class="badge badge-info">{r.width}x{r.height}</span>
              </div>
            </div>
            <div class="card-body">
              <h3 class="card-title" title={r.title}>{r.title}</h3>
              <div class="card-meta">
                <div class="meta-row">
                  <span class="meta-label">⏱ 时长</span>
                  <span class="meta-value">{formatTime(r.duration_ms)}</span>
                </div>
                <div class="meta-row">
                  <span class="meta-label">📅 日期</span>
                  <span class="meta-value">{formatDate(r.start_time)}</span>
                </div>
                <div class="meta-row">
                  <span class="meta-label">🕐 开始</span>
                  <span class="meta-value">{formatDateTime(r.start_time)}</span>
                </div>
                <div class="meta-row">
                  <span class="meta-label">📦 大小</span>
                  <span class="meta-value">{humanSize(r.total_size)}</span>
                </div>
              </div>
            </div>
            <div class="card-actions" on:click|stopPropagation>
              <button class="action-btn" on:click={() => openPlayback(r.id)} title="播放">
                ▶ 回放
              </button>
              <button class="action-btn danger" on:click={(e) => deleteRecording(r.id, e)} title="删除">
                🗑 删除
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .library-page {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 24px;
    overflow: hidden;
  }

  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    margin-bottom: 20px;
  }

  .header-left h1 {
    font-size: 28px;
    font-weight: 700;
    margin-bottom: 4px;
  }

  .subtitle {
    color: var(--text-secondary);
    font-size: 14px;
  }

  .search-bar {
    display: flex;
    gap: 12px;
    margin-bottom: 20px;
    background: var(--bg-secondary);
    padding: 14px;
    border-radius: 12px;
    border: 1px solid var(--border);
  }

  .search-field {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 14px;
    font-size: 14px;
    pointer-events: none;
    opacity: 0.6;
  }

  .search-field input {
    width: 100%;
    padding: 10px 14px 10px 38px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
  }

  .search-field input:focus {
    border-color: var(--accent);
  }

  .search-field input[type="date"]::-webkit-calendar-picker-indicator {
    filter: invert(0.8);
  }

  .btn {
    padding: 10px 18px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
  }

  .btn-primary {
    background: linear-gradient(135deg, var(--accent), #ff6b81);
    color: white;
  }

  .btn-secondary {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border);
  }

  .btn-secondary:hover {
    background: var(--bg-primary);
  }

  .recordings-container {
    flex: 1;
    overflow: auto;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-secondary);
    gap: 12px;
  }

  .spinner {
    width: 40px;
    height: 40px;
    border: 3px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .empty-icon {
    font-size: 72px;
    opacity: 0.5;
  }

  .empty-state h3 {
    font-size: 20px;
    color: var(--text-primary);
  }

  .recordings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 20px;
    padding-bottom: 20px;
  }

  .recording-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 14px;
    overflow: hidden;
    cursor: pointer;
    transition: all 0.2s ease;
    display: flex;
    flex-direction: column;
  }

  .recording-card:hover {
    transform: translateY(-4px);
    border-color: var(--accent);
    box-shadow: 0 12px 28px rgba(233, 69, 96, 0.15);
  }

  .card-thumbnail {
    position: relative;
    width: 100%;
    padding-top: 56.25%;
    background: var(--bg-primary);
    overflow: hidden;
  }

  .card-thumbnail img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .thumbnail-placeholder {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 48px;
    opacity: 0.3;
  }

  .card-badges {
    position: absolute;
    top: 10px;
    right: 10px;
    display: flex;
    gap: 6px;
  }

  .badge {
    padding: 3px 8px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 600;
    backdrop-filter: blur(8px);
  }

  .badge-primary {
    background: rgba(233, 69, 96, 0.9);
    color: white;
  }

  .badge-info {
    background: rgba(15, 52, 96, 0.9);
    color: #93c5fd;
  }

  .card-body {
    padding: 14px 16px;
    flex: 1;
  }

  .card-title {
    font-size: 15px;
    font-weight: 600;
    margin-bottom: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-meta {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .meta-row {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
  }

  .meta-label {
    color: var(--text-secondary);
  }

  .meta-value {
    color: var(--text-primary);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .card-actions {
    display: flex;
    border-top: 1px solid var(--border);
  }

  .action-btn {
    flex: 1;
    padding: 10px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    border-right: 1px solid var(--border);
  }

  .action-btn:last-child {
    border-right: none;
  }

  .action-btn:hover {
    background: var(--bg-tertiary);
    color: var(--accent);
  }

  .action-btn.danger:hover {
    background: rgba(239, 68, 68, 0.1);
    color: #f87171;
  }
</style>
