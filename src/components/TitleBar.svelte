<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { app } from "../lib/state.svelte";

  const win = getCurrentWindow();
  let maximized = $state(false);

  $effect(() => {
    win.isMaximized().then((m) => (maximized = m));
    const un = win.onResized(async () => {
      maximized = await win.isMaximized();
    });
    return () => {
      un.then((f) => f());
    };
  });

  const running = $derived(app.tasks.filter((t) => t.state === "running").length);
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <span class="pixel"></span>
    <span class="name" data-tauri-drag-region>Red Craft</span>
    {#if app.info?.portable}<span class="tag">便携</span>{/if}
    {#if running > 0}
      <span class="tag accent">{running} 个任务进行中</span>
    {/if}
  </div>
  <div class="controls">
    <button class="ctl" title="最小化" aria-label="最小化" onclick={() => win.minimize()}>
      <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5.5h10" stroke="currentColor" /></svg>
    </button>
    <button class="ctl" title={maximized ? "还原" : "最大化"} aria-label={maximized ? "还原" : "最大化"} onclick={() => win.toggleMaximize()}>
      {#if maximized}
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor">
          <path d="M2.5 2.5V.5h7v7h-2" /><rect x=".5" y="2.5" width="7" height="7" />
        </svg>
      {:else}
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor"><rect x=".5" y=".5" width="9" height="9" /></svg>
      {/if}
    </button>
    <button class="ctl close" title="关闭" aria-label="关闭" onclick={() => win.close()}>
      <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor"><path d="M0 0l10 10M10 0L0 10" /></svg>
    </button>
  </div>
</header>

<style>
  .titlebar {
    height: var(--titlebar-h);
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--rule);
    background: var(--paper);
    flex: none;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-left: 14px;
    height: 100%;
    flex: 1;
  }
  .name {
    font-family: var(--serif);
    font-weight: 700;
    font-size: 14px;
    letter-spacing: 0.04em;
  }
  .controls {
    display: flex;
    height: 100%;
  }
  .ctl {
    width: 46px;
    height: 100%;
    border: 0;
    background: transparent;
    color: var(--ink-2);
    display: grid;
    place-items: center;
    cursor: pointer;
    transition: background-color var(--t-fast);
  }
  .ctl:hover {
    background: var(--paper-2);
    color: var(--ink);
  }
  .ctl.close:hover {
    background: var(--danger);
    color: #fff;
  }
</style>
