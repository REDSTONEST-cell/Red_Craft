<script lang="ts">
  import TitleBar from "./components/TitleBar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import TaskDock from "./components/TaskDock.svelte";
  import LogViewer from "./components/LogViewer.svelte";
  import CrashDialog from "./components/CrashDialog.svelte";
  import Launch from "./pages/Launch.svelte";
  import Instances from "./pages/Instances.svelte";
  import Download from "./pages/Download.svelte";
  import Mods from "./pages/Mods.svelte";
  import Accounts from "./pages/Accounts.svelte";
  import Settings from "./pages/Settings.svelte";
  import { app, init, showError } from "./lib/state.svelte";
  import { api } from "./lib/api";
  import { page } from "./lib/motion";
  import { tick } from "svelte";

  let ready = $state(false);
  // 有任务面板时给页面底部留出空间，避免遮住内容
  const dockPad = $derived.by(() => {
    const n = app.tasks.filter((t) => t.state === "running" || t.state === "failed").length;
    return n && app.page !== "launch" ? `${28 + n * 74}px` : undefined;
  });
  let systemDark = $state(window.matchMedia("(prefers-color-scheme: dark)").matches);

  $effect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = (e: MediaQueryListEvent) => (systemDark = e.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  });

  // 主题与主题色
  $effect(() => {
    const s = app.settings;
    const theme = s?.theme ?? "system";
    const dark = theme === "dark" || (theme === "system" && systemDark);
    const root = document.documentElement;
    root.dataset.theme = dark ? "dark" : "light";
    const accent = s?.accent || "#E8590C";
    root.style.setProperty("--accent", accent);
    root.style.setProperty("--accent-ink", readableOn(accent));
  });

  function readableOn(hex: string) {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex);
    if (!m) return "#fffaf3";
    const n = parseInt(m[1], 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => {
      const v = c / 255;
      return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    });
    const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    return lum > 0.4 ? "#1b1209" : "#fffaf3";
  }

  $effect(() => {
    init()
      .catch((e) => showError(e, "初始化失败"))
      .finally(async () => {
        ready = true;
        // 内容渲染完成后再显示窗口，避免白屏闪烁。
        // 注意：窗口隐藏时 requestAnimationFrame 不会触发，这里用 tick + setTimeout
        await tick();
        setTimeout(() => api.appReady().catch(() => {}), 16);
      });
  });

  // 禁用浏览器默认右键菜单与部分快捷键，让它更像原生应用
  function onContext(e: MouseEvent) {
    const t = e.target as HTMLElement;
    if (!t.closest("input, textarea, .selectable")) e.preventDefault();
  }
</script>

<svelte:window oncontextmenu={onContext} />

<div class="shell">
  <TitleBar />
  <div class="body">
    <Sidebar />
    <main class="content">
      {#if ready}
        {#key app.page}
          <div class="page" in:page style:padding-bottom={dockPad}>
            {#if app.page === "launch"}
              <Launch />
            {:else if app.page === "instances"}
              <Instances />
            {:else if app.page === "download"}
              <Download />
            {:else if app.page === "mods"}
              <Mods />
            {:else if app.page === "accounts"}
              <Accounts />
            {:else if app.page === "settings"}
              <Settings />
            {/if}
          </div>
        {/key}
      {/if}
      <TaskDock />
    </main>
  </div>
</div>

<Toasts />
<LogViewer />
<CrashDialog />

<style>
  .shell {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .content {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
  .page {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    padding: 22px 28px 28px;
  }
</style>
