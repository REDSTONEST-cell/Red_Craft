<script lang="ts">
  // 手机版主页：左侧是当前版本与快捷操作，右侧固定账号、版本选择与「启动游戏」（参考 FCL 等手机启动器的横屏布局）
  import { api, type FileKind } from "../../lib/api";
  import { app, refreshInstances, selectInstance, selectedAccount, selectedInstance, showError, waitTask } from "../../lib/state.svelte";
  import { KIND_LABEL, LOADER_LABEL, relative } from "../../lib/format";
  import Avatar from "../../components/Avatar.svelte";
  import Icon from "../../components/Icon.svelte";
  import ProgressBar from "../../components/ProgressBar.svelte";
  import { fade, fly } from "svelte/transition";
  import { dur } from "../../lib/motion";

  const inst = $derived(selectedInstance());
  const account = $derived(selectedAccount());
  const game = $derived(inst ? app.games[inst.id] : undefined);
  let launchTask = $state<number | null>(null);
  const task = $derived(launchTask ? app.tasks.find((t) => t.id === launchTask) : undefined);
  const launching = $derived(!!task && task.state === "running");
  let picking = $state(false);

  const accountLabel = $derived.by(() => {
    const k = account?.kind;
    if (k?.type === "offline") return "离线账号";
    if (k?.type === "microsoft") return "微软账号";
    return k?.type === "yggdrasil" && k.serverName ? `外置 · ${k.serverName}` : "外置登录";
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

  function manage(kind: FileKind) {
    app.manage = kind;
    app.page = "mods";
  }

  async function pick(id: string) {
    picking = false;
    await selectInstance(id);
  }

  const actions = $derived([
    { label: "资源管理", icon: "cube", run: () => manage("mod") },
    { label: "存档", icon: "box", run: () => manage("save") },
    { label: "下载资源", icon: "download", run: () => (app.page = "mods") },
    { label: "全部版本", icon: "chevron", run: () => (app.page = "instances") },
  ]);
</script>

<div class="home">
  <section class="hero">
    {#if inst}
      <span class="eyebrow">当前版本</span>
      <h1 class="name">{inst.id}</h1>
      <p class="meta">
        Minecraft {inst.mcVersion}
        · {KIND_LABEL[inst.kind] ?? inst.kind}
        {#if inst.loader}· {LOADER_LABEL[inst.loader.kind]} {inst.loader.version}{/if}
        {#if inst.optifine}· OptiFine{/if}
      </p>
      <p class="played">上次游玩：{relative(inst.lastPlayed)}</p>
      <div class="tiles">
        {#each actions as a}
          <button class="tile" onclick={a.run}>
            <Icon name={a.icon} size={18} />
            <span>{a.label}</span>
          </button>
        {/each}
      </div>
    {:else if app.instancesLoaded}
      <span class="eyebrow">欢迎使用 RCL</span>
      <h1 class="name">还没有安装游戏版本</h1>
      <p class="meta">先安装一个 Minecraft 版本，几分钟后就能开始游戏。</p>
      <div class="tiles">
        <button class="tile primary" onclick={() => (app.page = "download")}>
          <Icon name="download" size={18} />
          <span>安装版本</span>
        </button>
      </div>
    {/if}
  </section>

  <aside class="panel">
    <button class="account" onclick={() => (app.page = "accounts")}>
      {#if account}
        <Avatar uuid={account.uuid} skin={account.skin} size={38} />
        <span class="who">
          <strong>{account.name}</strong>
          <small>{accountLabel}</small>
        </span>
      {:else}
        <span class="avatar-empty"><Icon name="user" size={18} /></span>
        <span class="who">
          <strong>未登录</strong>
          <small>点击添加账号</small>
        </span>
      {/if}
      <Icon name="chevron" size={14} />
    </button>

    <button class="version" disabled={!app.instances.length} onclick={() => (picking = true)}>
      <span class="who">
        <small>游戏版本</small>
        <strong>{inst?.id ?? "未安装"}</strong>
      </span>
      <Icon name="chevron" size={14} />
    </button>

    <div class="status">
      {#if launching && task}
        <span>{task.stage}…</span>
        <ProgressBar value={progressOf()} indeterminate={!task.progress} />
      {:else if game?.running}
        <span>{game.started ? "游戏运行中" : "游戏正在加载…"}</span>
      {/if}
    </div>

    {#if game?.running}
      <button class="launch stop" onclick={() => inst && api.killGame(inst.id).catch((e) => showError(e))}>
        <Icon name="stop" size={18} /> 结束游戏
      </button>
    {:else}
      <button class="launch" disabled={!inst || launching} onclick={launch}>
        <Icon name="play" size={20} />
        {launching ? "正在启动" : "启动游戏"}
      </button>
    {/if}
  </aside>
</div>

{#if picking}
  <div class="scrim" role="presentation" onclick={() => (picking = false)} transition:fade={{ duration: dur(150) }}></div>
  <div class="sheet" transition:fly={{ x: 40, duration: dur(200) }}>
    <header>
      <strong>选择版本</strong>
      <button class="btn ghost icon sm" onclick={() => (picking = false)} title="关闭"><Icon name="x" size={14} /></button>
    </header>
    <div class="list">
      {#each app.instances as i (i.id)}
        <button class="row" class:active={i.id === inst?.id} onclick={() => pick(i.id)}>
          <span class="badge" data-loader={i.loader?.kind ?? (i.optifine ? "optifine" : "vanilla")}>
            {i.loader ? LOADER_LABEL[i.loader.kind][0] : i.optifine ? "O" : "V"}
          </span>
          <span class="who">
            <strong>{i.id}</strong>
            <small>Minecraft {i.mcVersion}{i.loader ? ` · ${LOADER_LABEL[i.loader.kind]}` : ""}</small>
          </span>
          {#if i.id === inst?.id}<Icon name="check" size={16} />{/if}
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .home {
    height: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 236px;
    gap: 12px;
    padding: 12px;
  }
  .hero {
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 4px 8px;
  }
  .eyebrow {
    font-size: 11px;
    font-weight: 700;
    color: var(--accent);
    letter-spacing: 0.04em;
  }
  .name {
    margin: 4px 0 4px;
    font-size: 26px;
    font-weight: 800;
    line-height: 1.15;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .meta {
    font-size: 13px;
    color: var(--ink-2);
  }
  .played {
    font-size: 11px;
    color: var(--muted);
    margin-top: 2px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
    margin-top: 18px;
    max-width: 440px;
  }
  .tile {
    height: 62px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--card);
    color: var(--ink);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 5px;
    font-size: 11.5px;
    font-weight: 600;
    box-shadow: var(--shadow-sm);
    transition: transform var(--t-fast);
  }
  .tile :global(svg) {
    color: var(--accent);
  }
  .tile:active {
    transform: scale(0.96);
  }
  .tile.primary {
    background: var(--accent);
    color: var(--accent-ink);
    border-color: transparent;
    grid-column: span 2;
  }
  .tile.primary :global(svg) {
    color: inherit;
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    box-shadow: var(--shadow);
    min-height: 0;
  }
  .account,
  .version {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--paper);
    color: var(--ink);
    text-align: left;
    min-width: 0;
  }
  .account:active,
  .version:active {
    background: color-mix(in srgb, var(--ink) 6%, var(--paper));
  }
  .version:disabled {
    opacity: 0.6;
  }
  .account > :global(svg:last-child),
  .version > :global(svg) {
    color: var(--muted);
    flex: none;
  }
  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .who strong {
    font-size: 13.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .who small {
    font-size: 11px;
    color: var(--muted);
  }
  .avatar-empty {
    width: 38px;
    height: 38px;
    border-radius: 10px;
    border: 1.5px dashed var(--muted);
    display: grid;
    place-items: center;
    color: var(--muted);
    flex: none;
  }
  .status {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 6px;
    font-size: 11px;
    color: var(--muted);
    min-height: 0;
  }
  .launch {
    height: 52px;
    border: 0;
    border-radius: 14px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 16px;
    font-weight: 800;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    box-shadow: 0 6px 18px color-mix(in srgb, var(--accent) 40%, transparent);
    transition: transform var(--t-fast), opacity var(--t-fast);
  }
  .launch:active {
    transform: scale(0.97);
  }
  .launch:disabled {
    opacity: 0.5;
    box-shadow: none;
  }
  .launch.stop {
    background: var(--ink);
    color: var(--paper);
    box-shadow: none;
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(0, 0, 0, 0.35);
  }
  .sheet {
    position: fixed;
    top: 8px;
    right: 8px;
    bottom: 8px;
    z-index: 41;
    width: 300px;
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    background: var(--card);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .sheet header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 10px 8px 14px;
    border-bottom: 1px solid var(--line);
    font-size: 14px;
  }
  .sheet .list {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px;
    border: 0;
    border-radius: 10px;
    background: none;
    color: var(--ink);
    text-align: left;
  }
  .row.active {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .row.active > :global(svg) {
    color: var(--accent);
  }
  .badge {
    width: 32px;
    height: 32px;
    border-radius: 9px;
    display: grid;
    place-items: center;
    font-weight: 800;
    font-size: 13px;
    flex: none;
    background: var(--paper-2);
    color: var(--ink);
  }
  .badge[data-loader="fabric"] { background: #dbd0b4; color: #3b3424; }
  .badge[data-loader="forge"] { background: #3d4555; color: #fff; }
  .badge[data-loader="neoforge"] { background: #d9822b; color: #fff; }
  .badge[data-loader="quilt"] { background: #8a5cd6; color: #fff; }
  .badge[data-loader="optifine"] { background: #b52e31; color: #fff; }
</style>
