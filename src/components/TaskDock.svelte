<script lang="ts">
  // 任务按钮：与 PCL 类似，进行中的下载 / 安装任务折叠在右下角的悬浮按钮里，
  // 按钮上的圆环显示总进度，点击展开任务列表。
  import { app } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import { bytes, speed } from "../lib/format";
  import { page } from "../lib/motion";
  import ProgressBar from "./ProgressBar.svelte";
  import Icon from "./Icon.svelte";

  let open = $state(false);

  // 启动页底栏已经显示启动进度，不再重复显示
  const visible = $derived(
    app.tasks.filter(
      (t) => (t.state === "running" || t.state === "failed") && !(app.page === "launch" && t.title.startsWith("启动")),
    ),
  );
  const running = $derived(visible.filter((t) => t.state === "running"));
  const failed = $derived(visible.length - running.length);

  function ratio(t: (typeof app.tasks)[number]) {
    const p = t.progress;
    if (!p) return 0;
    if (p.totalBytes > 0) return p.doneBytes / p.totalBytes;
    return p.totalFiles ? p.doneFiles / p.totalFiles : 0;
  }
  // 总进度：各任务进度的平均值
  const total = $derived(running.length ? running.reduce((s, t) => s + ratio(t), 0) / running.length : 1);
  const totalSpeed = $derived(running.reduce((s, t) => s + (t.progress?.speed ?? 0), 0));

  // 圆环
  const R = 22;
  const C = 2 * Math.PI * R;

  // 没有任务时自动收起
  $effect(() => {
    if (!visible.length) open = false;
  });

  function cancelAll() {
    for (const t of running) api.cancelTask(t.id).catch(() => {});
  }

  function onKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") open = false;
  }
</script>

<svelte:window onkeydown={onKey} />

{#if visible.length}
  {#if open}
    <div class="task-panel" transition:page={{ y: 12 }} role="dialog" aria-label="任务列表">
      <header>
        <strong>任务</strong>
        <span class="muted small">
          {running.length} 个进行中{#if failed} · <span class="bad">{failed} 个失败</span>{/if}
          {#if totalSpeed > 0} · {speed(totalSpeed)}{/if}
        </span>
        <span class="spacer"></span>
        {#if running.length > 1}
          <button class="btn ghost sm" onclick={cancelAll}>全部取消</button>
        {/if}
        <button class="btn ghost icon sm" title="收起" onclick={() => (open = false)}><Icon name="x" size={12} /></button>
      </header>
      <div class="list">
        {#each visible as t (t.id)}
          <div class="task" class:failed={t.state === "failed"}>
            <div class="head">
              <strong title={t.title}>{t.title}</strong>
              {#if t.state === "running"}
                <button class="btn ghost icon sm" title="取消" onclick={() => api.cancelTask(t.id)}><Icon name="x" size={12} /></button>
              {/if}
            </div>
            <div class="meta">
              <span class="stage">{t.state === "failed" ? "失败" : t.stage}</span>
              {#if t.progress && t.state === "running"}
                <span class="mono nums">
                  {t.progress.doneFiles}/{t.progress.totalFiles} · {bytes(t.progress.doneBytes)}{#if t.progress.totalBytes}
                    / {bytes(t.progress.totalBytes)}{/if}
                  · {speed(t.progress.speed)}
                </span>
              {/if}
            </div>
            {#if t.state === "running"}
              <ProgressBar size="sm" value={ratio(t)} indeterminate={!t.progress} />
            {:else if t.error}
              <p class="err">{t.error.message}</p>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <button
    class="task-fab"
    class:failed={failed > 0 && !running.length}
    transition:page={{ y: 16 }}
    title={open ? "收起任务列表" : `${running.length} 个任务进行中，点击查看`}
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <svg viewBox="0 0 52 52" class="ring" aria-hidden="true">
      <circle cx="26" cy="26" r={R} class="track" />
      <circle
        cx="26"
        cy="26"
        r={R}
        class="value"
        stroke-dasharray={C}
        stroke-dashoffset={C * (1 - Math.min(1, Math.max(0, total)))}
      />
    </svg>
    <span class="icon"><Icon name={failed && !running.length ? "warn" : "download"} size={18} /></span>
    {#if visible.length > 1}<span class="count">{visible.length}</span>{/if}
  </button>
{/if}

<style>
  .task-fab {
    position: absolute;
    right: 22px;
    bottom: 20px;
    z-index: 31;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    border: 1.5px solid var(--ink);
    background: var(--card);
    box-shadow: var(--shadow);
    display: grid;
    place-items: center;
    cursor: pointer;
    color: var(--ink);
    transition: transform var(--t-fast) var(--ease);
  }
  .task-fab:hover {
    transform: translateY(-2px);
  }
  .task-fab:active {
    transform: scale(0.96);
  }
  .task-fab.failed {
    border-color: var(--danger);
    color: var(--danger);
  }
  .ring {
    position: absolute;
    inset: 2px;
    width: calc(100% - 4px);
    height: calc(100% - 4px);
    transform: rotate(-90deg);
  }
  .ring circle {
    fill: none;
    stroke-width: 3.5;
  }
  .ring .track {
    stroke: var(--rule);
  }
  .ring .value {
    stroke: var(--accent);
    stroke-linecap: round;
    transition: stroke-dashoffset var(--t-mid) var(--ease-out);
  }
  .icon {
    position: relative;
    display: grid;
    place-items: center;
  }
  .count {
    position: absolute;
    top: -4px;
    right: -4px;
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    border-radius: 10px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 11px;
    font-weight: 700;
    display: grid;
    place-items: center;
  }
  .task-panel {
    position: absolute;
    right: 22px;
    bottom: 88px;
    z-index: 31;
    width: min(440px, calc(100% - 44px));
    max-height: min(60vh, 520px);
    display: flex;
    flex-direction: column;
    background: var(--card);
    border: 1.5px solid var(--ink);
    box-shadow: var(--shadow);
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--rule);
  }
  header strong {
    font-family: var(--serif);
    font-size: 15px;
  }
  .small {
    font-size: 12px;
  }
  .bad {
    color: var(--danger);
  }
  .spacer {
    flex: 1;
  }
  .list {
    overflow-y: auto;
    padding: 4px 12px 10px;
  }
  .task {
    padding: 10px 0;
    border-bottom: 1px solid var(--rule);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .task:last-child {
    border-bottom: 0;
  }
  .task.failed {
    border-left: 4px solid var(--danger);
    padding-left: 8px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
  }
  .head strong {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    display: flex;
    gap: 10px;
    font-size: 12px;
    flex-wrap: wrap;
  }
  .stage {
    color: var(--muted);
  }
  .nums {
    font-size: 11px;
    color: var(--ink-2);
  }
  .err {
    font-size: 12px;
    color: var(--danger);
  }
</style>
