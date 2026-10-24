<script lang="ts">
  // 资源下载：Mod / 材质包 / 光影包 / 整合包，数据源 Modrinth 或 CurseForge，按实例筛选。
  import {
    api,
    type ContentType,
    type ContentVersion,
    type InstanceContext,
    type PackSource,
    type Platform,
    type ProjectHit,
    type SortBy,
  } from "../lib/api";
  import { app, selectedInstance, showError, toast } from "../lib/state.svelte";
  import { count, date, LOADER_LABEL } from "../lib/format";
  import Segmented from "../components/Segmented.svelte";
  import VirtualList from "../components/VirtualList.svelte";
  import ContentDetail from "../components/ContentDetail.svelte";
  import InstallPlan from "../components/InstallPlan.svelte";
  import ModpackInstall from "../components/ModpackInstall.svelte";
  import Icon from "../components/Icon.svelte";
  import Toggle from "../components/Toggle.svelte";

  function stored<T extends string>(key: string, fallback: T): T {
    try {
      return (localStorage.getItem(key) as T) || fallback;
    } catch {
      return fallback;
    }
  }
  function store(key: string, v: string) {
    try {
      localStorage.setItem(key, v);
    } catch {
      /* 忽略 */
    }
  }

  let platform = $state<Platform>(stored("rc.platform", "modrinth"));
  let tab = $state<ContentType>(stored("rc.contentTab", "mod"));
  let sort = $state<SortBy>("relevance");
  let query = $state("");
  let instanceId = $state<string | null>(selectedInstance()?.id ?? null);
  let ctx = $state<InstanceContext | null>(null);
  let compatibleOnly = $state(true);

  let hits = $state<ProjectHit[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<{ message: string; hint: string } | null>(null);
  let generation = 0;

  $effect(() => store("rc.platform", platform));
  $effect(() => store("rc.contentTab", tab));

  // 实例变化时获取它的游戏版本与加载器
  $effect(() => {
    const id = instanceId;
    ctx = null;
    if (!id) return;
    api
      .contentInstance(id)
      .then((c) => (ctx = c))
      .catch((e) => showError(e, "读取实例信息失败"));
  });

  const useFilter = $derived(compatibleOnly && tab !== "modpack" && !!ctx);

  async function search(reset: boolean) {
    if (!reset && (loading || hits.length >= total)) return;
    const gen = ++generation;
    loading = true;
    error = null;
    try {
      const res = await api.contentSearch(platform, {
        query: query.trim(),
        contentType: tab,
        gameVersion: useFilter ? ctx!.mcVersion : null,
        loader: useFilter ? ctx!.loader : null,
        sort,
        offset: reset ? 0 : hits.length,
        limit: 20,
      });
      if (gen !== generation) return;
      hits = reset ? res.hits : [...hits, ...res.hits];
      total = res.total;
    } catch (e) {
      if (gen !== generation) return;
      const err = e as { message: string; hint: string };
      error = { message: err.message, hint: err.hint };
      if (reset) hits = [];
    } finally {
      if (gen === generation) loading = false;
    }
  }

  // 条件变化时重新搜索；输入关键词时防抖
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    void [platform, tab, sort, useFilter, ctx?.mcVersion, ctx?.loader];
    const q = query;
    clearTimeout(timer);
    timer = setTimeout(() => search(true), q ? 350 : 0);
    return () => clearTimeout(timer);
  });

  // 详情与安装
  let detailOpen = $state(false);
  let current = $state<ProjectHit | null>(null);
  let planOpen = $state(false);
  let planVersion = $state<ContentVersion | null>(null);
  let packOpen = $state(false);
  let packSource = $state<PackSource | null>(null);

  function openDetail(h: ProjectHit) {
    current = h;
    detailOpen = true;
  }

  function onInstall(v: ContentVersion) {
    if (!current) return;
    if (current.contentType === "modpack") {
      packSource = { type: "online", platform: current.platform, projectId: v.projectId, versionId: v.id };
      detailOpen = false;
      packOpen = true;
      return;
    }
    if (!instanceId) {
      toast("error", "请先选择要安装到的实例");
      return;
    }
    planVersion = v;
    detailOpen = false;
    planOpen = true;
  }

  const tabs: { value: ContentType; label: string }[] = [
    { value: "mod", label: "Mod" },
    { value: "resourcepack", label: "材质包" },
    { value: "shader", label: "光影包" },
    { value: "modpack", label: "整合包" },
  ];
</script>

