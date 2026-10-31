<script lang="ts">
  // 导出整合包：与 PCL 类似，勾选要打包的内容；能在 Modrinth 上找到的文件只写下载地址。
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { api, type ExportEntry, type InstanceInfo } from "../lib/api";
  import { showError, toast } from "../lib/state.svelte";
  import { bytes } from "../lib/format";
  import Modal from "./Modal.svelte";
  import Toggle from "./Toggle.svelte";

  let { open = $bindable(false), instance }: { open: boolean; instance: InstanceInfo | null } = $props();

  let entries = $state<ExportEntry[] | null>(null);
  let picked = $state<Record<string, boolean>>({});
  let name = $state("");
  let version = $state("1.0.0");
  let summary = $state("");
  let online = $state(true);
  let busy = $state(false);

  // 常见条目的说明
  const LABELS: Record<string, string> = {
    mods: "Mod",
    config: "Mod 配置",
    defaultconfigs: "默认配置",
    kubejs: "KubeJS 脚本",
    scripts: "CraftTweaker 脚本",
    resourcepacks: "资源包",
    shaderpacks: "光影包",
    saves: "存档",
    screenshots: "截图",
    schematics: "原理图",
    "options.txt": "游戏设置",
    "optionsof.txt": "OptiFine 设置",
    "optionsshaders.txt": "光影设置",
    "servers.dat": "服务器列表",
  };

  $effect(() => {
    if (!open || !instance) return;
    const id = instance.id;
    name = id;
    entries = null;
    api
      .exportEntries(id)
      .then((list) => {
        entries = list;
        picked = Object.fromEntries(list.map((e) => [e.name, e.default]));
      })
      .catch((e) => {
        showError(e, "无法读取版本文件");
        open = false;
      });
  });

  const chosen = $derived((entries ?? []).filter((e) => picked[e.name]));
  const total = $derived(chosen.reduce((s, e) => s + e.size, 0));

  async function doExport() {
    if (!instance || !chosen.length) return;
    const fileName = `${name.trim() || instance.id}-${version.trim() || "1.0.0"}.mrpack`.replace(/[\\/:*?"<>|]/g, "_");
    const path = await saveDialog({
      title: "导出整合包",
      defaultPath: fileName,
      filters: [{ name: "Modrinth 整合包", extensions: ["mrpack"] }],
    });
    if (!path) return;
    busy = true;
    try {
      await api.exportModpack({
        instance: instance.id,
        path,
        name: name.trim() || instance.id,
        version: version.trim() || "1.0.0",
        summary: summary.trim(),
        entries: chosen.map((e) => e.name),
        online,
      });
      toast("ok", "已开始导出整合包", "进度见右下角的任务按钮");
      open = false;
    } catch (e) {
      showError(e, "导出失败");
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={`导出整合包 · ${instance?.id ?? ""}`} kicker="EXPORT" width={560}>
  <div class="form">
    <div class="grid">
      <label class="field">
        <span class="label">整合包名称</span>
        <input class="input" bind:value={name} placeholder={instance?.id} />
      </label>
      <label class="field">
        <span class="label">版本号</span>
        <input class="input mono" bind:value={version} placeholder="1.0.0" />
      </label>
    </div>
    <label class="field">
      <span class="label">简介（可选）</span>
      <input class="input" bind:value={summary} placeholder="一句话介绍这个整合包" />
    </label>

    <div class="field">
      <span class="label">打包内容</span>
      {#if entries === null}
        <p class="muted small">读取中…</p>
      {:else if entries.length === 0}
        <p class="muted small">这个版本的目录中还没有可以导出的内容。</p>
      {:else}
        <ul class="entries">
          {#each entries as e (e.name)}
            <li>
              <label class="check">
                <input type="checkbox" bind:checked={picked[e.name]} />
                <span class="name">
                  {LABELS[e.name] ?? e.name}
                  {#if LABELS[e.name]}<span class="mono muted small">{e.name}</span>{/if}
                </span>
                <span class="muted small nums">{e.isDir ? `${e.files} 个文件 · ` : ""}{bytes(e.size)}</span>
              </label>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="row">
      <div class="desc">
        <strong>Mod 改为在线下载</strong>
        <span class="help">能在 Modrinth 上找到的 Mod、资源包和光影包只记录下载地址，整合包体积更小；找不到的仍会打包进去。</span>
      </div>
      <Toggle checked={online} onchange={(v) => (online = v)} />
    </div>
    <p class="muted small">
      导出为 Modrinth 整合包（.mrpack），RCL、PCL、HMCL、Prism 等启动器都可以导入。已选 {chosen.length} 项，约 {bytes(total)}。
    </p>
  </div>
  {#snippet actions()}
    <button class="btn" onclick={() => (open = false)}>取消</button>
    <button class="btn primary" disabled={busy || !chosen.length} onclick={doExport}>导出</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 140px;
    gap: 12px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-size: 12px;
    font-weight: 700;
    color: var(--ink-2);
  }
  .entries {
    list-style: none;
    padding: 0;
    margin: 0;
    max-height: 240px;
    overflow-y: auto;
    border: 1px solid var(--rule);
    border-radius: var(--radius, 0);
  }
  .entries li + li {
    border-top: 1px solid var(--rule);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    cursor: pointer;
  }
  .check .name {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-weight: 600;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .desc {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .help {
    font-size: 12px;
    color: var(--muted);
  }
  .small {
    font-size: 12px;
  }
  .nums {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
