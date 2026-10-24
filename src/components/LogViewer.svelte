<script lang="ts">
  // 游戏日志窗口：虚拟滚动，实时追加，自动跟随底部。
  import { app, gameLogs, logVersion } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import Modal from "./Modal.svelte";
  import VirtualList from "./VirtualList.svelte";

  let filter = $state("");

  const instance = $derived(app.logViewer);
  const session = $derived(instance ? app.games[instance] : null);
  const lines = $derived.by(() => {
    void logVersion.value;
    const all = instance ? gameLogs(instance) : [];
    const f = filter.trim().toLowerCase();
    // 返回新数组，保证列表在追加后刷新
    return f ? all.filter((l) => l.text.toLowerCase().includes(f)) : all.slice();
  });

  function level(text: string) {
    if (/\b(ERROR|FATAL)\b|Exception|^\s+at /.test(text)) return "error";
    if (/\bWARN\b/.test(text)) return "warn";
    return "";
  }

  let open = $state(false);
  $effect(() => {
    open = !!app.logViewer;
  });

  async function copy() {
    const text = (instance ? gameLogs(instance) : []).map((l) => l.text).join("\n");
    await navigator.clipboard.writeText(text);
  }
</script>

<Modal bind:open title={`游戏日志 · ${instance ?? ""}`} kicker="LOG" width={860} onclose={() => (app.logViewer = null)}>
  <div class="bar">
    <input class="input" placeholder="过滤日志…" bind:value={filter} />
    <span class="muted">{lines.length} 行</span>
    <span class="spacer"></span>
    <button class="btn sm" onclick={copy}>复制全部</button>
    {#if session?.running}
      <button class="btn sm danger" onclick={() => instance && api.killGame(instance)}>结束游戏</button>
    {/if}
  </div>
  <div class="log mono">
    <VirtualList items={lines} rowHeight={18} follow>
      {#snippet row(line)}
        <div class="line {level(line.text)}" class:stderr={line.stderr}>{line.text}</div>
      {/snippet}
    </VirtualList>
  </div>
</Modal>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  .bar .input {
    width: 260px;
  }
  .spacer {
    flex: 1;
  }
  .log {
    height: 52vh;
    border: 1px solid var(--rule);
    background: var(--paper);
    font-size: 11.5px;
    padding: 4px 0;
    user-select: text;
  }
  .line {
    height: 18px;
    line-height: 18px;
    padding: 0 10px;
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ink-2);
  }
  .line.warn {
    color: #b7791f;
  }
  .line.error,
  .line.stderr.error {
    color: var(--danger);
  }
</style>
