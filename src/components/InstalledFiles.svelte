<script lang="ts">
  // 已安装的材质包 / 光影包 / 存档：启用材质包、切换光影、导入、移到回收站。
  import { api, type FileKind, type LocalFile } from "../lib/api";
  import { showError, toast } from "../lib/state.svelte";
  import { bytes, relative } from "../lib/format";
  import VirtualList from "./VirtualList.svelte";
  import Toggle from "./Toggle.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let {
    instance,
    kind,
    reload = 0,
    onimport,
  }: { instance: string; kind: Exclude<FileKind, "mod">; reload?: number; onimport: () => void } = $props();

  const LABEL = { resourcepack: "材质包", shader: "光影包", save: "存档" } as const;
  const FOLDER = { resourcepack: "resourcepacks", shader: "shaderpacks", save: "saves" } as const;

  let files = $state<LocalFile[] | null>(null);
  let filter = $state("");
  let busy = $state(false);
  let confirmDelete = $state<LocalFile | null>(null);
  let deleteOpen = $state(false);

  async function load() {
    try {
      files = await api.filesList(instance, kind);
    } catch (e) {
      showError(e, `读取${LABEL[kind]}失败`);
      files = [];
    }
  }

  $effect(() => {
    void [instance, kind, reload];
    files = null;
    filter = "";
    load();
  });

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = files ?? [];
    return q
      ? list.filter((f) => f.title.toLowerCase().includes(q) || f.name.toLowerCase().includes(q) || f.description.toLowerCase().includes(q))
      : list;
  });
  const activeShader = $derived(kind === "shader" ? (files ?? []).find((f) => f.active) : undefined);
  const enabledCount = $derived((files ?? []).filter((f) => f.active).length);

  async function setActive(f: LocalFile, active: boolean) {
    busy = true;
    try {
      await api.fileSetActive(instance, kind, f.name, active);
      if (kind === "shader") toast("ok", active ? `已切换到光影 ${f.title}` : "已关闭光影", "下次进入游戏时生效。");
      await load();
    } catch (e) {
      showError(e, "操作失败");
      await load();
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!confirmDelete) return;
    try {
      await api.filesDelete(instance, kind, [confirmDelete.name]);
      toast("ok", `已将「${confirmDelete.title}」移到回收站`);
      deleteOpen = false;
      await load();
    } catch (e) {
      showError(e, "删除失败");
    }
  }
</script>

<div class="wrap">
  <div class="bar">
    <div class="search">
      <Icon name="search" size={14} />
      <input class="input" placeholder="筛选{LABEL[kind]}…" bind:value={filter} />
    </div>
    <span class="muted small">
      {#if !files}读取中…
      {:else if kind === "resourcepack"}共 {files.length} 个，已启用 {enabledCount} 个
      {:else if kind === "shader"}共 {files.length} 个{activeShader ? `，正在使用 ${activeShader.title}` : "，未使用光影"}
      {:else}共 {files.length} 个{/if}
    </span>
    {#if activeShader}
      <button class="btn sm" disabled={busy} onclick={() => setActive(activeShader, false)}>关闭光影</button>
    {/if}
    <button class="btn sm" onclick={() => api.openInstanceDir(instance, FOLDER[kind]).catch((e) => showError(e))}>
      <Icon name="folder" size={12} /> 打开文件夹
    </button>
    <button class="btn sm" onclick={onimport}><Icon name="plus" size={12} /> 导入{LABEL[kind]}</button>
  </div>

  <div class="list">
    {#if files && files.length === 0}
      <div class="empty">
        <h3>还没有{LABEL[kind]}</h3>
        <p>
          {#if kind === "save"}点击「导入存档」选择存档文件夹，或把存档文件夹、存档压缩包拖进窗口。
          {:else}在「{LABEL[kind]}」标签中搜索并安装，或点击「导入{LABEL[kind]}」、把 zip 文件拖进窗口。{/if}
        </p>
      </div>
    {:else if files}
      <VirtualList items={shown} rowHeight={64} key={(f) => f.name}>
        {#snippet row(f)}
          <div class="row" class:on={f.active}>
            {#if f.icon}
              <img src={f.icon} alt="" class="icon" />
            {:else}
              <span class="icon blank"><Icon name={kind === "save" ? "box" : "cube"} size={18} /></span>
            {/if}
            <div class="main">
              <div class="title">
                <strong title={f.name}>{f.title}</strong>
                {#if kind === "shader" && f.active}<span class="tag accent">使用中</span>{/if}
                {#if f.dir && kind !== "save"}<span class="tag">文件夹</span>{/if}
              </div>
              {#if f.detail || f.description}
                <p class="desc" title={f.description}>{f.detail ?? f.description}</p>
              {/if}
              <p class="file small muted">
                {#if kind === "save"}{f.name} · 最后游玩 {relative(f.modified)} · {bytes(f.size)}
                {:else}<span class="mono">{f.name}</span> · {bytes(f.size)}{/if}
              </p>
            </div>
            {#if kind === "resourcepack"}
              <Toggle checked={f.active} disabled={busy} onchange={(v) => setActive(f, v)} />
            {:else if kind === "shader" && !f.active}
              <button class="btn sm" disabled={busy} onclick={() => setActive(f, true)}>使用</button>
            {/if}
            <button
              class="btn ghost icon sm"
              title="移到回收站"
              onclick={() => {
                confirmDelete = f;
                deleteOpen = true;
              }}><Icon name="trash" size={14} /></button
            >
          </div>
        {/snippet}
      </VirtualList>
    {/if}
  </div>
</div>

<Modal bind:open={deleteOpen} title="删除{LABEL[kind]}？" kicker="DELETE" width={440}>
  <p>
    将把实例「{instance}」中的 <strong>{confirmDelete?.title}</strong>（{confirmDelete?.name}）移到系统回收站，需要时可以从回收站恢复。
  </p>
  {#snippet actions()}
    <button class="btn" onclick={() => (deleteOpen = false)}>取消</button>
    <button class="btn primary" onclick={remove}>移到回收站</button>
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
  }
  /* 只作用于图标图片，不影响带 icon 类的按钮 */
  .icon:not(.btn) {
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
    overflow: hidden;
    text-overflow: ellipsis;
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
