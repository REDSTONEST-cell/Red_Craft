<script lang="ts">
  // 底部任务栏：显示正在进行的下载/安装任务，总进度与速度实时更新。
  import { app } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import { bytes, speed } from "../lib/format";
  import { page } from "../lib/motion";
  import ProgressBar from "./ProgressBar.svelte";
  import Icon from "./Icon.svelte";

  // 启动页底栏已经显示启动进度，不再重复显示
  const visible = $derived(
    app.tasks.filter(
      (t) => (t.state === "running" || t.state === "failed") && !(app.page === "launch" && t.title.startsWith("启动")),
    ),
  );

  function ratio(t: (typeof app.tasks)[number]) {
    const p = t.progress;
    if (!p) return 0;
    if (p.totalBytes > 0) return p.doneBytes / p.totalBytes;
    return p.totalFiles ? p.doneFiles / p.totalFiles : 0;
  }
</script>

{#if visible.length}
  <div class="dock" transition:page={{ y: 16 }}>
    {#each visible as t (t.id)}
      <div class="task" class:failed={t.state === "failed"}>
        <div class="head">
          <strong>{t.title}</strong>
          <span class="stage">{t.state === "failed" ? "失败" : t.stage}</span>
          <span class="spacer"></span>
          {#if t.progress && t.state === "running"}
            <span class="mono nums">
              {t.progress.doneFiles}/{t.progress.totalFiles} · {bytes(t.progress.doneBytes)}{#if t.progress.totalBytes}
                / {bytes(t.progress.totalBytes)}{/if}
              · {speed(t.progress.speed)}
            </span>
          {/if}
          {#if t.state === "running"}
            <button class="btn ghost icon sm" title="取消" onclick={() => api.cancelTask(t.id)}><Icon name="x" size={12} /></button>
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
{/if}

<style>
  .dock {
    position: absolute;
    left: 24px;
    right: 24px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 30;
    pointer-events: none;
  }
  .task {
    pointer-events: auto;
    background: var(--card);
    border: 1.5px solid var(--ink);
    box-shadow: var(--shadow);
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .task.failed {
    border-left: 6px solid var(--danger);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
    min-height: 26px;
  }
  .head strong {
    font-size: 13px;
  }
  .stage {
    color: var(--muted);
  }
  .spacer {
    flex: 1;
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
