<script>
  import Router from "svelte-router-spa";
  import { routes } from "./router.js";
  import Sidebar from "./components/Sidebar.svelte";
</script>

<div class="app-container">
  <Sidebar />
  <main class="main-content">
    <Router {routes} />
  </main>
</div>

<style>
  .app-container {
    display: flex;
    width: 100%;
    height: 100%;
  }

  .main-content {
    flex: 1;
    overflow: hidden;
    position: relative;
  }
</style>
