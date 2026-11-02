<script lang="ts">
  import { iconSrc, zhName } from "../lib/zh";
  // 项目详情：左栏是项目信息与版本分类（游戏版本 / 加载器 / 类型），右栏是筛选后的版本列表。
  // 有实例时只列出兼容该实例的版本，可一键安装最新的版本。
  import { api, type ContentVersion, type ProjectHit } from "../lib/api";
  import { count, date, LOADER_LABEL } from "../lib/format";
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
  /** 当前选择的分类，null 表示全部 */
  let game = $state<string | null>(null);
  let loader = $state<string | null>(null);
  let type = $state<string | null>(null);

  $effect(() => {
    if (!open || !project) return;
    const p = project;
    versions = null;
    error = null;
    game = loader = type = null;
    api
      .contentVersions(p.platform, p.id, p.contentType, compatibleOnly ? instance : null)
      .then((v) => (versions = v))
      .catch((e) => (error = `${e.message}${e.hint ? "\n" + e.hint : ""}`));
  });

  const typeLabel: Record<string, string> = { release: "正式", beta: "测试", alpha: "早期" };

  // 正式版号（如 1.21.10）按数值从新到旧；快照等其他版本号排在后面，保持出现顺序（版本列表本身按发布时间从新到旧）
  const releaseLike = (v: string) => /^\d+(\.\d+)*$/.test(v);
  function compareGame(a: string, b: string) {
    const ra = releaseLike(a);
    const rb = releaseLike(b);
    if (ra !== rb) return ra ? -1 : 1;
    if (!ra) return 0;
    const pa = a.split(".").map(Number);
    const pb = b.split(".").map(Number);
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
      const d = (pb[i] ?? -1) - (pa[i] ?? -1);
      if (d) return d;
    }
    return 0;
  }

  /** 统计每个分类下的版本数量，只列出实际出现过的分类 */
  function tally(list: ContentVersion[], pick: (v: ContentVersion) => string[]) {
    const counts = new Map<string, number>();
    for (const v of list) for (const k of new Set(pick(v))) counts.set(k, (counts.get(k) ?? 0) + 1);
    return counts;
  }

  // 各分类的选项按另外两个分类的当前选择计算数量，选项之间不会出现「选了之后没有结果」
  const matches = (v: ContentVersion, skip: "game" | "loader" | "type") =>
    (skip === "game" || game === null || v.gameVersions.includes(game)) &&
    (skip === "loader" || loader === null || v.loaders.includes(loader)) &&
    (skip === "type" || type === null || v.releaseType === type);

  const gameOptions = $derived.by(() => {
    const counts = tally(versions?.filter((v) => matches(v, "game")) ?? [], (v) => v.gameVersions);
    return [...counts].sort((a, b) => compareGame(a[0], b[0]));
  });
  const loaderOptions = $derived([...tally(versions?.filter((v) => matches(v, "loader")) ?? [], (v) => v.loaders)]);
  const typeOptions = $derived(
    ["release", "beta", "alpha"]
      .map((t) => [t, versions?.filter((v) => matches(v, "type") && v.releaseType === t).length ?? 0] as const)
      .filter(([, n]) => n > 0),
  );

  const shown = $derived(versions?.filter((v) => matches(v, "game") && matches(v, "loader") && matches(v, "type")) ?? []);
  const filtered = $derived(game !== null || loader !== null || type !== null);
  const best = $derived(shown.find((v) => v.releaseType === "release") ?? shown[0] ?? null);
</script>

<Modal
  bind:open
  title={project ? (project.zhName ?? zhName(project.title, project.slug) ?? project.title) : ""}
  kicker={project?.platform === "curseforge" ? "CURSEFORGE" : "MODRINTH"}
  width={1000}
