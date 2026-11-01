<script lang="ts">
  // 手机版导航栏：左侧窄栏，图标 + 文字，选中项为胶囊高亮（类似 Material 的 Navigation Rail）
  import Icon from "../components/Icon.svelte";
  import { app, type Page } from "../lib/state.svelte";

  const items: { page: Page; label: string; icon: string }[] = [
    { page: "launch", label: "主页", icon: "play" },
    { page: "instances", label: "版本", icon: "box" },
    { page: "download", label: "下载", icon: "download" },
    { page: "mods", label: "资源", icon: "cube" },
    { page: "accounts", label: "账号", icon: "user" },
  ];
  const running = $derived(app.tasks.filter((t) => t.state === "running").length);
</script>

<nav class="rail">
  <div class="brand">RCL</div>
  <div class="items">
    {#each items as item}
      <button class="item" class:active={app.page === item.page} onclick={() => (app.page = item.page)}>
        <span class="pill"><Icon name={item.icon} size={22} /></span>
        <span class="label">{item.label}</span>
        {#if item.page === "download" && running}<span class="dot">{running}</span>{/if}
      </button>
    {/each}
  </div>
  <button class="item" class:active={app.page === "settings"} onclick={() => (app.page = "settings")}>
    <span class="pill"><Icon name="gear" size={22} /></span>
    <span class="label">设置</span>
  </button>
</nav>

<style>
  /* 挖孔在左侧时，导航栏的背景延伸到屏幕边缘，按钮整体右移避开挖孔；上下避开屏幕圆角 */
  .rail {
    width: calc(64px + var(--m-safe-l));
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    overflow-y: auto;
    overflow-x: hidden;
    min-height: 0;
    overscroll-behavior-y: contain;
    touch-action: pan-y;
    padding: max(8px, var(--m-safe-t), var(--m-corner)) 0 max(8px, var(--m-safe-b), var(--m-corner)) var(--m-safe-l);
    background: var(--card);
    border-right: 1px solid var(--rule);
  }
  .brand {
    flex-shrink: 0;
    font-weight: 800;
    font-size: 13px;
    letter-spacing: 0.04em;
    color: var(--accent);
    margin: 2px 0 6px;
  }
  .items {
    flex: 1 0 auto;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
  }
  .item {
    position: relative;
    flex-shrink: 0;
    width: 56px;
    border: 0;
    background: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 4px 0;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    -webkit-tap-highlight-color: transparent;
  }
  .pill {
    width: 44px;
    height: 30px;
    border-radius: 15px;
    display: grid;
    place-items: center;
    transition: background var(--t-fast), color var(--t-fast);
  }
  .item.active {
    color: var(--ink);
  }
  .item.active .pill {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
  }
  .item:active .pill {
    background: color-mix(in srgb, var(--ink) 10%, transparent);
  }
  .dot {
    position: absolute;
    top: 2px;
    right: 6px;
    min-width: 15px;
    height: 15px;
    padding: 0 4px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 9px;
    line-height: 15px;
  }
</style>
