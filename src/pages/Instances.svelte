<script lang="ts">
  import { api, type InstanceConfig, type InstanceInfo } from "../lib/api";
  import { app, refreshInstances, selectInstance, selectedInstance, showError, toast } from "../lib/state.svelte";
  import { KIND_LABEL, LOADER_LABEL, relative } from "../lib/format";
  import Modal from "../components/Modal.svelte";
  import Icon from "../components/Icon.svelte";
  import Segmented from "../components/Segmented.svelte";
  import ModpackInstall from "../components/ModpackInstall.svelte";
  import ImportLauncher from "../components/ImportLauncher.svelte";
  import ExportModpack from "../components/ExportModpack.svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { page } from "../lib/motion";
  import type { ModpackInfo, PackSource } from "../lib/api";

  let packOpen = $state(false);
  let exportOpen = $state(false);
  let exporting = $state<InstanceInfo | null>(null);
  let migrateOpen = $state(false);
  let packSource = $state<PackSource | null>(null);
  let packInfo = $state<ModpackInfo | null>(null);

  async function importPack() {
    const file = await openDialog({
      title: "选择整合包",
      multiple: false,
      directory: false,
      filters: [{ name: "整合包", extensions: ["mrpack", "zip"] }],
    });
    if (typeof file !== "string") return;
    try {
      packInfo = await api.readModpackFile(file);
      packSource = { type: "file", path: file };
      packOpen = true;
    } catch (e) {
      showError(e, "无法读取整合包");
    }
  }

  $effect(() => {
    refreshInstances();
  });

  const current = $derived(selectedInstance());

  let confirmDelete = $state<InstanceInfo | null>(null);
  let deleteOpen = $state(false);

  async function doDelete() {
    if (!confirmDelete) return;
    const id = confirmDelete.id;
    try {
      await api.deleteInstance(id);
      toast("ok", `已删除实例 ${id}`);
      deleteOpen = false;
      await refreshInstances();
    } catch (e) {
      showError(e, "删除失败");
    }
  }

  // 批量管理：多选实例后统一补全文件、设置版本隔离或删除
  let selecting = $state(false);
  let picked = $state<Record<string, boolean>>({});
  let batchBusy = $state(false);
  let batchDeleteOpen = $state(false);
  const pickedList = $derived(app.instances.filter((i) => picked[i.id]));
  const allPicked = $derived(app.instances.length > 0 && pickedList.length === app.instances.length);

  function toggleSelecting() {
    selecting = !selecting;
    picked = {};
  }

  function pickAll() {
    picked = allPicked ? {} : Object.fromEntries(app.instances.map((i) => [i.id, true]));
  }

  // 列表变化后（例如删除）去掉已不存在的实例
  $effect(() => {
    const ids = new Set(app.instances.map((i) => i.id));
    for (const id of Object.keys(picked)) if (!ids.has(id)) delete picked[id];
  });

  async function batchRepair() {
    const list = pickedList.filter((i) => !i.error);
    let started = 0;
    for (const i of list) {
      try {
        await api.repairInstance(i.id);
        started++;
      } catch (e) {
        showError(e, `补全 ${i.id} 失败`);
      }
    }
    if (started) toast("ok", `已开始补全 ${started} 个实例`, "进度见右下角的任务按钮");
  }

  async function batchIsolation(value: "default" | "on" | "off") {
    batchBusy = true;
    try {
      for (const i of pickedList) {
        const config = await api.getInstanceConfig(i.id);
        await api.saveInstanceConfig(i.id, { ...config, isolation: value === "default" ? null : value === "on" });
      }
      toast("ok", `已更新 ${pickedList.length} 个实例的版本隔离`);
      await refreshInstances();
    } catch (e) {
      showError(e, "批量设置失败");
    } finally {
      batchBusy = false;
    }
  }

  const pickedRunning = $derived(pickedList.filter((i) => i.running));

  async function batchDelete() {
    batchBusy = true;
    let done = 0;
    // 逐个删除：共用的原版父版本会在最后一个继承它的实例删除时一并清理
    for (const i of pickedList.filter((i) => !i.running)) {
      try {
        await api.deleteInstance(i.id);
        done++;
      } catch (e) {
        showError(e, `删除 ${i.id} 失败`);
      }
    }
    batchBusy = false;
    batchDeleteOpen = false;
    if (done) toast("ok", `已删除 ${done} 个实例`);
    await refreshInstances();
    if (!app.instances.length) selecting = false;
  }

  async function launch(i: InstanceInfo) {
    await selectInstance(i.id);
    app.page = "launch";
  }

  // 实例设置
  let editing = $state<InstanceInfo | null>(null);
  let editOpen = $state(false);
  let config = $state<InstanceConfig | null>(null);
  let isolation = $state<"default" | "on" | "off">("default");
  let memory = $state("");

  async function openSettings(i: InstanceInfo) {
    editing = i;
    try {
      config = await api.getInstanceConfig(i.id);
      isolation = config.isolation === null ? "default" : config.isolation ? "on" : "off";
      memory = config.maxMemoryMb ? String(config.maxMemoryMb) : "";
      editOpen = true;
    } catch (e) {
      showError(e);
    }
  }

  async function saveConfig() {
    if (!editing || !config) return;
    const mem = parseInt(memory, 10);
    const next: InstanceConfig = {
      ...config,
      isolation: isolation === "default" ? null : isolation === "on",
      maxMemoryMb: Number.isFinite(mem) && mem > 0 ? mem : null,
      jvmArgs: config.jvmArgs?.trim() ? config.jvmArgs : null,
      javaPath: config.javaPath?.trim() ? config.javaPath : null,
      server: config.server?.trim() ? config.server : null,
    };
    try {
      await api.saveInstanceConfig(editing.id, next);
      editOpen = false;
      toast("ok", "实例设置已保存");
      refreshInstances();
    } catch (e) {
      showError(e, "保存失败");
    }
  }