>
  <div class="layout">
    <aside class="side">
      {#if project}
        <div class="info">
          {#if project.iconUrl}<img src={iconSrc(project.iconUrl)} alt="" class="icon" />{:else}<span class="icon blank"></span>{/if}
          <p class="desc">{project.zhDescription ?? project.description}</p>
          {#if project.zhDescription}<p class="muted small">{project.description}</p>{/if}
          <p class="muted small">
            {#if project.author}{project.author} · {/if}{count(project.downloads)} 次下载 · 更新于 {date(project.updated)}
          </p>
        </div>
      {/if}

      {#if versions && versions.length}
        {#if typeOptions.length > 1}
          <div class="group">
            <h4>类型</h4>
            <div class="chips">
              <button class="chip" class:on={type === null} onclick={() => (type = null)}>全部</button>
              {#each typeOptions as [t, n] (t)}
                <button class="chip" class:on={type === t} onclick={() => (type = t)}>{typeLabel[t]} <em>{n}</em></button>
              {/each}
            </div>
          </div>
        {/if}
        {#if loaderOptions.length > 1}
          <div class="group">
            <h4>加载器</h4>
            <div class="chips">
              <button class="chip" class:on={loader === null} onclick={() => (loader = null)}>全部</button>
              {#each loaderOptions as [l, n] (l)}
                <button class="chip" class:on={loader === l} onclick={() => (loader = l)}>
                  {LOADER_LABEL[l] ?? l} <em>{n}</em>
                </button>
              {/each}
            </div>
          </div>
        {/if}
        {#if gameOptions.length > 1}
          <div class="group games">
            <h4>游戏版本</h4>
            <div class="game-list">
              <button class="game" class:on={game === null} onclick={() => (game = null)}>
                <span>全部版本</span><em>{versions.filter((v) => matches(v, "game")).length}</em>
              </button>
              {#each gameOptions as [g, n] (g)}
                <button class="game" class:on={game === g} onclick={() => (game = g)}>
                  <span class="mono">{g}</span><em>{n}</em>
                </button>
              {/each}
            </div>
          </div>
        {/if}
      {/if}
    </aside>

    <section class="versions">
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
        <div class="list-head">
          <span>{filtered ? `符合条件的版本 ${shown.length} 个` : `全部版本 ${versions.length} 个`}</span>
          {#if filtered}
            <button class="btn ghost sm" onclick={() => (game = loader = type = null)}>清除筛选</button>
          {/if}
        </div>
        <div class="list">
          <VirtualList items={shown} rowHeight={60} key={(v) => v.id}>
            {#snippet row(v)}
              <div class="row">
                <div class="main">
                  <strong title={v.name || v.versionNumber}>{v.name || v.versionNumber}</strong>
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
    </section>
  </div>
  {#snippet actions()}
    <button class="btn" onclick={() => (open = false)}>关闭</button>
    <button class="btn primary" disabled={!best} onclick={() => best && oninstall(best)}>
      {filtered ? "安装所选分类的最新版本" : "安装最新兼容版本"}{best ? `（${best.versionNumber}）` : ""}
    </button>
  {/snippet}
</Modal>

<style>
  .layout {
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr);
    gap: 20px;
    height: min(62vh, 560px);
  }
  .side {
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding-right: 4px;
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .icon {
    width: 96px;
    height: 96px;
    border: 1.5px solid var(--ink);
    object-fit: cover;
    image-rendering: pixelated;
    margin-bottom: 6px;
  }
  .icon.blank {
    display: block;
    background: var(--paper-2);
  }
  .desc {
    font-family: var(--serif);
    font-size: 15px;
  }
  .small {
    font-size: 11px;
  }
  .group h4 {
    font-size: 12px;
    font-weight: 700;
    color: var(--ink-2);
    margin: 0 0 8px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rule);
    background: var(--card);
    color: var(--ink);
    font-size: 12px;
    cursor: pointer;
  }
  .chip em,
  .game em {
    font-style: normal;
    color: var(--muted);
    font-size: 11px;
  }
  .chip.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--card));
    color: var(--accent);
    font-weight: 600;
  }
  .games {
    min-height: 0;
  }
  .game-list {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--rule);
  }
  .game {
    display: flex;
    justify-content: space-between;
    align-items: center;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-bottom: 1px solid var(--rule);
    border-left: 3px solid transparent;
    background: none;
    color: var(--ink);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .game.on {
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--accent);
    font-weight: 600;
  }
  .versions {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 30px;
    font-size: 12px;
    color: var(--ink-2);
  }
  .list {
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--rule-strong);
  }
  .row {
    height: 60px;
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
    gap: 2px;
  }
  .main strong,
  .main span {
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
  /* 窄窗口：上下排列，分类在上 */
  @media (max-width: 700px) {
    .layout {
      grid-template-columns: 1fr;
      height: auto;
    }
    .side {
      overflow: visible;
    }
    .game-list {
      max-height: 160px;
      overflow-y: auto;
    }
    .versions {
      height: 50vh;
    }
  }
</style>
