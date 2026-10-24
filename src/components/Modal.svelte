<script lang="ts">
  import type { Snippet } from "svelte";
  import { fade, pop } from "../lib/motion";
  import Icon from "./Icon.svelte";

  let {
    open = $bindable(false),
    title,
    kicker,
    width = 480,
    children,
    actions,
    onclose,
  }: {
    open: boolean;
    title: string;
    kicker?: string;
    width?: number;
    children: Snippet;
    actions?: Snippet;
    onclose?: () => void;
  } = $props();

  function close() {
    open = false;
    onclose?.();
  }

  function onKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") close();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div class="backdrop" transition:fade onclick={close} role="presentation"></div>
  <div class="center">
    <div class="modal" style:width="{width}px" transition:pop role="dialog" aria-modal="true" aria-label={title}>
      <header>
        <div>
          {#if kicker}<div class="kicker">{kicker}</div>{/if}
          <h2>{title}</h2>
        </div>
        <button class="btn ghost icon sm" onclick={close} title="关闭"><Icon name="x" size={14} /></button>
      </header>
      <div class="body">
        {@render children()}
      </div>
      {#if actions}
        <footer>{@render actions()}</footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: var(--titlebar-h) 0 0 0;
    background: color-mix(in srgb, var(--paper) 70%, transparent);
    z-index: 50;
  }
  .center {
    position: fixed;
    inset: var(--titlebar-h) 0 0 0;
    display: grid;
    place-items: center;
    z-index: 51;
    pointer-events: none;
  }
  .modal {
    pointer-events: auto;
    max-width: calc(100vw - 48px);
    max-height: calc(100vh - var(--titlebar-h) - 48px);
    display: flex;
    flex-direction: column;
    background: var(--card);
    border: 1.5px solid var(--ink);
    box-shadow: 6px 6px 0 var(--ink);
  }
  :global(:root[data-theme="dark"]) .modal {
    box-shadow: 6px 6px 0 #000;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 18px 20px 12px;
    border-bottom: 2px solid var(--rule-strong);
    margin: 0 20px;
    padding-left: 0;
    padding-right: 0;
  }
  h2 {
    font-size: 22px;
    line-height: 1.2;
  }
  .body {
    padding: 16px 20px;
    overflow: auto;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 12px 20px 18px;
    border-top: 1px solid var(--rule);
    margin: 0 20px;
    padding-left: 0;
    padding-right: 0;
  }
</style>
