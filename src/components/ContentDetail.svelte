<script lang="ts">
  // 项目详情：只列出兼容当前实例的版本，可一键安装最新兼容版本。
  import { api, type ContentVersion, type ProjectHit } from "../lib/api";
  import { count, date } from "../lib/format";
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

  $effect(() => {
    if (!open || !project) return;
    const p = project;
    versions = null;
    error = null;
    api
      .contentVersions(p.platform, p.id, p.contentType, compatibleOnly ? instance : null)
      .then((v) => (versions = v))
      .catch((e) => (error = `${e.message}${e.hint ? "\n" + e.hint : ""}`));
  });

  const best = $derived(versions?.find((v) => v.releaseType === "release") ?? versions?.[0] ?? null);
  const typeLabel: Record<string, string> = { release: "正式", beta: "测试", alpha: "早期" };
</script>

<Modal bind:open title={project?.title ?? ""} kicker={project?.platform === "curseforge" ? "CURSEFORGE" : "MODRINTH"} width={720}>
  {#if project}
    <div class="head">
      {#if project.iconUrl}<img src={project.iconUrl} alt="" class="icon" />{:else}<span class="icon blank"></span>{/if}
      <div>
        <p class="desc">{project.description}</p>
        <p class="muted small">
          {#if project.author}{project.author} · {/if}{count(project.downloads)} 次下载 · 更新于 {date(project.updated)}
        </p>
      </div>
    </div>
  {/if}
  {#if error}
    <p class="err">{error}</p>
  {:else if versions === null}
    <ProgressBar indeterminate size="sm" />
  {:else if versions.length === 0}
    <div class="empty">
      <h3>没有兼容的版本</h3>
      <p>这个项目没有适用于当前实例游戏版本{instance ? "和加载器" : ""}的版本。</p>
    </div>
  {:else}
    <div class="list">
      <VirtualList items={versions} rowHeight={52} key={(v) => v.id}>
        {#snippet row(v)}
          <div class="row">
            <div class="main">
              <strong>{v.name || v.versionNumber}</strong>
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
  {#snippet actions()}
    <button class="btn" onclick={() => (open = false)}>关闭</button>
    <button class="btn primary" disabled={!best} onclick={() => best && oninstall(best)}>
      安装最新兼容版本{best ? `（${best.versionNumber}）` : ""}
    </button>
  {/snippet}
</Modal>

<style>
  .head {
    display: flex;
    gap: 14px;
    align-items: flex-start;
    margin-bottom: 14px;
  }
  .icon {
    width: 56px;
    height: 56px;
    border: 1.5px solid var(--ink);
    object-fit: cover;
    image-rendering: pixelated;
    flex: none;
  }
  .icon.blank {
    background: var(--paper-2);
  }
  .desc {
    font-family: var(--serif);
    font-size: 15px;
  }
  .small {
    font-size: 11px;
  }
  .list {
    height: 46vh;
    border-top: 1px solid var(--rule-strong);
  }
  .row {
    height: 52px;
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
  }
  .main strong {
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
</style>
