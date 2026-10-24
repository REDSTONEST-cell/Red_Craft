<script lang="ts">
  import { api, type InstanceConfig, type InstanceInfo } from "../lib/api";
  import { app, refreshInstances, selectInstance, selectedInstance, showError, toast } from "../lib/state.svelte";
  import { KIND_LABEL, LOADER_LABEL, relative } from "../lib/format";
  import Modal from "../components/Modal.svelte";
  import Icon from "../components/Icon.svelte";
  import Segmented from "../components/Segmented.svelte";
  import ModpackInstall from "../components/ModpackInstall.svelte";
  import ImportLauncher from "../components/ImportLauncher.svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import type { ModpackInfo, PackSource } from "../lib/api";

  let packOpen = $state(false);
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
  <div class="list stagger">
    {#each app.instances as inst, i (inst.id)}
      <article class="row" class:current={current?.id === inst.id} style:--i={i}>
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
        <div class="actions">
          <button class="btn sm primary" disabled={!!inst.error} onclick={() => launch(inst)}><Icon name="play" size={12} /> 启动</button>
          <button class="btn sm icon" title="打开文件夹" onclick={() => api.openInstanceDir(inst.id).catch((e) => showError(e))}>
            <Icon name="folder" size={14} />
          </button>
          <button class="btn sm icon" title="补全文件" onclick={() => api.repairInstance(inst.id).catch((e) => showError(e))}>
            <Icon name="refresh" size={14} />
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
  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
</style>
