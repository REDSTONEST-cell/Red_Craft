<script lang="ts">
  // 安装整合包：确认实例名称后创建新实例。
  import { api, type ModpackInfo, type PackSource } from "../lib/api";
  import { app, refreshInstances, selectInstance, showError, waitTask } from "../lib/state.svelte";
  import { LOADER_LABEL } from "../lib/format";
  import Modal from "./Modal.svelte";

  let {
    open = $bindable(false),
    source,
    title,
    info = null,
  }: {
    open: boolean;
    source: PackSource | null;
    title: string;
    /** 本地整合包可以预先读取信息 */
    info?: ModpackInfo | null;
  } = $props();

  let name = $state("");
  let busy = $state(false);

  $effect(() => {
    if (open) name = info?.name ?? title;
  });

  const exists = $derived(app.instances.some((i) => i.id === name.trim()));

  async function install() {
    if (!source) return;
    busy = true;
    try {
      const id = await api.installModpack(source, name.trim() || null);
      open = false;
      const t = await waitTask(id);
      if (t.state === "done") {
        await refreshInstances();
        const newest = app.instances.find((i) => i.id === name.trim()) ?? null;
        if (newest) await selectInstance(newest.id);
      }
    } catch (e) {
      showError(e, "无法安装整合包");
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={`安装整合包 · ${info?.name ?? title}`} kicker="MODPACK" width={500}>
  {#if info}
    <dl class="meta">
      <dt>版本</dt><dd>{info.version || "—"}</dd>
      <dt>游戏版本</dt><dd>Minecraft {info.mcVersion}</dd>
      <dt>加载器</dt><dd>{info.loader ? `${LOADER_LABEL[info.loader.kind]} ${info.loader.version}` : "原版"}</dd>
      <dt>文件</dt><dd>{info.fileCount} 个</dd>
      <dt>来源</dt><dd>{info.format === "modrinth" ? "Modrinth（.mrpack）" : "CurseForge"}</dd>
    </dl>
  {/if}
  <div class="field">
    <label for="pack-name">实例名称</label>
    <input id="pack-name" class="input" bind:value={name} maxlength="64" />
    <span class="help">
      {#if exists}已有同名实例，安装时会自动在名称后加编号。{:else}整合包会安装为独立的新实例（自动开启版本隔离）。{/if}
    </span>
  </div>
  {#snippet actions()}
    <button class="btn" onclick={() => (open = false)}>取消</button>
    <button class="btn primary" disabled={busy || !source} onclick={install}>开始安装</button>
  {/snippet}
</Modal>

<style>
  .meta {
    display: grid;
    grid-template-columns: 80px 1fr;
    gap: 6px 12px;
    margin: 0 0 16px;
    font-size: 13px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
  }
</style>
