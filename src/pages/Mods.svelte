<script lang="ts">
  // 资源下载：Mod / 材质包 / 光影包 / 整合包，数据源 Modrinth 或 CurseForge，按实例筛选。
  import {
    api,
    type ContentType,
    type ContentVersion,
    type FileKind,
    type InstanceContext,
    type PackSource,
    type Platform,
    type ProjectHit,
    type SortBy,
    type SearchPlatform,
    type ZhMatch,
  } from "../lib/api";
  import { app, selectedInstance, showError, toast } from "../lib/state.svelte";
  import { count, date, LOADER_LABEL } from "../lib/format";
  import Segmented from "../components/Segmented.svelte";
  import VirtualList from "../components/VirtualList.svelte";
  import ContentDetail from "../components/ContentDetail.svelte";
  import MobileContentDetail from "../mobile/ContentDetail.svelte";
  import InstallPlan from "../components/InstallPlan.svelte";
  import ModpackInstall from "../components/ModpackInstall.svelte";
  import ResourceManager from "../components/ResourceManager.svelte";
  import Dropdown from "../components/Dropdown.svelte";
  import Download from "./Download.svelte";
  import Icon from "../components/Icon.svelte";
  import { fade } from "svelte/transition";
  import Toggle from "../components/Toggle.svelte";
  import { iconSrc, zhName } from "../lib/zh";
  import { getContext, getQuery, getSearch, patchSearch, putContext, putQuery, putSearch } from "../lib/searchCache";
  import { dur } from "../lib/motion";

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

  // 默认聚合搜索两个平台（与 PCL 相同）
  let platform = $state<SearchPlatform>(stored("rc.searchPlatform", "all"));
  // 电脑版把「下载游戏」也放在这个页面里（games），手机版仍是单独的页面
  let { games = false }: { games?: boolean } = $props();
  type Tab = ContentType | "installed" | "game";
  function initialTab(): Tab {
    const saved = stored("rc.contentTab", "mod") as Tab;
    if (!games) return saved === "game" ? "mod" : saved;
    // 还没有安装任何版本时先看游戏列表
    return app.openGames || (app.instancesLoaded && app.instances.length === 0) ? "game" : saved;
  }
  let tab = $state<Tab>(initialTab());
  // 搜索用的类型（「游戏」「资源管理」标签不搜索）
  const searchType = $derived<ContentType>(tab === "installed" || tab === "game" ? "mod" : tab);
  let sort = $state<SortBy>("relevance");
  // 每类资源各自的搜索词，切换标签时恢复
  let query = $state("");
  let queryTab = "";
  $effect(() => {
    const t = searchType;
    if (t !== queryTab) {
      queryTab = t;
      query = getQuery(t);
    }
  });
  $effect(() => {
    if (queryTab) putQuery(queryTab, query);
  });
  const initialId = selectedInstance()?.id ?? null;
  let instanceId = $state<string | null>(initialId);
  let ctx = $state<InstanceContext | null>(initialId ? getContext(initialId) : null);
  let compatibleOnly = $state(true);

  let hits = $state<ProjectHit[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<{ message: string; hint: string } | null>(null);
  let generation = 0;
  // 每次换成新的一组结果时加一：旧列表淡出、新列表的行淡入
  let listGen = $state(0);
  // 正在为哪一组结果重新搜索（这组结果会变淡，直到新结果到来）
  let staleGen = $state(-1);
  // 中文关键词转换后的提示，例如「万用皮肤补丁 → CustomSkinLoader」，以及其他可能的匹配
  let converted = $state<ZhMatch | null>(null);
  let alternatives = $state<ZhMatch[]>([]);
  // 两个平台各自的分页位置
  let cursor = { modrinthOffset: 0, curseforgeOffset: 0, modrinthTotal: null as number | null, curseforgeTotal: null as number | null };
  const hasMore = $derived.by(() => {
    void hits.length;
    const mr = platform !== "curseforge" && (cursor.modrinthTotal === null || cursor.modrinthOffset < cursor.modrinthTotal);
    const cf = platform !== "modrinth" && (cursor.curseforgeTotal === null || cursor.curseforgeOffset < cursor.curseforgeTotal);
    return mr || cf;
  });

  // 资源管理中正在查看的类别
  let manageKind = $state<FileKind>(stored("rc.manageKind", "mod") as FileKind);

  $effect(() => store("rc.searchPlatform", platform));
  $effect(() => store("rc.contentTab", tab));
  $effect(() => store("rc.manageKind", manageKind));
  // 从「安装新版本」「去下载」等按钮跳转过来
  $effect(() => {
    if (!app.openGames) return;
    if (games) tab = "game";
    app.openGames = false;
  });
  // 从主页面「资源管理」「存档」等按钮跳转过来
  $effect(() => {
    const k = app.manage;
    if (!k) return;
    tab = "installed";
    manageKind = k;
    instanceId = selectedInstance()?.id ?? instanceId;
    app.manage = null;
  });

  // 实例变化时获取它的游戏版本与加载器
  $effect(() => {
    const id = instanceId;
    // 读取过的版本信息直接使用，不必等待
    ctx = id ? getContext(id) : null;
    if (!id) return;
    api
      .contentInstance(id)
      .then((c) => {
        putContext(c);
        if (instanceId !== id) return;
        // 信息没有变化时保持原对象，避免重复搜索
        if (!ctx || ctx.mcVersion !== c.mcVersion || ctx.loader !== c.loader) ctx = c;
      })
      .catch((e) => showError(e, "读取版本信息失败"));
  });

  const useFilter = $derived(compatibleOnly && searchType !== "modpack" && !!ctx);

  // 同一个项目只显示一次：同平台按 id，跨平台按名称
  const nameKey = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
  function appendUnique(list: ProjectHit[], add: ProjectHit[]) {
    const ids = new Set(list.map((h) => `${h.platform}:${h.id}`));
    const names = new Set(list.map((h) => nameKey(h.title)));
    const out = [...list];
    for (const h of add) {
      if (ids.has(`${h.platform}:${h.id}`) || names.has(nameKey(h.title))) continue;
      ids.add(`${h.platform}:${h.id}`);
      names.add(nameKey(h.title));
      out.push(h);
    }
    return out;
  }

  // 当前条件对应的缓存键
  function searchKey(type: ContentType = searchType) {
    const filter = compatibleOnly && type !== "modpack" && ctx;
    return JSON.stringify([platform, type, query.trim(), sort, filter ? ctx!.mcVersion : null, filter ? ctx!.loader : null]);
  }
  let currentKey = "";

  function request(type: ContentType, from: typeof cursor) {
    const filter = compatibleOnly && type !== "modpack" && ctx;
    return api.contentSearch({
      platform,
      query: {
        query: query.trim(),
        contentType: type,
        gameVersion: filter ? ctx!.mcVersion : null,
        loader: filter ? ctx!.loader : null,
        sort,
        offset: 0,
        limit: 20,
      },
      ...from,
    });
  }

  async function search(reset: boolean) {
    if (tab === "installed" || tab === "game") return;
    if (!reset && (loading || !hasMore)) return;
    const gen = ++generation;
    error = null;
    if (reset) {
      currentKey = searchKey();
      // 已经加载过相同条件的结果时直接显示，不重新请求（与 PCL 相同）
      const cached = getSearch(currentKey);
      if (cached) {
        cursor = { ...cached.cursor };
        listGen++;
        hits = cached.hits;
        total = cached.total;
        converted = cached.converted;
        alternatives = cached.alternatives;
        staleGen = -1;
        loading = false;
        prefetch();
        return;
      } else {
        if (hits.length) staleGen = listGen;
      }
    }
    loading = true;
    const from = reset ? { modrinthOffset: 0, curseforgeOffset: 0, modrinthTotal: null, curseforgeTotal: null } : cursor;
    const key = currentKey;
    try {
      const res = await request(searchType, from);
      if (gen !== generation) return;
      cursor = {
        modrinthOffset: res.modrinthOffset,
        curseforgeOffset: res.curseforgeOffset,
        modrinthTotal: res.modrinthTotal,
        curseforgeTotal: res.curseforgeTotal,
      };
      if (reset) listGen++;
      hits = reset ? appendUnique([], res.hits) : appendUnique(hits, res.hits);
      total = (platform !== "curseforge" ? (res.modrinthTotal ?? 0) : 0) + (platform !== "modrinth" ? (res.curseforgeTotal ?? 0) : 0);
      if (reset) {
        converted = res.converted;
        alternatives = res.alternatives;
      }
      putSearch(key, { hits, total, cursor: { ...cursor }, converted, alternatives });
      translate(res.hits, gen);
      if (reset) prefetch();
    } catch (e) {
      if (gen !== generation) return;
      const err = e as { message: string; hint: string };
      error = { message: err.message, hint: err.hint };
      if (reset) hits = [];
    } finally {
      if (gen === generation) {
        loading = false;
        // 没有换成新结果（例如搜索失败）时恢复原样
        if (staleGen === listGen) staleGen = -1;
      }
    }
  }

  // 补充简介的中文翻译（失败时保持英文）；聚合时两个平台分别请求
  async function translate(list: ProjectHit[], gen: number) {
    for (const p of ["modrinth", "curseforge"] as Platform[]) {
      const ids = list.filter((h) => h.platform === p).map((h) => h.id);
      if (!ids.length) continue;
      api
        .contentTranslations(p, ids)
        .then((map) => {
          if (gen !== generation) return;
          hits = hits.map((h) => (h.platform === p && map[h.id] ? { ...h, zhDescription: map[h.id] } : h));
          patchSearch(currentKey, hits);
        })
        .catch(() => {
          /* 翻译不可用时保持英文 */
        });
    }
  }

  // 后台预先加载其他类型的默认列表，切换标签时可以立即显示（只在没有关键词时进行，依次请求）
  let prefetching = false;
  async function prefetch() {
    if (prefetching || query.trim()) return;
    prefetching = true;
    try {
      for (const type of ["mod", "resourcepack", "shader", "modpack"] as ContentType[]) {
        const key = searchKey(type);
        if (type === searchType || getSearch(key)) continue;
        try {
          const empty = { modrinthOffset: 0, curseforgeOffset: 0, modrinthTotal: null, curseforgeTotal: null };
          const res = await request(type, empty);
          if (key !== searchKey(type)) continue;
          const total = (platform !== "curseforge" ? (res.modrinthTotal ?? 0) : 0) + (platform !== "modrinth" ? (res.curseforgeTotal ?? 0) : 0);
          putSearch(key, {
            hits: appendUnique([], res.hits),
            total,
            cursor: {
              modrinthOffset: res.modrinthOffset,
              curseforgeOffset: res.curseforgeOffset,
              modrinthTotal: res.modrinthTotal,
              curseforgeTotal: res.curseforgeTotal,
            },
            converted: res.converted,
            alternatives: res.alternatives,
          });
        } catch {
          /* 预加载失败不影响使用 */
        }
      }
    } finally {
      prefetching = false;
    }
  }

  // 条件变化时重新搜索；输入关键词时防抖
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    void [platform, searchType, tab, sort, useFilter, ctx?.mcVersion, ctx?.loader];
    const q = query;
    clearTimeout(timer);
    // 开启「仅兼容」时等版本信息读取完成再搜索，避免先按不筛选搜一遍
    if (compatibleOnly && instanceId && !ctx && searchType !== "modpack") return;
    // 输入关键词时防抖；已经加载过的条件（例如切换回之前的标签）立即显示
    const wait = q && !getSearch(searchKey()) ? 350 : 0;
    timer = setTimeout(() => search(true), wait);
    return () => clearTimeout(timer);
  });

  // 旧结果从当前的透明度淡出（重新搜索期间它已经变淡）
  function fadeAway(node: Element) {
    const from = Number(getComputedStyle(node).opacity) || 1;
    const el = node as HTMLElement;
    el.style.transition = "none";
    el.style.pointerEvents = "none";
    return { duration: dur(160), css: (t: number) => `opacity:${t * from}` };
  }

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
      toast("error", "请先选择要安装到的版本");
      return;
    }
    planVersion = v;
    detailOpen = false;
    planOpen = true;
  }

  const tabs = $derived<{ value: Tab; label: string }[]>([
    ...(games ? [{ value: "game" as Tab, label: "游戏" }] : []),
    { value: "mod", label: "Mod" },
    { value: "resourcepack", label: "材质包" },
    { value: "shader", label: "光影包" },
    { value: "modpack", label: "整合包" },
    { value: "installed", label: "资源管理" },
  ]);
  const manageKinds: { value: FileKind; label: string }[] = [
    { value: "mod", label: "Mod" },
    { value: "resourcepack", label: "材质包" },
    { value: "shader", label: "光影包" },
    { value: "save", label: "存档" },
  ];
