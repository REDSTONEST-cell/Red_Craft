<script lang="ts">
  import Icon from "./Icon.svelte";
  import { app, type Page } from "../lib/state.svelte";

  const items: { page: Page; label: string; icon: string }[] = [
    { page: "launch", label: "启动", icon: "play" },
    { page: "instances", label: "版本", icon: "box" },
    // 「下载」页面包含游戏与资源（Mod、材质包、光影包、整合包）
    { page: "download", label: "下载", icon: "download" },
    { page: "accounts", label: "账号", icon: "user" },
  ];

  // 资源页现在是「下载」的一部分
  const currentPage = $derived<Page>(app.page === "mods" ? "download" : app.page);
  const activeIndex = $derived(items.findIndex((i) => i.page === currentPage));
</script>

<nav class="sidebar">
  <span class="marker" class:at-settings={currentPage === "settings"} style:--active-index={activeIndex} aria-hidden="true"></span>
  <div class="items">
    {#each items as item}
      <button
        class="item"
        class:active={currentPage === item.page}
        onclick={() => (app.page = item.page)}
        aria-current={currentPage === item.page ? "page" : undefined}
      >
        <Icon name={item.icon} size={20} />
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
  <button
    class="item settings"
    class:active={app.page === "settings"}
    aria-current={currentPage === "settings" ? "page" : undefined}
    onclick={() => (app.page = "settings")}
  >
    <Icon name="gear" size={20} />
    <span>设置</span>
  </button>
</nav>

<style>
  .sidebar {
    --item-height: 64px;
    --nav-padding: 10px;
    --settings-height: var(--item-height);
    --bottom-padding: var(--nav-padding);
    position: relative;
    width: var(--sidebar-w);
    flex: none;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    border-right: 1px solid var(--rule);
    padding: var(--nav-padding) 0 var(--bottom-padding);
    background: var(--paper);
  }
  .items {
    display: flex;
    flex-direction: column;
  }
  .marker {
    position: absolute;
    left: 0;
    top: calc(var(--nav-padding) + (var(--active-index) + 0.5) * var(--item-height));
    width: 4px;
    height: var(--item-height);
    background: var(--accent);
    transform: translateY(-50%);
    transition: top var(--t-mid) var(--ease-out);
    pointer-events: none;
  }
  .marker.at-settings {
    top: calc(100% - var(--bottom-padding) - var(--settings-height) / 2);
  }
  .settings {
    flex: 0 0 var(--settings-height);
  }
  .item {
    height: var(--item-height);
    border: 0;
    background: transparent;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    cursor: pointer;
    transition: color var(--t-fast);
  }
  .item :global(svg) {
    transition: transform var(--t-fast) var(--ease);
  }
  .item:hover {
    color: var(--ink);
  }
  .item:hover :global(svg) {
    transform: translateY(-1px);
  }
  .item:active :global(svg) {
    transform: scale(0.9);
  }
  .item.active {
    color: var(--ink);
  }
  .item.active :global(svg) {
    color: var(--accent);
  }
</style>
