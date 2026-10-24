<script lang="ts">
  import { api, type VersionList } from "../lib/api";
  import {
    app,
    refreshInstances,
    selectInstance,
    selectedAccount,
    selectedInstance,
    showError,
    waitTask,
  } from "../lib/state.svelte";
  import { bytes, dateline, KIND_LABEL, LOADER_LABEL, relative } from "../lib/format";
  import Avatar from "../components/Avatar.svelte";
  import Icon from "../components/Icon.svelte";
  import ProgressBar from "../components/ProgressBar.svelte";

  const inst = $derived(selectedInstance());
  const account = $derived(selectedAccount());
  const game = $derived(inst ? app.games[inst.id] : undefined);
  let launchTask = $state<number | null>(null);
  const task = $derived(launchTask ? app.tasks.find((t) => t.id === launchTask) : undefined);
  const launching = $derived(!!task && task.state === "running");
  let news = $state<VersionList | null>(null);

  $effect(() => {
    api
      .versionList(false)
      .then((v) => (news = v))
      .catch(() => {});
  });

  async function launch() {
    if (!inst) return;
    if (!account) {
      app.page = "accounts";
      return;
    }
    try {
      launchTask = await api.launch(inst.id);
      const t = await waitTask(launchTask);
      if (t.state === "done") refreshInstances();
    } catch (e) {
      showError(e, "无法启动");
    }
  }

  function progressOf() {
    const p = task?.progress;
    if (!p) return 0;
    return p.totalBytes ? p.doneBytes / p.totalBytes : p.totalFiles ? p.doneFiles / p.totalFiles : 0;
  }

  // 报头期号：今年的第几天
  const issue = Math.floor((Date.now() - new Date(new Date().getFullYear(), 0, 0).getTime()) / 86400000);
  const accountLabel = $derived(
    account?.kind.type === "offline" ? "离线账号" : account?.kind.type === "microsoft" ? "微软账号" : "外置登录",
  );
</script>