</script>

<div class="page-head">
  <div>
    <div class="kicker">INSTANCES</div>
    <h1>游戏实例</h1>
    <div class="sub" title={app.info?.gameDir}>共 {app.instances.length} 个 · 游戏目录 <span class="mono">{app.info?.gameDir}</span></div>
  </div>
  <div class="tools">
    <button class="btn icon" title="打开游戏目录" aria-label="打开游戏目录" onclick={() => api.openGameDir().catch((e) => showError(e))}><Icon name="folder" size={14} /></button>
    {#if app.instances.length > 1 || selecting}
      <button class="btn" class:active={selecting} aria-pressed={selecting} onclick={toggleSelecting}>
        <Icon name={selecting ? "x" : "check"} size={14} /> {selecting ? "退出批量管理" : "批量管理"}
      </button>
    {/if}
    <button class="btn" onclick={importPack}><Icon name="box" size={14} /> 导入整合包</button>
    <button class="btn" onclick={() => (migrateOpen = true)}><Icon name="link" size={14} /> 从其他启动器导入</button>
    <button class="btn primary" onclick={() => (app.page = "download")}><Icon name="plus" size={14} /> 新建实例</button>
  </div>
</div>

{#if app.instancesLoaded && app.instances.length === 0}
  <div class="empty">
    <h3>这里还空着</h3>
    <p>在「下载」页面安装一个版本，或者从其他启动器导入已有的实例。</p>
    <button class="btn" style="margin-top:14px" onclick={() => (migrateOpen = true)}>从其他启动器导入</button>
  </div>
{:else}
  {#if selecting}
    <div class="batch-bar" transition:page={{ y: -6 }}>
      <label class="check">
        <input type="checkbox" checked={allPicked} indeterminate={pickedList.length > 0 && !allPicked} onchange={pickAll} />
        全选
      </label>
      <span class="muted">已选 <strong class="nums">{pickedList.length}</strong> / {app.instances.length}</span>
      <span class="spacer"></span>
      <button class="btn sm" disabled={!pickedList.length || batchBusy} onclick={batchRepair}><Icon name="refresh" size={12} /> 补全文件</button>
      <select
        class="input sm-select"
        disabled={!pickedList.length || batchBusy}
        value=""
        onchange={(e) => {
          const v = e.currentTarget.value as "default" | "on" | "off";
          e.currentTarget.value = "";
          if (v) batchIsolation(v);
        }}
      >
        <option value="" disabled>版本隔离…</option>
        <option value="on">开启隔离</option>
        <option value="off">关闭隔离</option>
        <option value="default">跟随全局</option>
      </select>
      <button class="btn sm danger" disabled={!pickedList.length || batchBusy} onclick={() => (batchDeleteOpen = true)}>
        <Icon name="trash" size={12} /> 删除
      </button>
    </div>
  {/if}
  <div class="list stagger" class:selecting>
    {#each app.instances as inst, i (inst.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <article
        class="row"
        class:current={current?.id === inst.id}
        style:--i={i}
        onclick={() => selecting && (picked[inst.id] = !picked[inst.id])}
      >
        {#if selecting}
          <input
            type="checkbox"
            class="pick"
            aria-label={`选择 ${inst.id}`}
            checked={!!picked[inst.id]}
            onclick={(e) => e.stopPropagation()}
            onchange={(e) => (picked[inst.id] = e.currentTarget.checked)}
          />
        {/if}
        <div class="badge" data-loader={inst.loader?.kind ?? (inst.optifine ? "optifine" : "vanilla")}>
          <span>{inst.loader ? LOADER_LABEL[inst.loader.kind][0] : inst.optifine ? "O" : "V"}</span>
        </div>
        <div class="main">
          <h3>
            {inst.id}
            {#if current?.id === inst.id}<span class="tag accent">当前</span>{/if}
            {#if inst.running}<span class="tag accent">运行中</span>{/if}
          </h3>
          {#if inst.error}
            <p class="err">{inst.error}</p>
          {:else}
            <p class="muted">
              Minecraft {inst.mcVersion} · {KIND_LABEL[inst.kind] ?? inst.kind}
              {#if inst.loader}· {LOADER_LABEL[inst.loader.kind]} {inst.loader.version}{/if}
              {#if inst.optifine}· OptiFine {inst.optifine.replaceAll("_", " ")}{/if}
              · {inst.isolated ? "版本隔离" : "共用目录"} · {relative(inst.lastPlayed)}
            </p>
          {/if}
        </div>
        <div class="actions" class:hidden={selecting}>
          <button class="btn sm primary" disabled={!!inst.error} onclick={() => launch(inst)}><Icon name="play" size={12} /> 启动</button>
          <button class="btn sm icon" title="打开文件夹" onclick={() => api.openInstanceDir(inst.id).catch((e) => showError(e))}>
            <Icon name="folder" size={14} />
          </button>
          <button class="btn sm icon" title="补全文件" onclick={() => api.repairInstance(inst.id).catch((e) => showError(e))}>
            <Icon name="refresh" size={14} />
          </button>
          <button
            class="btn sm icon"
            title="导出整合包"
            disabled={!!inst.error}
            onclick={() => {
              exporting = inst;
              exportOpen = true;
            }}
          >
            <Icon name="upload" size={14} />
          </button>
          <button class="btn sm icon" title="实例设置" onclick={() => openSettings(inst)}>
            <Icon name="gear" size={14} />
          </button>
          <button
            class="btn sm icon danger"
            title="删除"
            onclick={() => {
              confirmDelete = inst;
              deleteOpen = true;
            }}
          >
            <Icon name="trash" size={14} />
          </button>
        </div>
      </article>
    {/each}
  </div>
{/if}

<Modal bind:open={batchDeleteOpen} title={`删除 ${pickedList.length} 个实例？`} kicker="DELETE" width={460}>
  <p>将删除以下实例的整个版本目录，<strong>其中开启版本隔离的实例会连同存档、Mod 与配置一起删除</strong>。此操作无法撤销。</p>
  <ul class="del-list">
    {#each pickedList as i (i.id)}
      <li>
        <strong>{i.id}</strong>
        <span class="muted small">{i.isolated ? "版本隔离" : "共用目录"}{#if i.running} · 运行中，将跳过{/if}</span>
      </li>
    {/each}
  </ul>
  {#if pickedRunning.length}<p class="muted small">运行中的实例不会被删除，请先关闭游戏。</p>{/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (batchDeleteOpen = false)}>取消</button>
    <button class="btn primary" disabled={batchBusy || pickedList.length === pickedRunning.length} onclick={batchDelete}>
      {batchBusy ? "删除中…" : "确认删除"}
    </button>
  {/snippet}
</Modal>

<ModpackInstall bind:open={packOpen} source={packSource} title={packInfo?.name ?? ""} info={packInfo} />

<Modal bind:open={deleteOpen} title="删除实例？" kicker="DELETE" width={420}>
  <p>
    将删除 <strong>{confirmDelete?.id}</strong> 的整个版本目录{#if confirmDelete?.isolated}，<strong>包括其中的存档、Mod 与配置</strong>{/if}。此操作无法撤销。
  </p>
  {#snippet actions()}
    <button class="btn" onclick={() => (deleteOpen = false)}>取消</button>
    <button class="btn primary" onclick={doDelete}>确认删除</button>
  {/snippet}
</Modal>

<Modal bind:open={editOpen} title={`实例设置 · ${editing?.id ?? ""}`} kicker="SETTINGS" width={520}>
  {#if config}
    <div class="form">
      <div class="field">
        <span class="label">版本隔离</span>
        <Segmented
          bind:value={isolation}
          options={[
            { value: "default", label: "跟随全局" },
            { value: "on", label: "开启" },
            { value: "off", label: "关闭" },
          ]}
        />
        <span class="help">开启后，此实例的存档、Mod、配置都保存在 versions/{editing?.id} 中，互不影响。</span>
      </div>
      <div class="field">
        <label for="mem">最大内存（MB）</label>
        <input id="mem" class="input" type="number" min="512" step="256" placeholder="跟随全局设置" bind:value={memory} />
      </div>
      <div class="field">
        <label for="java">Java 路径</label>
        <input id="java" class="input" placeholder="跟随全局设置" bind:value={config.javaPath} />
      </div>
      <div class="field">
        <label for="jvm">额外 JVM 参数</label>
        <input id="jvm" class="input mono" placeholder="跟随全局设置" bind:value={config.jvmArgs} />
      </div>
      <div class="field">
        <label for="server">启动后进入服务器</label>
        <input id="server" class="input" placeholder="例如 mc.example.com:25565" bind:value={config.server} />
      </div>
    </div>
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (editOpen = false)}>取消</button>
    <button class="btn primary" onclick={saveConfig}>保存</button>
  {/snippet}
</Modal>

<ImportLauncher bind:open={migrateOpen} />
<ExportModpack bind:open={exportOpen} instance={exporting} />

<style>
  .tools {
    display: flex;
    gap: 10px;
  }
  .list {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 6px;
    border-bottom: 1px solid var(--rule);
    transition: background-color var(--t-fast);
  }
  .row:hover {
    background: var(--accent-soft);
  }
  .badge {
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    border: 1.5px solid var(--ink);
    box-shadow: var(--shadow-sm);
    font-family: var(--serif);
    font-weight: 900;
    font-size: 20px;
    background: var(--card);
    flex: none;
  }
  .badge[data-loader="fabric"] {
    background: #dbcfb0;
    color: #1b1a17;
  }
  .badge[data-loader="forge"] {
    background: #3b3f4a;
    color: #fff;
  }
  .badge[data-loader="neoforge"] {
    background: #d6812b;
    color: #fff;
  }
  .badge[data-loader="optifine"] {
    background: #a3262a;
    color: #fff;
  }
  .badge[data-loader="quilt"] {
    background: #8a5cc8;
    color: #fff;
  }
  .row.current .badge {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .main {
    flex: 1;
    min-width: 0;
  }
  .main h3 {
    font-size: 19px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .main p {
    font-size: 12px;
    margin-top: 2px;
  }
  .err {
    color: var(--danger);
  }
  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .batch-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 10px 12px;
    margin-bottom: 8px;
    border: 1.5px solid var(--ink);
    background: var(--card);
    box-shadow: var(--shadow-sm);
    position: sticky;
    top: 0;
    z-index: 2;
  }
  .batch-bar .spacer {
    flex: 1;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-weight: 600;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
    cursor: pointer;
  }
  .sm-select {
    min-width: 0;
    height: 30px;
    padding: 0 8px;
    font-size: 13px;
  }
  .nums {
    font-variant-numeric: tabular-nums;
  }
  .list.selecting .row {
    cursor: pointer;
  }
  .actions.hidden {
    visibility: hidden;
  }
  .del-list {
    list-style: none;
    padding: 0;
    margin: 12px 0 4px;
    max-height: 240px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .del-list li {
    display: flex;
    gap: 10px;
    align-items: baseline;
  }
  .small {
    font-size: 12px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
</style>
