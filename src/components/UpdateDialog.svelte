<script lang="ts">
  // 新版本确认框：显示更新内容，用户确认后下载并安装，完成后自动重启。
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { app } from "../lib/state.svelte";
  import { updater, downloadAndInstall, skipVersion } from "../lib/update.svelte";
  import { bytes, speed } from "../lib/format";
  import Modal from "./Modal.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  const info = $derived(updater.info);
  const task = $derived(updater.taskId === null ? null : app.tasks.find((t) => t.id === updater.taskId));
  const ratio = $derived(task?.progress?.totalBytes ? task.progress.doneBytes / task.progress.totalBytes : 0);

  const KIND: Record<string, string> = {
    nsis: "安装版",
    msi: "安装版（MSI）",
    portable: "单文件版",
    appImage: "AppImage",
    manual: "",
  };

  function escape(s: string) {
    return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
  }

  // 把更新说明（Markdown）简单转换为标题、列表与段落；表格（下载说明）省略
  const notes = $derived.by(() => {
    if (!info?.notes) return "";
    const out: string[] = [];
    let inList = false;
    const inline = (t: string) =>
      escape(t)
        .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
        .replace(/`([^`]+)`/g, "<code>$1</code>");
    for (const raw of info.notes.split(/\r?\n/)) {
      const line = raw.trimEnd();
      if (line.trimStart().startsWith("|")) continue;
      const item = /^\s*[-*] (.*)$/.exec(line);
      if (item) {
        if (!inList) out.push("<ul>");
        inList = true;
        out.push(`<li>${inline(item[1])}</li>`);
        continue;
      }
      if (inList && /^\s+\S/.test(line)) {
        out.push(`<p class="sub">${inline(line.trim())}</p>`);
        continue;
      }
      if (inList) out.push("</ul>");
      inList = false;
      const head = /^#{1,4} (.*)$/.exec(line);
      if (head) out.push(`<h4>${inline(head[1])}</h4>`);
      else if (line.trim()) out.push(`<p>${inline(line)}</p>`);
    }
    if (inList) out.push("</ul>");
    return out.join("");
  });
</script>

<Modal bind:open={updater.open} title={`发现新版本 ${info?.version ?? ""}`} kicker="UPDATE" width={560}>
  {#if info}
    <p class="ver">
      当前 <span class="mono">{info.current}</span> → 新版本 <strong class="mono">{info.version}</strong>
      {#if info.canInstall && info.size}<span class="muted"> · {KIND[info.kind]} {bytes(info.size)}</span>{/if}
    </p>
    {#if notes}
      <div class="notes">{@html notes}</div>
    {/if}
    {#if task}
      <div class="progress">
        <div class="row">
          <span>{task.stage}</span>
          {#if task.progress}
            <span class="mono muted">{bytes(task.progress.doneBytes)} / {bytes(task.progress.totalBytes)} · {speed(task.progress.speed)}</span>
          {/if}
        </div>
        <ProgressBar size="sm" value={ratio} indeterminate={!task.progress} />
        <p class="muted small">下载完成后会自动安装并重启启动器。可以先关闭这个窗口，下载会在后台继续。</p>
      </div>
    {:else if !info.canInstall}
      <p class="muted small">当前系统需要手动下载安装新版本。</p>
    {/if}
    {#if updater.error}<p class="err">{updater.error}</p>{/if}
  {/if}
  {#snippet actions()}
    {#if !task}
      <button class="btn ghost" onclick={skipVersion}>跳过此版本</button>
      <span class="spacer"></span>
      <button class="btn" onclick={() => (updater.open = false)}>稍后</button>
      {#if info?.canInstall}
        <button class="btn primary" onclick={downloadAndInstall}>{updater.error ? "重试" : "下载并安装"}</button>
      {:else}
        <button class="btn primary" onclick={() => info && openUrl(info.pageUrl)}>前往下载页</button>
      {/if}
    {:else}
      <button class="btn" onclick={() => (updater.open = false)}>后台下载</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .ver {
    margin-bottom: 12px;
  }
  .notes {
    max-height: 300px;
    overflow-y: auto;
    padding: 4px 14px 10px;
    border: 1px solid var(--rule);
    border-radius: var(--radius, 0);
    font-size: 13px;
    user-select: text;
    -webkit-user-select: text;
  }
  .notes :global(h4) {
    font-size: 14px;
    margin: 12px 0 4px;
  }
  .notes :global(ul) {
    margin: 4px 0;
    padding-left: 20px;
  }
  .notes :global(li) {
    margin: 3px 0;
  }
  .notes :global(p) {
    margin: 6px 0;
  }
  .notes :global(p.sub) {
    margin: 2px 0 2px 20px;
    color: var(--ink-2);
  }
  .notes :global(code) {
    font-family: var(--mono);
    font-size: 12px;
  }
  .progress {
    margin-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .progress .row {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    font-size: 13px;
  }
  .small {
    font-size: 12px;
  }
  .err {
    margin-top: 10px;
    color: var(--danger);
    font-size: 13px;
  }
  .spacer {
    flex: 1;
  }
</style>