</script>

{#if tab === "game"}
  <Download>
    {#snippet bar()}
      <div class="bar"><Segmented bind:value={tab} options={tabs} /></div>
    {/snippet}
  </Download>
{:else}
<div class="wrap">
  <div class="page-head">
    <div>
      <div class="kicker">RESOURCES</div>
      <h1>{tab === "installed" ? "资源管理" : "资源下载"}</h1>
      <div class="sub">
        {#if tab === "installed"}{instanceId ? `管理「${instanceId}」中的 Mod、材质包、光影包与存档` : "请先选择版本"}
        {:else if tab === "modpack"}整合包会安装为新的版本{:else if ctx}
          安装到「{ctx.id}」· Minecraft {ctx.mcVersion}{#if ctx.loader} · {LOADER_LABEL[ctx.loader]}{/if}
        {:else}请选择要安装到的版本{/if}
      </div>
    </div>
    <div class="tools">
      {#if tab !== "modpack"}
        <span class="inst">
          <Dropdown
            bind:value={instanceId}
            options={app.instances.map((i) => ({ value: i.id, label: i.id }))}
            placeholder="选择版本"
            title="安装到的版本"
          />
        </span>
      {/if}
      {#if tab !== "installed"}
        <Segmented
          bind:value={platform}
          options={[
            { value: "all", label: "聚合" },
            { value: "modrinth", label: "Modrinth" },
            { value: "curseforge", label: "CurseForge" },
          ]}
        />
      {/if}
    </div>
  </div>

  <div class="bar">
    <Segmented bind:value={tab} options={tabs} />
    {#if tab === "installed"}
      <span class="spacer"></span>
      <Segmented bind:value={manageKind} options={manageKinds} />
    {:else}
    <div class="search">
      <Icon name="search" size={14} />
      <input class="input" placeholder={tab === "mod" ? "搜索 Mod，支持中文名…" : `搜索${tabs.find((t) => t.value === tab)?.label}，支持常见中文名…`} bind:value={query} />
    </div>
    <span class="sort">
      <Dropdown
        bind:value={sort}
        options={[
          { value: "relevance", label: "相关度" },
          { value: "downloads", label: "下载量" },
          { value: "updated", label: "最近更新" },
          { value: "newest", label: "最新发布" },
        ]}
      />
    </span>
    {#if tab !== "modpack"}
      <Toggle bind:checked={compatibleOnly} label="仅兼容" />
    {/if}
    {/if}
  </div>

  {#if tab === "installed"}
    <div class="installed">
      {#if instanceId}
        <ResourceManager instance={instanceId} kind={manageKind} />
      {:else}
        <div class="empty"><h3>请先选择版本</h3></div>
      {/if}
    </div>
  {:else}
  <div class="results">
    {#if error}
      <div class="empty">
        <h3>搜索失败</h3>
        <p>{error.message}</p>
        <p class="muted">{error.hint}</p>
        <button class="btn" style="margin-top:14px" onclick={() => search(true)}>重试</button>
      </div>
    {:else if loading && hits.length === 0}
      <!-- 第一次加载：与 PCL 一样显示「正在获取 Mod 列表」 -->
      <div class="fetching" in:fade={{ duration: 150 }}>
        <span class="fetch-icon"><Icon name="cube" size={30} /></span>
        <p>正在获取 {tabs.find((t) => t.value === tab)?.label ?? ""} 列表</p>
      </div>
    {:else if !loading && hits.length === 0}
      <div class="empty"><h3>没有找到结果</h3><p>换个关键词，或关闭「仅兼容」试试。</p></div>
    {:else}
      {#key listGen}
      <div class="result-list" class:stale={staleGen === listGen} out:fadeAway>
      <VirtualList items={hits} rowHeight={84} key={(h) => `${h.platform}:${h.id}`} onEnd={() => search(false)} appear>
        {#snippet row(h)}
          {@const zh = h.zhName ?? zhName(h.title, h.slug)}
          <button class="hit" onclick={() => openDetail(h)}>
            {#if h.iconUrl}
              <img src={iconSrc(h.iconUrl)} alt="" loading="lazy" class="icon" />
            {:else}
              <span class="icon blank"><Icon name="cube" size={20} /></span>
            {/if}
            <div class="main">
              <div class="title">
                <strong>{zh ?? h.title}</strong>
                {#if zh}<span class="en">{h.title}</span>{/if}
                {#if h.author}<span class="muted small">by {h.author}</span>{/if}
                {#if platform === "all"}<span class="src" class:cf={h.platform === "curseforge"}>{h.platform === "curseforge" ? "CF" : "MR"}</span>{/if}
              </div>
              <p class="desc" title={h.zhDescription ? h.description : undefined}>{h.zhDescription ?? h.description}</p>
              <div class="meta">
                <span class="mono small"><Icon name="download" size={11} /> {count(h.downloads)}</span>
                <span class="muted small">更新于 {date(h.updated)}</span>
                {#each h.categories.slice(0, 3) as c}<span class="tag">{c}</span>{/each}
              </div>
            </div>
          </button>
        {/snippet}
      </VirtualList>
      </div>
      {/key}
    {/if}
  </div>
  <div class="foot muted small">
    {#if converted}
      已按「{converted.zh}」搜索 {converted.keyword}
      {#if alternatives.length}
        · 也可能是：
        {#each alternatives as alt (alt.wikiId + alt.zh)}
          <button class="alt" onclick={() => (query = alt.zh)} title={alt.en}>{alt.zh}</button>
        {/each}
      {/if}
      ·
    {/if}
    {#if loading}加载中…{:else if total}共 {count(total)} 个结果，已显示 {hits.length} 个{/if}
  </div>
  {/if}
</div>
{/if}

{#if app.info?.os === "android"}
  <!-- 手机版：左右两栏，可按版本分类 -->
  <MobileContentDetail
    bind:open={detailOpen}
    project={current}
    instance={tab === "modpack" ? null : instanceId}
    {compatibleOnly}
    oninstall={onInstall}
  />
{:else}
  <ContentDetail
    bind:open={detailOpen}
    project={current}
    instance={tab === "modpack" ? null : instanceId}
    {compatibleOnly}
    oninstall={onInstall}
  />
{/if}
<ModpackInstall bind:open={packOpen} source={packSource} title={current?.title ?? ""} />
{#if current && instanceId}
  <InstallPlan
    bind:open={planOpen}
    platform={current.platform}
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
  .inst,
  .sort {
    display: inline-flex;
    flex: none;
  }
  .inst :global(.dropdown),
  .sort :global(.dropdown) {
    width: 100%;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }
  .spacer {
    flex: 1;
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
  .installed {
    flex: 1;
    min-height: 0;
  }
  .fetching {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--accent);
    font-weight: 600;
  }
  .fetch-icon {
    display: grid;
    animation: fetch-bob 900ms var(--ease) infinite alternate;
  }
  @keyframes fetch-bob {
    from {
      transform: translateY(-4px) rotate(-8deg);
    }
    to {
      transform: translateY(4px) rotate(8deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .fetch-icon {
      animation: none;
    }
  }
  .results {
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--rule-strong);
    position: relative;
  }
  /* 切换结果时新旧列表叠在同一位置交替淡入淡出，不影响布局 */
  .result-list {
    position: absolute;
    inset: 0;
    transition: opacity var(--t-mid) var(--ease);
  }
  .result-list.stale {
    opacity: 0.45;
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
  .title .en {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
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
  .alt {
    border: 0;
    background: none;
    padding: 0 2px;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .src {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.04em;
    padding: 1px 5px;
    border-radius: 4px;
    color: #1b7f4a;
    background: color-mix(in srgb, #1bd96a 16%, transparent);
  }
  .src.cf {
    color: #c2410c;
    background: color-mix(in srgb, #f16436 16%, transparent);
  }
</style>