<div class="front">
  <header class="masthead">
    <div class="meta">
      <span>第 {issue} 期</span>
      <span>{dateline()}</span>
      <span>Minecraft Java 版启动器</span>
    </div>
    <hr class="rule-double" />
    <h1 class="title">Red Craft</h1>
    <hr class="rule-double" />
  </header>

  <div class="columns">
    <section class="lead">
      {#if inst}
        <div class="kicker">头条 · 当前实例</div>
        <h2 class="headline">{inst.id}</h2>
        <p class="deck">
          Minecraft {inst.mcVersion}
          · {KIND_LABEL[inst.kind] ?? inst.kind}
          {#if inst.loader}· {LOADER_LABEL[inst.loader.kind]} {inst.loader.version}{/if}
          · {inst.isolated ? "版本隔离" : "共用游戏目录"}
        </p>
        <p class="byline">上次游玩：{relative(inst.lastPlayed)}</p>
        <div class="quick">
          <button class="btn sm" onclick={() => api.openInstanceDir(inst.id).catch((e) => showError(e))}>
            <Icon name="folder" size={14} /> 游戏目录
          </button>
          <button class="btn sm" onclick={() => api.openInstanceDir(inst.id, "saves").catch((e) => showError(e))}>
            存档
          </button>
          {#if game}
            <button class="btn sm" onclick={() => (app.logViewer = inst.id)}>
              <Icon name="terminal" size={14} /> 日志
            </button>
          {/if}
          <button class="btn sm ghost" onclick={() => (app.page = "instances")}>管理实例 <Icon name="chevron" size={12} /></button>
        </div>
        {#if app.instances.length > 1}
          <div class="others">
            <span class="kicker">其它实例</span>
            <div class="chips">
              {#each app.instances.filter((i) => i.id !== inst.id).slice(0, 6) as other}
                <button class="chip" onclick={() => selectInstance(other.id)}>{other.id}</button>
              {/each}
            </div>
          </div>
        {/if}
      {:else if app.instancesLoaded}
        <div class="kicker">号外</div>
        <h2 class="headline">还没有游戏实例</h2>
        <p class="deck">先到「下载」页面挑一个版本安装，几分钟后就能开始游戏。</p>
        <div class="quick">
          <button class="btn primary" onclick={() => (app.page = "download")}><Icon name="download" size={14} /> 去下载</button>
        </div>
      {/if}
    </section>

    <aside class="brief">
      <div class="kicker">简讯</div>
      {#if news}
        <article>
          <h4>最新正式版 {news.latestRelease}</h4>
          <p class="muted">已发布，可在「下载」页面安装。</p>
        </article>
        <article>
          <h4>最新快照 {news.latestSnapshot}</h4>
          <p class="muted">抢先体验下一个版本的新内容。</p>
        </article>
      {:else}
        <article><p class="muted">正在获取版本消息…</p></article>
      {/if}
      {#if app.info}
        <article>
          <h4>本机内存</h4>
          <p class="muted mono">
            {bytes(app.info.memory.totalMb * 1024 * 1024)} 总计{#if app.info.memory.availableMb}
              · {bytes(app.info.memory.availableMb * 1024 * 1024)} 可用{/if}
          </p>
        </article>
      {/if}
    </aside>
  </div>

  <footer class="launchbar">
    <button class="account" onclick={() => (app.page = "accounts")}>
      {#if account}
        <Avatar uuid={account.uuid} size={36} />
        <div>
          <strong>{account.name}</strong>
          <span class="muted">{accountLabel}</span>
        </div>
      {:else}
        <span class="avatar-empty"><Icon name="user" size={18} /></span>
        <div>
          <strong>未登录</strong>
          <span class="muted">点击添加账号</span>
        </div>
      {/if}
    </button>

    <div class="status">
      {#if launching && task}
        <div class="stage">{task.stage}…</div>
        <ProgressBar value={progressOf()} indeterminate={!task.progress} />
      {:else if game?.running}
        <div class="stage">
          {game.started ? "游戏运行中" : "游戏正在加载…"} <span class="muted mono">PID {game.pid}</span>
        </div>
      {/if}
    </div>

    {#if game?.running}
      <button class="btn lg-btn" onclick={() => inst && api.killGame(inst.id).catch((e) => showError(e))}>
        <Icon name="stop" size={18} /> 结束游戏
      </button>
    {:else}
      <button class="btn primary lg-btn" disabled={!inst || launching} onclick={launch}>
        <Icon name="play" size={20} />
        {launching ? "正在启动" : "启动游戏"}
      </button>
    {/if}
  </footer>
</div>

<style>
  .front {
    min-height: 100%;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    letter-spacing: 0.12em;
    color: var(--muted);
    padding-bottom: 6px;
  }
  .title {
    font-size: 64px;
    line-height: 1;
    text-align: center;
    letter-spacing: 0.06em;
    padding: 10px 0 8px;
    font-weight: 900;
  }
  .columns {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(220px, 1fr);
    gap: 28px;
  }
  .lead {
    min-width: 0;
  }
  .headline {
    font-size: 52px;
    line-height: 1.05;
    margin: 6px 0 12px;
    word-break: break-word;
  }
  .deck {
    font-family: var(--serif);
    font-size: 17px;
    color: var(--ink-2);
  }
  .byline {
    margin-top: 6px;
    font-size: 12px;
    color: var(--muted);
  }
  .quick {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 18px;
  }
  .others {
    margin-top: 26px;
    padding-top: 12px;
    border-top: 1px solid var(--rule);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }
  .chip {
    border: 1px solid var(--rule);
    background: transparent;
    padding: 3px 10px;
    font-size: 12px;
    cursor: pointer;
    transition: transform var(--t-fast) var(--ease);
  }
  .chip:hover {
    border-color: var(--ink);
    transform: translateY(-1px);
  }
  .chip:active {
    transform: scale(0.96);
  }
  .brief {
    border-left: 1px solid var(--rule);
    padding-left: 22px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .brief article {
    padding-bottom: 12px;
    border-bottom: 1px solid var(--rule);
  }
  .brief h4 {
    font-size: 16px;
    margin-bottom: 2px;
  }
  .brief p {
    font-size: 12px;
  }
  .launchbar {
    display: flex;
    align-items: center;
    gap: 18px;
    padding-top: 14px;
    border-top: 2px solid var(--rule-strong);
  }
  .account {
    display: flex;
    align-items: center;
    gap: 10px;
    border: 0;
    background: transparent;
    cursor: pointer;
    text-align: left;
    padding: 4px 8px 4px 0;
    min-width: 170px;
  }
  .account strong {
    display: block;
    font-size: 14px;
  }
  .account span {
    font-size: 11px;
  }
  .avatar-empty {
    width: 36px;
    height: 36px;
    border: 1.5px dashed var(--muted);
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .status {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .stage {
    font-size: 12px;
    color: var(--ink-2);
  }
  .lg-btn {
    height: 52px;
    min-width: 200px;
    font-size: 17px;
    font-family: var(--serif);
    letter-spacing: 0.1em;
  }
</style>