<div class="wrap">
  <div class="page-head">
    <div>
      <div class="kicker">RESOURCES</div>
      <h1>资源下载</h1>
      <div class="sub">
        {#if tab === "modpack"}整合包会创建新的实例{:else if ctx}
          安装到「{ctx.id}」· Minecraft {ctx.mcVersion}{#if ctx.loader} · {LOADER_LABEL[ctx.loader]}{/if}
        {:else}请选择要安装到的实例{/if}
      </div>
    </div>
    <div class="tools">
      {#if tab !== "modpack"}
        <select class="input inst" bind:value={instanceId} title="安装到的实例">
          {#each app.instances as i}
            <option value={i.id}>{i.id}</option>
          {/each}
        </select>
      {/if}
      <Segmented
        bind:value={platform}
        options={[
          { value: "modrinth", label: "Modrinth" },
          { value: "curseforge", label: "CurseForge" },
        ]}
      />
    </div>
  </div>

  <div class="bar">
    <Segmented bind:value={tab} options={tabs} />
    <div class="search">
      <Icon name="search" size={14} />
      <input class="input" placeholder={`搜索${tabs.find((t) => t.value === tab)?.label}…`} bind:value={query} />
    </div>
    <select class="input sort" bind:value={sort}>
      <option value="relevance">相关度</option>
      <option value="downloads">下载量</option>
      <option value="updated">最近更新</option>
      <option value="newest">最新发布</option>
    </select>
    {#if tab !== "modpack"}
      <Toggle bind:checked={compatibleOnly} label="仅兼容" />
    {/if}
  </div>

  <div class="results">
    {#if error}
      <div class="empty">
        <h3>搜索失败</h3>
        <p>{error.message}</p>
        <p class="muted">{error.hint}</p>
        <button class="btn" style="margin-top:14px" onclick={() => search(true)}>重试</button>
      </div>
    {:else if !loading && hits.length === 0}
      <div class="empty"><h3>没有找到结果</h3><p>换个关键词，或关闭「仅兼容」试试。</p></div>
    {:else}
      <VirtualList items={hits} rowHeight={84} key={(h) => `${h.platform}:${h.id}`} onEnd={() => search(false)}>
        {#snippet row(h)}
          <button class="hit" onclick={() => openDetail(h)}>
            {#if h.iconUrl}
              <img src={h.iconUrl} alt="" loading="lazy" class="icon" />
            {:else}
              <span class="icon blank"><Icon name="cube" size={20} /></span>
            {/if}
            <div class="main">
              <div class="title">
                <strong>{h.title}</strong>
                {#if h.author}<span class="muted small">by {h.author}</span>{/if}
              </div>
              <p class="desc">{h.description}</p>
              <div class="meta">
                <span class="mono small"><Icon name="download" size={11} /> {count(h.downloads)}</span>
                <span class="muted small">更新于 {date(h.updated)}</span>
                {#each h.categories.slice(0, 3) as c}<span class="tag">{c}</span>{/each}
              </div>
            </div>
          </button>
        {/snippet}
      </VirtualList>
    {/if}
  </div>
  <div class="foot muted small">
    {#if loading}加载中…{:else if total}共 {count(total)} 个结果，已显示 {hits.length} 个{/if}
  </div>
</div>

<ContentDetail
  bind:open={detailOpen}
  project={current}
  instance={tab === "modpack" ? null : instanceId}
  {compatibleOnly}
  oninstall={onInstall}
/>
<ModpackInstall bind:open={packOpen} source={packSource} title={current?.title ?? ""} />
{#if current && instanceId}
  <InstallPlan
    bind:open={planOpen}
    {platform}
    instance={instanceId}
    contentType={current.contentType}
    version={planVersion}
    title={current.title}
  />
{/if}

<style>
  .wrap {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .tools {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .inst {
    width: 200px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
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
  .sort {
    width: 110px;
  }
  .results {
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--rule-strong);
  }
  .hit {
    width: 100%;
    height: 84px;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 6px;
    border: 0;
    border-bottom: 1px solid var(--rule);
    background: transparent;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--t-fast);
  }
  .hit:hover {
    background: var(--accent-soft);
  }
  .hit:active .icon {
    transform: scale(0.95);
  }
  .icon {
    width: 52px;
    height: 52px;
    border: 1.5px solid var(--ink);
    object-fit: cover;
    flex: none;
    background: var(--paper-2);
    display: grid;
    place-items: center;
    color: var(--muted);
    transition: transform var(--t-fast) var(--ease);
  }
  .main {
    flex: 1;
    min-width: 0;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .title strong {
    font-family: var(--serif);
    font-size: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .desc {
    font-size: 12px;
    color: var(--ink-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin: 2px 0 4px;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .small {
    font-size: 11px;
  }
  .foot {
    padding-top: 6px;
    min-height: 22px;
  }
</style>
