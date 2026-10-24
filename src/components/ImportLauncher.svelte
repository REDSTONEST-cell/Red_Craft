<script lang="ts">
  // 从其他启动器导入：选择来源 → 勾选实例与离线账号 → 导入（每个实例一个后台任务）。
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { api, type ImportInstance, type ImportLocation, type ImportScan } from "../lib/api";
  import { refreshAccounts, refreshInstances, showError, toast } from "../lib/state.svelte";
  import { LOADER_LABEL } from "../lib/format";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { open = $bindable(false) }: { open: boolean } = $props();

  let locations = $state<ImportLocation[]>([]);
  let scan = $state<ImportScan | null>(null);
  let scanning = $state(false);
  let error = $state<{ message: string; hint: string } | null>(null);
  let pickedInst = $state<Record<string, boolean>>({});
  let pickedAcc = $state<Record<string, boolean>>({});
  let busy = $state(false);

  $effect(() => {
    if (!open) return;
    scan = null;
    error = null;
    api
      .importLocations()
      .then((l) => {
        locations = l;
        if (l.length > 0) load(l[0].path);
      })
      .catch((e) => showError(e));
  });

  async function load(path: string) {
    scanning = true;
    error = null;
    try {
      const r = await api.importScan(path);
      scan = r;
      // 默认勾选：带 Mod 或存档的实例、全部离线账号
      pickedInst = Object.fromEntries(r.instances.map((i) => [i.name, !i.problem && (i.mods > 0 || i.saves > 0)]));
      pickedAcc = Object.fromEntries(r.offlineAccounts.map((n) => [n, true]));
    } catch (e) {
      scan = null;
      error = e as { message: string; hint: string };
    } finally {
      scanning = false;
    }
  }

  async function pickFolder() {
    const dir = await openDialog({ directory: true, title: "选择其他启动器的文件夹或 .minecraft 文件夹" });
    if (typeof dir === "string") load(dir);
  }

  const chosenInstances = $derived(scan?.instances.filter((i) => pickedInst[i.name] && !i.problem) ?? []);
  const chosenAccounts = $derived(scan?.offlineAccounts.filter((n) => pickedAcc[n]) ?? []);

  async function run() {
    busy = true;
    try {
      const parts: string[] = [];
      if (chosenAccounts.length) {
        const n = await api.importAccounts(chosenAccounts);
        await refreshAccounts();
        parts.push(`${n} 个离线账号`);
      }
      if (chosenInstances.length) {
        const ids = await api.importInstances(chosenInstances);
        parts.push(`${ids.length} 个实例（正在后台安装）`);
      }
      toast("ok", `已开始导入 ${parts.join("、")}`, chosenInstances.length ? "安装完成后会出现在实例列表中。" : undefined);
      refreshInstances();
      open = false;
    } catch (e) {
      showError(e, "导入失败");
    } finally {
      busy = false;
    }
  }

  const desc = (i: ImportInstance) =>
    [
      `Minecraft ${i.mcVersion || "?"}`,
      i.loader ? `${LOADER_LABEL[i.loader.kind]} ${i.loader.version}` : "原版",
      i.mods ? `${i.mods} 个 Mod` : null,
      i.saves ? `${i.saves} 个存档` : null,
    ]
      .filter(Boolean)
      .join(" · ");
</script>

<Modal bind:open title="从其他启动器导入" kicker="IMPORT" width={620}>
  <div class="sources">
    {#each locations as l (l.path)}
      <button class="src" class:active={scan?.root === l.path} title={l.path} onclick={() => load(l.path)}>{l.label}</button>
    {/each}
    <button class="src more" onclick={pickFolder}><Icon name="folder" size={12} /> 选择文件夹…</button>
  </div>
  <p class="muted small">
    支持官方启动器、HMCL、PCL 的 .minecraft 文件夹（也可以选启动器所在的文件夹），以及 Prism Launcher / MultiMC 的数据目录。
    导入时会在 Red Craft 中重新安装相同的游戏版本和加载器，再复制 Mod、存档、配置等数据，原文件不会被修改。
  </p>

  {#if scanning}
    <ProgressBar indeterminate size="sm" />
  {:else if error}
    <div class="err">
      <strong>{error.message}</strong>
      {#if error.hint}<p>{error.hint}</p>{/if}
    </div>
  {:else if scan}
    <div class="from mono" title={scan.root}>{scan.label} · {scan.root}</div>
    {#if scan.instances.length}
      <h4>实例</h4>
      <ul class="items">
        {#each scan.instances as i (i.name)}
          <li class:bad={!!i.problem}>
            <label class="check">
              <input type="checkbox" bind:checked={pickedInst[i.name]} disabled={!!i.problem} />
              <span class="body">
                <strong>{i.name}</strong>
                <span class="muted small">{i.problem ?? desc(i)}</span>
              </span>
              {#if i.layout === "shared"}<span class="tag" title="原启动器中多个版本共用这个目录">共用目录</span>{/if}
            </label>
          </li>
        {/each}
      </ul>
    {/if}
    {#if scan.offlineAccounts.length}
      <h4>离线账号</h4>
      <div class="accs">
        {#each scan.offlineAccounts as n (n)}
          <label class="check"><input type="checkbox" bind:checked={pickedAcc[n]} /> {n}</label>
        {/each}
      </div>
    {/if}
    {#if !scan.instances.length && !scan.offlineAccounts.length}
      <div class="empty"><h3>没有可导入的内容</h3></div>
    {/if}
  {:else if locations.length === 0}
    <div class="empty">
      <h3>没有自动找到其他启动器</h3>
      <p>点击「选择文件夹…」手动选择 HMCL、PCL 所在的文件夹或 .minecraft 文件夹。</p>
    </div>
  {/if}

  {#snippet actions()}
    <button class="btn" onclick={() => (open = false)}>取消</button>
    <button
      class="btn primary"
      disabled={busy || scanning || (!chosenInstances.length && !chosenAccounts.length)}
      onclick={run}
    >
      导入{#if chosenInstances.length || chosenAccounts.length}（{chosenInstances.length + chosenAccounts.length}）{/if}
    </button>
  {/snippet}
</Modal>

<style>
  .sources {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }
  .src {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rule);
    background: transparent;
    font-size: 12px;
    font-weight: 600;
    color: var(--ink-2);
    cursor: pointer;
    transition:
      border-color var(--t-fast),
      background-color var(--t-fast);
  }
  .src:hover {
    border-color: var(--ink-2);
  }
  .src.active {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--accent-soft);
  }
  .src.more {
    border-style: dashed;
  }
  .small {
    font-size: 12px;
  }
  p.small {
    margin-bottom: 12px;
  }
  .from {
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--rule-strong);
  }
  h4 {
    font-size: 13px;
    margin: 12px 0 4px;
  }
  .items {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 260px;
    overflow-y: auto;
  }
  .items li {
    border-bottom: 1px solid var(--rule);
  }
  .items li.bad {
    opacity: 0.55;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 4px;
    cursor: pointer;
  }
  .check input {
    accent-color: var(--accent);
    flex: none;
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .body strong {
    font-family: var(--serif);
    font-size: 15px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .accs {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 16px;
  }
  .accs .check {
    padding: 4px 0;
  }
  .err {
    border-left: 4px solid var(--danger);
    padding: 10px 12px;
    background: color-mix(in srgb, var(--danger) 8%, transparent);
  }
  .err p {
    margin-top: 4px;
    font-size: 13px;
    color: var(--ink-2);
    user-select: text;
  }
</style>
