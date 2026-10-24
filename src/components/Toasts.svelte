<script lang="ts">
  import { flip } from "svelte/animate";
  import { app, dismissToast } from "../lib/state.svelte";
  import { slide, dur } from "../lib/motion";
  import Icon from "./Icon.svelte";

  const icons = { info: "info", ok: "check", error: "warn" } as const;
</script>

<div class="toasts" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast {t.kind}" transition:slide animate:flip={{ duration: dur(200) }}>
      <span class="ico"><Icon name={icons[t.kind]} size={16} /></span>
      <div class="text">
        <strong>{t.title}</strong>
        {#if t.body}<p>{t.body}</p>{/if}
      </div>
      <button class="btn ghost icon sm" onclick={() => dismissToast(t.id)} title="关闭"><Icon name="x" size={12} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 18px;
    bottom: 18px;
    width: 340px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    z-index: 80;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 12px 10px 12px 14px;
    background: var(--card);
    border: 1.5px solid var(--ink);
    box-shadow: var(--shadow);
  }
  .ico {
    margin-top: 2px;
    color: var(--accent);
  }
  .toast.ok .ico {
    color: var(--ok);
  }
  .toast.error {
    border-left: 6px solid var(--danger);
  }
  .toast.error .ico {
    color: var(--danger);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  strong {
    display: block;
    font-size: 13px;
  }
  p {
    margin-top: 3px;
    font-size: 12px;
    color: var(--ink-2);
    white-space: pre-line;
    user-select: text;
    word-break: break-word;
  }
</style>
