<script lang="ts">
  // 安装确认：列出将一起安装的全部前置（名称、版本、是否已安装），可选依赖供勾选，冲突与缺失给出警告。
  import { api, type ContentType, type ContentVersion, type Plan, type Platform } from "../lib/api";
  import { showError, waitTask } from "../lib/state.svelte";
  import Modal from "./Modal.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let {
    open = $bindable(false),
    platform,
    instance,
    contentType,
    version,
    title,
  }: {
    open: boolean;
    platform: Platform;
    instance: string;
    contentType: ContentType;
    version: ContentVersion | null;
    title: string;
  } = $props();

  let plan = $state<Plan | null>(null);
  let error = $state<string | null>(null);
  let chosen = $state<Record<string, boolean>>({});
  let busy = $state(false);

  $effect(() => {
    if (!open || !version) return;
    const v = version;
    plan = null;
    error = null;
    chosen = {};
    api
      .contentPlan(platform, instance, contentType, [{ projectId: v.projectId, versionId: v.id }])
      .then((p) => (plan = p))
      .catch((e) => (error = `${e.message}${e.hint ? "\n" + e.hint : ""}`));
  });

  const toInstall = $derived(plan ? plan.items.filter((i) => !i.installed) : []);

  async function confirm() {
    if (!plan || !version) return;
    busy = true;
    let id: number;
    try {
      const roots = [{ projectId: version.projectId, versionId: version.id }];
      for (const o of plan.optional) {
        if (chosen[o.projectId]) roots.push({ projectId: o.projectId, versionId: o.version.id });
      }
      id = await api.contentInstall(platform, instance, contentType, roots);
      open = false;
    } catch (e) {
      showError(e, "安装失败");
      return;
    } finally {
      // 只在提交期间禁用按钮；下载在后台进行，可以继续安装别的资源
      busy = false;
    }
    waitTask(id).catch((e) => showError(e, "安装失败"));
  }
</script>

<Modal bind:open title={`安装 ${title}`} kicker="CONFIRM" width={560}>
  {#if error}
    <p class="err">{error}</p>
  {:else if !plan}
    <p class="muted">正在解析前置依赖…</p>
    <div class="gap"><ProgressBar indeterminate size="sm" /></div>
  {:else}
    <h4>将安装到「{instance}」</h4>
    <ul class="list">
      {#each plan.items as item}
        <li class:done={item.installed}>
          <span class="pixel" class:grey={item.installed}></span>
          <div class="main">
            <strong>{item.title}</strong>
            <span class="muted small mono">{item.version.versionNumber}</span>
            {#if item.requiredBy}<span class="muted small">· {item.requiredBy} 的前置</span>{/if}
          </div>
          <span class="tag" class:accent={!item.installed}>{item.installed ? "已安装，跳过" : "将安装"}</span>
        </li>
      {/each}
    </ul>

    {#if plan.optional.length}
      <h4>可选依赖</h4>
      <ul class="list">
        {#each plan.optional as item}
          <li>
            <label class="check">
              <input type="checkbox" bind:checked={chosen[item.projectId]} />
              <strong>{item.title}</strong>
              <span class="muted small mono">{item.version.versionNumber}</span>
            </label>
          </li>
        {/each}
      </ul>
    {/if}

    {#if plan.incompatible.length}
      <div class="warn">
        <strong>冲突提醒</strong>
        {#each plan.incompatible as w}
          <p>已安装的「{w.title}」{w.reason}，建议先在实例的 mods 文件夹中移除它。</p>
        {/each}
      </div>
    {/if}
    {#if plan.missing.length}
      <div class="err">
        <strong>缺少前置</strong>
        {#each plan.missing as m}
          <p>「{m.title}」（{m.requiredBy} 需要）：{m.reason}</p>
        {/each}
      </div>
    {/if}
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={() => (open = false)}>取消</button>
    <button class="btn primary" disabled={!plan || busy || plan.missing.length > 0} onclick={confirm}>
      安装 {toInstall.length + Object.values(chosen).filter(Boolean).length} 项
    </button>
  {/snippet}
</Modal>

<style>
  h4 {
    font-size: 14px;
    margin: 4px 0 8px;
  }
  .list {
    list-style: none;
    padding: 0;
    margin: 0 0 14px;
    border-top: 1px solid var(--rule);
  }
  .list li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 2px;
    border-bottom: 1px solid var(--rule);
  }
  .list li.done {
    opacity: 0.6;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 6px;
    flex-wrap: wrap;
  }
  .small {
    font-size: 11px;
  }
  .pixel.grey {
    background: var(--muted);
  }
  .check {
    display: flex;
    align-items: baseline;
    gap: 8px;
    cursor: pointer;
  }
  .check input {
    accent-color: var(--accent);
  }
  .warn,
  .err {
    padding: 10px 12px;
    margin-bottom: 10px;
    font-size: 13px;
    white-space: pre-line;
  }
  .warn {
    border-left: 4px solid #b7791f;
    background: color-mix(in srgb, #b7791f 10%, transparent);
  }
  .err {
    border-left: 4px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 8%, transparent);
  }
  .warn p,
  .err p {
    margin-top: 4px;
  }
  .gap {
    margin-top: 12px;
  }
</style>
