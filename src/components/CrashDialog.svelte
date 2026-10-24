<script lang="ts">
  import { app, showError } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import Modal from "./Modal.svelte";

  let open = $state(false);
  let report = $state<string | null>(null);

  $effect(() => {
    open = !!app.crash;
    report = null;
  });

  const crash = $derived(app.crash);

  async function viewReport() {
    if (!crash?.exit.crashReport) return;
    try {
      report = await api.readGameFile(crash.exit.crashReport);
    } catch (e) {
      showError(e, "读取崩溃报告失败");
    }
  }
</script>

<Modal bind:open title="游戏意外退出" kicker="CRASH" width={620} onclose={() => (app.crash = null)}>
  {#if crash}
    <p class="lede">
      实例 <strong>{crash.instance}</strong> 已退出{#if crash.exit.code !== null}（退出码 <span class="mono">{crash.exit.code}</span>）{/if}。
    </p>
    {#if crash.exit.causes.length}
      <div class="causes">
        {#each crash.exit.causes as c}
          <div class="cause">
            <span class="pixel"></span>
            <div>
              <strong>{c.reason}</strong>
              <p>{c.solution}</p>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <p class="muted">没有识别出具体原因。请查看崩溃报告或游戏日志，日志最后几行通常包含报错信息。</p>
    {/if}

    {#if report !== null}
      <pre class="report mono">{report}</pre>
    {:else if crash.exit.tail.length}
      <pre class="report mono">{crash.exit.tail.slice(-30).join("\n")}</pre>
    {/if}
  {/if}
  {#snippet actions()}
    {#if crash?.exit.crashReport}
      <button class="btn" onclick={viewReport}>查看崩溃报告</button>
      <button class="btn" onclick={() => crash?.exit.crashReport && api.openFile(crash.exit.crashReport)}>用系统打开</button>
    {/if}
    <button
      class="btn"
      onclick={() => {
        app.logViewer = crash?.instance ?? null;
        app.crash = null;
      }}>查看日志</button
    >
    <button class="btn primary" onclick={() => (app.crash = null)}>知道了</button>
  {/snippet}
</Modal>

<style>
  .lede {
    margin-bottom: 14px;
  }
  .causes {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 14px;
  }
  .cause {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 10px 12px;
    border-left: 3px solid var(--accent);
    background: var(--accent-soft);
  }
  .cause .pixel {
    margin-top: 6px;
  }
  .cause p {
    font-size: 13px;
    color: var(--ink-2);
    margin-top: 2px;
  }
  .report {
    max-height: 240px;
    overflow: auto;
    font-size: 11px;
    background: var(--paper);
    border: 1px solid var(--rule);
    padding: 10px;
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }
</style>
