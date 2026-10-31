<script lang="ts">
  // 手机版「版本」页：已安装的游戏版本卡片，点按选择；资源管理、补全文件、删除放在卡片的操作区
  import { api, type FileKind, type InstanceInfo } from "../../lib/api";
  import { app, refreshInstances, selectInstance, selectedInstance, showError, toast } from "../../lib/state.svelte";
  import { KIND_LABEL, LOADER_LABEL, relative } from "../../lib/format";
  import Icon from "../../components/Icon.svelte";
  import Modal from "../../components/Modal.svelte";

  const current = $derived(selectedInstance());
  let confirmDelete = $state<InstanceInfo | null>(null);
  let deleteOpen = $state(false);

  async function choose(i: InstanceInfo) {
    await selectInstance(i.id);
    app.page = "launch";
  }

  function manage(i: InstanceInfo, kind: FileKind) {
    selectInstance(i.id).then(() => {
      app.manage = kind;
      app.page = "mods";
    });
  }

  async function remove() {
    if (!confirmDelete) return;
    const id = confirmDelete.id;
    try {
      await api.deleteInstance(id);
      toast("ok", `已删除版本 ${id}`);
      deleteOpen = false;
      await refreshInstances();
    } catch (e) {
      showError(e, "删除失败");
    }
  }
</script>

<div class="wrap">
  <header class="head">
    <h1>游戏版本</h1>
    <span class="count">{app.instances.length} 个</span>
    <span class="spacer"></span>
    <button class="btn sm primary" onclick={() => (app.page = "download")}><Icon name="plus" size={12} /> 安装新版本</button>
  </header>

  {#if app.instancesLoaded && !app.instances.length}
    <div class="empty">
      <h3>还没有安装游戏版本</h3>
      <p>在「下载」中选择一个 Minecraft 版本安装。</p>
    </div>
  {:else}
    <div class="grid">
      {#each app.instances as i (i.id)}
        <article class="card" class:active={current?.id === i.id}>
          <button class="main" disabled={!!i.error} onclick={() => choose(i)}>
            <span class="badge" data-loader={i.loader?.kind ?? (i.optifine ? "optifine" : "vanilla")}>
              {i.loader ? LOADER_LABEL[i.loader.kind][0] : i.optifine ? "O" : "V"}
            </span>
            <span class="who">
              <strong>{i.id}</strong>
              {#if i.error}
                <small class="err">{i.error}</small>
              {:else}
                <small>
                  {i.mcVersion} · {KIND_LABEL[i.kind] ?? i.kind}{#if i.loader} · {LOADER_LABEL[i.loader.kind]}{/if}
                </small>
                <small>{relative(i.lastPlayed)}</small>
              {/if}
            </span>
            {#if current?.id === i.id}<span class="tag accent">当前</span>{/if}
          </button>
          <div class="ops">
            <button class="op" onclick={() => manage(i, "mod")}><Icon name="cube" size={14} />资源</button>
            <button class="op" onclick={() => manage(i, "save")}><Icon name="box" size={14} />存档</button>
            <button class="op" onclick={() => api.repairInstance(i.id).catch((e) => showError(e))}><Icon name="refresh" size={14} />补全</button>
            <button
              class="op danger"
              disabled={i.running}
              onclick={() => {
                confirmDelete = i;
                deleteOpen = true;
              }}><Icon name="trash" size={14} />删除</button
            >
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>

<Modal bind:open={deleteOpen} title="删除版本？" width={380}>
  <p>将删除版本「{confirmDelete?.id}」及其中的 Mod、存档等全部文件，此操作无法撤销。</p>
  {#snippet actions()}
    <button class="btn" onclick={() => (deleteOpen = false)}>取消</button>
    <button class="btn primary" onclick={remove}>确认删除</button>
  {/snippet}
</Modal>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }
  .head h1 {
    font-size: 20px;
    font-weight: 800;
  }
  .count {
    font-size: 12px;
    color: var(--muted);
  }
  .spacer {
    flex: 1;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 10px;
  }
  .card {
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--card);
    box-shadow: var(--shadow-sm);
    overflow: hidden;
  }
  .card.active {
    border-color: color-mix(in srgb, var(--accent) 60%, transparent);
  }
  .main {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: 0;
    background: none;
    color: var(--ink);
    text-align: left;
  }
  .main:active {
    background: color-mix(in srgb, var(--ink) 5%, transparent);
  }
  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .who strong {
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .who small {
    font-size: 11px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .who .err {
    color: var(--danger);
  }
  .badge {
    width: 38px;
    height: 38px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    font-weight: 800;
    font-size: 15px;
    flex: none;
    background: var(--paper-2);
    color: var(--ink);
  }
  .badge[data-loader="fabric"] { background: #dbd0b4; color: #3b3424; }
  .badge[data-loader="forge"] { background: #3d4555; color: #fff; }
  .badge[data-loader="neoforge"] { background: #d9822b; color: #fff; }
  .badge[data-loader="quilt"] { background: #8a5cd6; color: #fff; }
  .badge[data-loader="optifine"] { background: #b52e31; color: #fff; }
  .ops {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    border-top: 1px solid var(--line);
  }
  .op {
    height: 34px;
    border: 0;
    background: none;
    color: var(--ink-2);
    font-size: 11.5px;
    font-weight: 600;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
  }
  .op + .op {
    border-left: 1px solid var(--line);
  }
  .op:active {
    background: color-mix(in srgb, var(--ink) 6%, transparent);
  }
  .op.danger {
    color: var(--danger);
  }
  .op:disabled {
    opacity: 0.4;
  }
  .empty {
    padding: 40px 0;
    text-align: center;
    color: var(--muted);
  }
  .empty h3 {
    color: var(--ink);
    font-size: 16px;
    margin-bottom: 4px;
  }
</style>
