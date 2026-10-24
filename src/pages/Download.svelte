<script lang="ts">
  import { api, type VersionItem, type VersionList } from "../lib/api";
  import { app, refreshInstances, selectInstance, showError, waitTask } from "../lib/state.svelte";
  import { date, KIND_LABEL } from "../lib/format";
  import VirtualList from "../components/VirtualList.svelte";
  import Segmented from "../components/Segmented.svelte";
  import Modal from "../components/Modal.svelte";
  import Icon from "../components/Icon.svelte";

  type Filter = "release" | "snapshot" | "old";
  let filter = $state<Filter>((localStorage.getItem("rc.versionFilter") as Filter) || "release");
  let query = $state("");
  let list = $state<VersionList | null>(null);
  let loading = $state(false);
  let error = $state<{ message: string; hint: string } | null>(null);

  async function load(refresh = false) {
    loading = true;
    error = null;
    try {
      list = await api.versionList(refresh);
    } catch (e) {
      const err = e as { message: string; hint: string };
      error = { message: err.message, hint: err.hint };
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load(false);
  });

  $effect(() => {
    localStorage.setItem("rc.versionFilter", filter);
  });

  const shown = $derived.by(() => {
    if (!list) return [];
    const q = query.trim().toLowerCase();
    return list.versions.filter((v) => {
      const kindOk =
        filter === "release" ? v.kind === "release" : filter === "snapshot" ? v.kind === "snapshot" : v.kind.startsWith("old_");
      return kindOk && (!q || v.id.toLowerCase().includes(q));
    });
  });

  const installed = $derived(new Set(app.instances.map((i) => i.mcVersion)));

  // 安装弹窗
  let dialog = $state(false);
  let target = $state<VersionItem | null>(null);
  let name = $state("");
  let installing = $state(false);

  function openInstall(v: VersionItem) {
    target = v;
    let n = v.id;
    let i = 2;
    const ids = new Set(app.instances.map((x) => x.id));
    while (ids.has(n)) n = `${v.id}-${i++}`;
    name = n;
    dialog = true;
  }

  async function install() {
    if (!target) return;
    installing = true;
    try {
      const id = await api.installVanilla(target.id, name);
      dialog = false;
      const t = await waitTask(id);
      if (t.state === "done") {
        await refreshInstances();
        await selectInstance(name);
      }
    } catch (e) {
      showError(e, "无法安装");
    } finally {
      installing = false;
    }
  }

  const nameExists = $derived(app.instances.some((i) => i.id === name.trim()));
</script>

<div class="wrap">
  <div class="page-head">
    <div>
      <div class="kicker">DOWNLOAD</div>
      <h1>下载游戏</h1>
      <div class="sub">
        {#if list}最新正式版 {list.latestRelease} · 最新快照 {list.latestSnapshot}{:else}版本清单来自 Mojang 官方{/if}
      </div>
    </div>
    <div class="tools">
      <input class="input search" placeholder="搜索版本号…" bind:value={query} />
      <Segmented
        bind:value={filter}
        options={[
          { value: "release", label: "正式版" },
          { value: "snapshot", label: "快照" },
          { value: "old", label: "远古版" },
        ]}
      />
      <button class="btn icon" title="刷新" onclick={() => load(true)} disabled={loading}>
        <span class:spin={loading}><Icon name="refresh" size={16} /></span>
      </button>
    </div>
  </div>

  <div class="table">
    <div class="thead">
      <span>版本</span><span>类型</span><span>发布日期</span><span></span>
    </div>
    <div class="tbody">
      {#if error}
        <div class="empty">
          <h3>获取版本列表失败</h3>
          <p>{error.message}</p>
          <p class="hint">{error.hint}</p>
          <button class="btn" style="margin-top:14px" onclick={() => load(true)}>重试</button>
        </div>
      {:else if !list}
        <div class="empty"><p>正在加载版本列表…</p></div>
      {:else if shown.length === 0}
        <div class="empty"><h3>没有匹配的版本</h3><p>换个关键词或分类试试。</p></div>
      {:else}
        <VirtualList items={shown} rowHeight={48} key={(v) => v.id}>
          {#snippet row(v)}
            <div class="tr">
              <span class="vid">
                {v.id}
                {#if v.id === list?.latestRelease || v.id === list?.latestSnapshot}<span class="tag accent">最新</span>{/if}
                {#if installed.has(v.id)}<span class="tag">已安装</span>{/if}
              </span>
              <span class="muted">{KIND_LABEL[v.kind] ?? v.kind}</span>
              <span class="mono muted">{date(v.releaseTime)}</span>
              <span class="act"><button class="btn sm" onclick={() => openInstall(v)}><Icon name="download" size={12} /> 安装</button></span>
            </div>
          {/snippet}
        </VirtualList>
      {/if}
    </div>
  </div>
</div>

<Modal bind:open={dialog} title={`安装 ${target?.id ?? ""}`} kicker="NEW INSTANCE">
  <div class="form">
    <div class="field">
      <label for="inst-name">实例名称</label>
      <input id="inst-name" class="input" bind:value={name} maxlength="64" />
      {#if nameExists}
        <span class="help" style="color:var(--danger)">已存在同名实例，请换一个名称。</span>
      {:else}
        <span class="help">实例会保存在 versions/{name || "…"} 目录中。</span>
      {/if}
    </div>
    <div class="field">
      <span class="label">模组加载器</span>
      <div class="loaders">
        <span class="tag accent">原版</span>
        <span class="help">Fabric、Quilt、Forge、NeoForge 的安装将在后续版本加入。</span>
      </div>
    </div>
  </div>
  {#snippet actions()}
    <button class="btn" onclick={() => (dialog = false)}>取消</button>
    <button class="btn primary" disabled={installing || !name.trim() || nameExists} onclick={install}>开始安装</button>
  {/snippet}
</Modal>

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
  .search {
    width: 200px;
  }
  .table {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--rule);
  }
  .thead,
  .tr {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 120px 130px 100px;
    align-items: center;
    padding: 0 6px;
  }
  .thead {
    height: 32px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--muted);
    border-bottom: 1px solid var(--rule-strong);
  }
  .tbody {
    flex: 1;
    min-height: 0;
  }
  .tr {
    height: 48px;
    border-bottom: 1px solid var(--rule);
    transition: background-color var(--t-fast);
  }
  .tr:hover {
    background: var(--accent-soft);
  }
  .vid {
    font-family: var(--serif);
    font-weight: 700;
    font-size: 16px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .act {
    text-align: right;
  }
  .hint {
    margin-top: 6px;
    font-size: 12px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .loaders {
    display: flex;
    align-items: center;
    gap: 10px;
  }
</style>
