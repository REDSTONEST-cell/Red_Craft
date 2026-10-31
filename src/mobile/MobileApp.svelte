<script lang="ts">
  // 手机版外壳：左侧导航栏 + 页面，横屏全屏。主页、版本、设置是手机专用页面，下载、资源、账号复用电脑版的页面并按手机收紧样式。
  import "./mobile.css";
  import Rail from "./Rail.svelte";
  import Home from "./pages/Home.svelte";
  import Versions from "./pages/Versions.svelte";
  import MobileSettings from "./pages/Settings.svelte";
  import Download from "../pages/Download.svelte";
  import Mods from "../pages/Mods.svelte";
  import Accounts from "../pages/Accounts.svelte";
  import Toasts from "../components/Toasts.svelte";
  import TaskDock from "../components/TaskDock.svelte";
  import LogViewer from "../components/LogViewer.svelte";
  import CrashDialog from "../components/CrashDialog.svelte";
  import { app, init, showError } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import { page } from "../lib/motion";
  import { tick } from "svelte";

  let ready = $state(false);
  let systemDark = $state(window.matchMedia("(prefers-color-scheme: dark)").matches);

  $effect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = (e: MediaQueryListEvent) => (systemDark = e.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  });

  // 主题与主题色；手机版固定使用圆角与极简布局
  $effect(() => {
    const s = app.settings;
    const theme = s?.theme ?? "light";
    const dark = theme === "dark" || (theme === "system" && systemDark);
    const root = document.documentElement;
    root.classList.add("mobile");
    root.dataset.theme = dark ? "dark" : "light";
    root.dataset.corner = "rounded";
    root.dataset.layout = "minimal";
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
    return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.4 ? "#1b1209" : "#fffaf3";
  }

  $effect(() => {
    init()
      .catch((e) => showError(e, "初始化失败"))
      .finally(async () => {
        ready = true;
        await tick();
        setTimeout(() => api.appReady().catch(() => {}), 16);
      });
  });
</script>

<div class="m-shell">
  <Rail />
  <main class="m-main">
    {#if ready}
      {#key app.page}
        <div class="m-page" class:fill={app.page === "launch"} in:page={{ y: 6, duration: 200 }}>
          {#if app.page === "launch"}
            <Home />
          {:else if app.page === "instances"}
            <Versions />
          {:else if app.page === "download"}
            <Download />
          {:else if app.page === "mods"}
            <Mods />
          {:else if app.page === "accounts"}
            <Accounts />
          {:else if app.page === "settings"}
            <MobileSettings />
          {/if}
        </div>
      {/key}
    {/if}
    <TaskDock />
  </main>
</div>

<Toasts />
<LogViewer />
<CrashDialog />

<style>
  .m-shell {
    height: 100%;
    display: flex;
    background: var(--paper);
    color: var(--ink);
    /* 刘海、圆角屏幕的安全区域 */
    padding: env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left);
  }
  .m-main {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
  .m-page {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    padding: 12px 14px 16px;
  }
  /* 主页自己安排边距，不滚动 */
  .m-page.fill {
    overflow: hidden;
    padding: 0;
  }
</style>
