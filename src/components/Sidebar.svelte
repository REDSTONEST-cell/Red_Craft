<script lang="ts">
  import Icon from "./Icon.svelte";
  import { app, type Page } from "../lib/state.svelte";

  const items: { page: Page; label: string; icon: string }[] = [
    { page: "launch", label: "启动", icon: "play" },
    { page: "instances", label: "实例", icon: "box" },
    { page: "download", label: "下载", icon: "download" },
    { page: "mods", label: "Mod", icon: "cube" },
    { page: "accounts", label: "账号", icon: "user" },
  ];

  const activeIndex = $derived(
    app.page === "settings" ? -1 : items.findIndex((i) => i.page === app.page),
  );
</script>

<nav class="sidebar">
  <div class="items">
    {#if activeIndex >= 0}
      <span class="marker" style:transform="translateY({activeIndex * 64}px)"></span>
    {/if}
    {#each items as item}
      <button
        class="item"
        class:active={app.page === item.page}
        onclick={() => (app.page = item.page)}
        aria-current={app.page === item.page ? "page" : undefined}
      >
        <Icon name={item.icon} size={20} />
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
  <button
    class="item settings"
    class:active={app.page === "settings"}
    onclick={() => (app.page = "settings")}
  >
    <Icon name="gear" size={20} />
    <span>设置</span>
  </button>
</nav>

<style>
  .sidebar {
    width: var(--sidebar-w);
    flex: none;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    border-right: 1px solid var(--rule);
    padding: 10px 0;
    background: var(--paper);
  }
  .items {
    position: relative;
    display: flex;
    flex-direction: column;
  }
  .marker {
    position: absolute;
    left: 0;
    top: 0;
    width: 4px;
    height: 64px;
    background: var(--accent);
    transition: transform var(--t-mid) var(--ease-out);
    will-change: transform;
  }
  .item {
    height: 64px;
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
