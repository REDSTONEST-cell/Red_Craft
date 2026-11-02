<script lang="ts">
  import { iconSrc, zhName } from "../lib/zh";
  // 项目详情：只列出兼容当前实例的版本，可按游戏版本、加载器、类型筛选，一键安装最新的版本。
  import { api, type ContentVersion, type ProjectHit } from "../lib/api";
  import { count, date, LOADER_LABEL } from "../lib/format";
  import { filterOptions, matchesFilter, TYPE_LABEL, type VersionFilter } from "../lib/versionFilter";
  import Modal from "./Modal.svelte";
  import ProgressBar from "./ProgressBar.svelte";
  import VirtualList from "./VirtualList.svelte";

  let {
    open = $bindable(false),
    project,
    instance,
    compatibleOnly,
    oninstall,
  }: {
    open: boolean;
    project: ProjectHit | null;
    /** 为空时不按实例筛选（整合包） */
    instance: string | null;
    compatibleOnly: boolean;
    oninstall: (v: ContentVersion) => void;
  } = $props();

  let versions = $state<ContentVersion[] | null>(null);
  let error = $state<string | null>(null);
  let filter = $state<VersionFilter>({ game: null, loader: null, type: null });

  $effect(() => {
    if (!open || !project) return;
    const p = project;
    versions = null;
    error = null;
    filter = { game: null, loader: null, type: null };
    api
      .contentVersions(p.platform, p.id, p.contentType, compatibleOnly ? instance : null)
      .then((v) => (versions = v))
      .catch((e) => (error = `${e.message}${e.hint ? "\n" + e.hint : ""}`));
  });

  const options = $derived(filterOptions(versions ?? [], filter));
  const shown = $derived(versions?.filter((v) => matchesFilter(v, filter)) ?? []);
  const filtered = $derived(filter.game !== null || filter.loader !== null || filter.type !== null);
  const best = $derived(shown.find((v) => v.releaseType === "release") ?? shown[0] ?? null);
  const typeLabel = TYPE_LABEL;
  // <select> 的「全部」用空字符串表示
  const pick = (key: keyof VersionFilter) => (e: Event) => {
    const value = (e.currentTarget as HTMLSelectElement).value;
    filter = { ...filter, [key]: value === "" ? null : value };
  };
</script>

<Modal bind:open title={project ? (project.zhName ?? zhName(project.title, project.slug) ?? project.title) : ""} kicker={project?.platform === "curseforge" ? "CURSEFORGE" : "MODRINTH"} width={720}>
  {#if project}
    <div class="head">
      {#if project.iconUrl}<img src={iconSrc(project.iconUrl)} alt="" class="icon" />{:else}<span class="icon blank"></span>{/if}
      <div>
        <p class="desc">{project.zhDescription ?? project.description}</p>
        {#if project.zhDescription}<p class="muted small">{project.description}</p>{/if}
        <p class="muted small">
          {#if project.author}{project.author} · {/if}{count(project.downloads)} 次下载 · 更新于 {date(project.updated)}
        </p>
      </div>
    </div>
  {/if}
  {#if error}
    <p class="err">{error}</p>
  {:else if versions === null}
    <ProgressBar indeterminate size="sm" />
  {:else if versions.length === 0}
    <div class="empty">
      <h3>没有兼容的版本</h3>
      <p>{#if instance}这个项目没有适用于「{instance}」的游戏版本和加载器的文件。{:else}这个项目暂时没有可下载的文件。{/if}</p>
    </div>
  {:else}
    <div class="filters">
      {#if options.game.length > 1}
        <label>游戏版本
          <select class="input" value={filter.game ?? ""} onchange={pick("game")}>
            <option value="">全部（{options.gameTotal}）</option>
            {#each options.game as [g, n] (g)}<option value={g}>{g}（{n}）</option>{/each}
          </select>
        </label>
      {/if}
      {#if options.loader.length > 1}
        <label>加载器
          <select class="input" value={filter.loader ?? ""} onchange={pick("loader")}>
            <option value="">全部</option>
            {#each options.loader as [l, n] (l)}<option value={l}>{LOADER_LABEL[l] ?? l}（{n}）</option>{/each}
          </select>
        </label>
      {/if}
      {#if options.type.length > 1}
        <label>类型
          <select class="input" value={filter.type ?? ""} onchange={pick("type")}>
            <option value="">全部</option>
            {#each options.type as [t, n] (t)}<option value={t}>{typeLabel[t]}（{n}）</option>{/each}
          </select>
        </label>
      {/if}
      <span class="muted small count">{filtered ? `符合条件 ${shown.length} 个` : `共 ${versions.length} 个版本`}</span>
      {#if filtered}<button class="btn ghost sm" onclick={() => (filter = { game: null, loader: null, type: null })}>清除筛选</button>{/if}
    </div>
    <div class="list">
      <VirtualList items={shown} rowHeight={52} key={(v) => v.id}>
        {#snippet row(v)}
          <div class="row">
            <div class="main">
              <strong>{v.name || v.versionNumber}</strong>
              <span class="muted small mono">
                {v.gameVersions.slice(0, 4).join(", ")}{v.gameVersions.length > 4 ? "…" : ""}
                {#if v.loaders.length}· {v.loaders.join(", ")}{/if}
              </span>
            </div>
            <span class="tag" class:accent={v.releaseType === "release"}>{typeLabel[v.releaseType] ?? v.releaseType}</span>
            <span class="muted small mono date">{date(v.published)}</span>
            <button class="btn sm" onclick={() => oninstall(v)}>安装</button>
          </div>
        {/snippet}
      </VirtualList>
    </div>
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (open = false)}>关闭</button>
    <button class="btn primary" disabled={!best} onclick={() => best && oninstall(best)}>
      {filtered ? "安装所选条件的最新版本" : "安装最新兼容版本"}{best ? `（${best.versionNumber}）` : ""}
    </button>
  {/snippet}
</Modal>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 8px 12px;
    margin-bottom: 10px;
  }
  .filters label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
    color: var(--muted);
  }
  .filters select {
    min-width: 130px;
    height: 30px;
  }
  .filters .count {
    margin-left: auto;
    align-self: center;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: flex-start;
    margin-bottom: 14px;
  }
  .icon {
    width: 56px;
    height: 56px;
    border: 1.5px solid var(--ink);
    object-fit: cover;
    image-rendering: pixelated;
    flex: none;
  }
  .icon.blank {
    background: var(--paper-2);
  }
  .desc {
    font-family: var(--serif);
    font-size: 15px;
  }
  .small {
    font-size: 11px;
  }
  .list {
    height: 46vh;
    border-top: 1px solid var(--rule-strong);
  }
  .row {
    height: 52px;
    display: flex;
    align-items: center;
    gap: 12px;
    border-bottom: 1px solid var(--rule);
    padding: 0 4px;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .main strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .date {
    width: 80px;
  }
  .err {
    color: var(--danger);
    white-space: pre-line;
  }
</style>
