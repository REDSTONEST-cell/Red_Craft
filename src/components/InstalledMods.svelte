<script lang="ts">
  // 已安装 Mod：启用 / 禁用、删除、检查更新。
  import { api, type LocalMod, type ModUpdate } from "../lib/api";
  import { showError, toast, waitTask } from "../lib/state.svelte";
  import { bytes, LOADER_LABEL } from "../lib/format";
  import VirtualList from "./VirtualList.svelte";
  import Toggle from "./Toggle.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let { instance }: { instance: string } = $props();

  let mods = $state<LocalMod[] | null>(null);
  let filter = $state("");
  let updates = $state<Record<string, ModUpdate>>({});
  let checking = $state(false);
  let updating = $state(false);
  let confirmDelete = $state<LocalMod | null>(null);
  let deleteOpen = $state(false);

  async function load() {
    try {
      mods = await api.modsList(instance);
    } catch (e) {
      showError(e, "读取 Mod 列表失败");
      mods = [];
    }
  }

  $effect(() => {
    void instance;
    mods = null;
    updates = {};
    load();
  });

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = mods ?? [];
    return q
      ? list.filter((m) => m.name.toLowerCase().includes(q) || m.modId.toLowerCase().includes(q) || m.filename.toLowerCase().includes(q))
      : list;
  });
  const enabledCount = $derived((mods ?? []).filter((m) => m.enabled).length);
  const updateList = $derived(Object.values(updates));

  async function toggle(m: LocalMod, enabled: boolean) {
    try {
      const name = await api.modSetEnabled(instance, m.filename, enabled);
      const old = m.filename;
      m.filename = name;
      m.enabled = enabled;
      if (updates[old]) {
        updates[name] = { ...updates[old], filename: name };
        delete updates[old];
      }
    } catch (e) {
      m.enabled = !enabled;
      showError(e, "操作失败");
    }
  }

  async function remove() {
    if (!confirmDelete) return;
    try {
      await api.modDelete(instance, [confirmDelete.filename]);
      toast("ok", `已删除 ${confirmDelete.name}`);
      deleteOpen = false;
      await load();
    } catch (e) {
      showError(e, "删除失败");
    }
  }

  async function check() {
    checking = true;
    try {
      const list = await api.modsCheckUpdates(instance);
      updates = Object.fromEntries(list.map((u) => [u.filename, u]));
      toast(list.length ? "info" : "ok", list.length ? `${list.length} 个 Mod 有可用更新` : "所有 Mod 都是最新版本");
    } catch (e) {
      showError(e, "检查更新失败");
    } finally {
      checking = false;
    }
  }

  async function update(list: ModUpdate[]) {
    if (!list.length) return;
    updating = true;
    try {
      const id = await api.modsUpdate(instance, list);
      const t = await waitTask(id);
      if (t.state === "done") {
        for (const u of list) delete updates[u.filename];
        await load();
      }
    } catch (e) {
      showError(e, "更新失败");
    } finally {
      updating = false;
    }
  }
</script>

<div class="wrap">
  <div class="bar">
    <div class="search">
      <Icon name="search" size={14} />
      <input class="input" placeholder="筛选已安装的 Mod…" bind:value={filter} />
    </div>
    <span class="muted small">{mods ? `共 ${mods.length} 个，已启用 ${enabledCount} 个` : "读取中…"}</span>
    <button class="btn sm" onclick={() => api.openInstanceDir(instance, "mods").catch((e) => showError(e))}>
      <Icon name="folder" size={12} /> 打开文件夹
    </button>
    <button class="btn sm" onclick={check} disabled={checking || !mods?.length}>
      <span class:spin={checking}><Icon name="refresh" size={12} /></span> 检查更新
    </button>
    {#if updateList.length}
      <button class="btn sm primary" onclick={() => update(updateList)} disabled={updating}>全部更新（{updateList.length}）</button>
    {/if}
  </div>

  <div class="list">
    {#if mods && mods.length === 0}
      <div class="empty">
        <h3>还没有安装 Mod</h3>
        <p>在「Mod」标签中搜索并安装，或者把 jar 文件放进 mods 文件夹。</p>
      </div>
    {:else if mods}
      <VirtualList items={shown} rowHeight={64} key={(m) => m.filename}>
        {#snippet row(m)}
          {@const up = updates[m.filename]}
          <div class="row" class:off={!m.enabled}>
            {#if m.icon}
              <img src={m.icon} alt="" class="icon" />
            {:else}
              <span class="icon blank"><Icon name="cube" size={18} /></span>
            {/if}
            <div class="main">
              <div class="title">
                <strong>{m.name}</strong>
                <span class="mono small muted">{m.version}</span>
                {#if m.loader}<span class="tag">{LOADER_LABEL[m.loader] ?? m.loader}</span>{/if}
                {#if up}<span class="tag accent">可更新到 {up.latest.versionNumber}</span>{/if}
              </div>
              <p class="desc" title={m.description}>{m.description || m.filename}</p>
              <p class="file mono small muted">{m.filename} · {bytes(m.size)}</p>
            </div>
            {#if up}
              <button class="btn sm" disabled={updating} onclick={() => update([up])}>更新</button>
            {/if}
            <Toggle checked={m.enabled} onchange={(v) => toggle(m, v)} />
            <button
              class="btn ghost icon sm"
              title="删除"
              onclick={() => {
                confirmDelete = m;
                deleteOpen = true;
              }}><Icon name="trash" size={14} /></button
            >
          </div>
        {/snippet}
      </VirtualList>
    {/if}
  </div>
</div>

<Modal bind:open={deleteOpen} title="删除 Mod？" kicker="DELETE" width={420}>
  <p>将从实例「{instance}」中删除 <strong>{confirmDelete?.name}</strong>（{confirmDelete?.filename}），此操作无法撤销。</p>
  {#snippet actions()}
    <button class="btn" onclick={() => (deleteOpen = false)}>取消</button>
    <button class="btn primary" onclick={remove}>确认删除</button>
  {/snippet}
</Modal>

<style>
  .wrap {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
  }
  .search {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    color: var(--muted);
  }
  .search .input {
    width: 100%;
    padding-left: 30px;
  }
  .small {
    font-size: 11px;
  }
  .list {
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--rule-strong);
  }
  .row {
    height: 64px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 6px;
    border-bottom: 1px solid var(--rule);
    transition: opacity var(--t-mid);
  }
  .row.off {
    opacity: 0.5;
  }
  .icon {
    width: 40px;
    height: 40px;
    border: 1.5px solid var(--ink);
    object-fit: cover;
    image-rendering: pixelated;
    flex: none;
    background: var(--paper-2);
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .main {
    flex: 1;
    min-width: 0;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    white-space: nowrap;
    overflow: hidden;
  }
  .title strong {
    font-family: var(--serif);
    font-size: 15px;
  }
  .desc,
  .file {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .desc {
    font-size: 12px;
    color: var(--ink-2);
  }
</style>
