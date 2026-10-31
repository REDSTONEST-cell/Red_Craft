<script lang="ts">
  import TitleBar from "./components/TitleBar.svelte";
  import BackgroundLayer from "./components/BackgroundLayer.svelte";
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
  import ModpackInstall from "./components/ModpackInstall.svelte";
  import UpdateDialog from "./components/UpdateDialog.svelte";
  import { checkUpdate } from "./lib/update.svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import type { ModpackInfo, PackSource } from "./lib/api";
  import { app, init, showError } from "./lib/state.svelte";
  import { api } from "./lib/api";
  import { page } from "./lib/motion";
  import { tick } from "svelte";

  let ready = $state(false);
  // 有任务面板时给页面底部留出空间，避免遮住内容
  const dockPad = $derived.by(() => {
    const n = app.tasks.filter((t) => t.state === "running" || t.state === "failed").length;
    // 任务折叠在右下角的悬浮按钮里，只需给按钮留出位置
    return n && app.page !== "launch" ? "88px" : undefined;
  });
  // 把整合包文件拖进窗口即可导入
  let dragging = $state(false);
  let dropOpen = $state(false);
  let dropSource = $state<PackSource | null>(null);
  let dropInfo = $state<ModpackInfo | null>(null);
  const isPack = (p: string) => /\.(mrpack|zip)$/i.test(p);

  $effect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    getCurrentWebview()
      .onDragDropEvent(async (e) => {
        const p = e.payload;
        if (p.type === "enter") dragging = !!app.dropTarget || p.paths.some(isPack);
        else if (p.type === "leave") dragging = false;
        else if (p.type === "drop") {
          const was = dragging;
          dragging = false;
          // 资源管理页面：导入到当前类别
          if (app.dropTarget) {
            if (p.paths.length) app.dropTarget.drop(p.paths);
            return;
          }
          const file = p.paths.find(isPack);
          if (!was || !file) return;
          try {
            dropInfo = await api.readModpackFile(file);
            dropSource = { type: "file", path: file };
            dropOpen = true;
          } catch (err) {
            showError(err, "无法读取整合包");
          }
        }
      })
      .then((u) => (alive ? (unlisten = u) : u()))
      .catch(() => {});
    return () => {
      alive = false;
      unlisten?.();
    };
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
    root.dataset.corner = s?.corner ?? "rounded";
    root.dataset.layout = s?.layout ?? "minimal";
    // 毛玻璃不透明度：面板使用设定值，顶栏、侧栏等稍实一些
    const glass = Math.min(0.95, Math.max(0.2, s?.glassOpacity ?? 0.58));
    root.style.setProperty("--glass-pct", `${Math.round(glass * 100)}%`);
    root.style.setProperty("--glass-strong-pct", `${Math.round(Math.min(0.97, glass + 0.18) * 100)}%`);
    // 设置了自定义背景时，内容面板至少保留一定不透明度，保证文字可读
    root.style.setProperty("--glass-content-pct", `${Math.round(Math.max(glass, 0.5) * 100)}%`);
    const blur = Math.min(40, Math.max(0, s?.glassBlur ?? 24));
    root.style.setProperty("--glass-blur-px", `${blur}px`);
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
        // 启动后稍等再检查更新，不影响启动速度
        if (app.settings?.autoCheckUpdate) setTimeout(() => checkUpdate(false), 3000);
      });
  });

  // 禁用浏览器默认右键菜单与部分快捷键，让它更像原生应用
  function onContext(e: MouseEvent) {
    const t = e.target as HTMLElement;
    if (!t.closest("input, textarea, .selectable")) e.preventDefault();
  }
</script>

<svelte:window oncontextmenu={onContext} />

<BackgroundLayer />
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

{#if dragging}
  <div class="drop-hint" transition:page={{ y: 0, duration: 160 }}>
    <div class="drop-card">
      {#if app.dropTarget}
        <strong>松开以导入{app.dropTarget.label}</strong>
        <span>文件会复制到当前实例中</span>
      {:else}
        <strong>松开以导入整合包</strong>
        <span>支持 Modrinth（.mrpack）与 CurseForge（.zip）整合包</span>
      {/if}
    </div>
  </div>
{/if}
<UpdateDialog />
<ModpackInstall bind:open={dropOpen} source={dropSource} title={dropInfo?.name ?? ""} info={dropInfo} />

<Toasts />
<LogViewer />
<CrashDialog />

<style>
  .drop-hint {
    position: fixed;
    inset: var(--titlebar-h) 0 0 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--accent) 16%, rgba(0, 0, 0, 0.35));
    pointer-events: none;
  }
  .drop-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 34px 48px;
    background: var(--card);
    color: var(--ink);
    border: 2px dashed var(--accent);
    border-radius: var(--radius-lg, 0);
    box-shadow: var(--shadow);
  }
  .drop-card strong {
    font-size: 20px;
  }
  .drop-card span {
    font-size: 12px;
    color: var(--muted);
  }
  .shell {
    position: relative;
    z-index: 2;
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
