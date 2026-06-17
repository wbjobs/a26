<script>
  import { navigate, currentPath } from "svelte-router-spa";

  const menuItems = [
    { name: "录制", path: "/", icon: "🎬" },
    { name: "录制库", path: "/library", icon: "📁" },
  ];

  $: activePath = $currentPath;

  function isActive(path) {
    if (path === "/") return activePath === "/" || activePath === "";
    return activePath.startsWith(path);
  }
</script>

<aside class="sidebar">
  <div class="logo">
    <span class="logo-icon">🎯</span>
    <span class="logo-text">PixelRecorder</span>
  </div>

  <nav class="menu">
    {#each menuItems as item}
      <button
        class="menu-item {isActive(item.path) ? 'active' : ''}"
        on:click={() => navigate(item.path)}
      >
        <span class="icon">{item.icon}</span>
        <span class="label">{item.name}</span>
      </button>
    {/each}
  </nav>

  <div class="footer-info">
    <span class="version">v0.1.0</span>
    <span class="tag">像素级增量录制</span>
  </div>
</aside>

<style>
  .sidebar {
    width: 220px;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
  }

  .logo {
    padding: 24px 20px;
    display: flex;
    align-items: center;
    gap: 12px;
    border-bottom: 1px solid var(--border);
  }

  .logo-icon {
    font-size: 28px;
  }

  .logo-text {
    font-size: 18px;
    font-weight: 700;
    background: linear-gradient(135deg, var(--accent), #ff8fa3);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
  }

  .menu {
    flex: 1;
    padding: 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: transparent;
    color: var(--text-secondary);
    border-radius: 8px;
    font-size: 14px;
    text-align: left;
  }

  .menu-item:hover {
    background: rgba(233, 69, 96, 0.1);
    color: var(--text-primary);
  }

  .menu-item.active {
    background: linear-gradient(135deg, var(--accent), #ff6b81);
    color: white;
    box-shadow: 0 4px 12px rgba(233, 69, 96, 0.3);
  }

  .icon {
    font-size: 18px;
  }

  .footer-info {
    padding: 16px 20px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .version {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .tag {
    font-size: 11px;
    color: var(--accent);
    opacity: 0.8;
  }
</style>
