<script lang="ts">
  import { onMount } from "svelte";
  import BackgroundThumbnail from "./BackgroundThumbnail.svelte";
  import ImageCropper from "./ImageCropper.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { waitTask, showError, toast, app } from "../lib/state.svelte";
  import { backgroundApi, defaults, isVisibleBackgroundAsset, type BackgroundLibrary, type BackgroundAsset } from "../lib/background";
  import { background, applyBackground, loadBackground } from "../lib/background-state.svelte";

  let library = $state<BackgroundLibrary>({ appearance: defaults(), assets: [], roots: [], scanNotes: [] });
  let draft = $state(defaults());
  let source = $state("wallpaperEngine");
  let busy = $state(false);
  let saving = $state(false);
  let cropping = $state<BackgroundAsset | null>(null);
  let cropRevision = $state(0);
  let scanStatus = $state("");
  let taskId = $state<number | null>(null);
  let alive = true;
  const task = $derived(app.tasks.find(t => t.id === taskId));
  const assets = $derived(library.assets.filter(a => a.source === source && isVisibleBackgroundAsset(a)));
  let page = $state(0);
  const activePage = $derived(Math.max(0, Math.min(page, Math.ceil(assets.length / 24) - 1)));
  const shown = $derived(assets.slice(activePage * 24, (activePage + 1) * 24));
  const dirty = $derived(JSON.stringify(draft) !== JSON.stringify(background.saved));

  onMount(() => {
    alive = true;
    void refresh().then(() => { if (alive) return scan(); }).catch(e => { if (alive) showError(e, "读取背景库失败"); });
    return () => {
      alive = false;
      background.preview = null;
      if (taskId) void api.cancelTask(taskId);
    };
  });

  async function refresh() {
    const value = await loadBackground();
    if (!alive) return;
    library = value;
    if (!background.preview) draft = { ...background.saved };
  }


  async function run(start: () => Promise<number>) {
    if (!alive || busy) return;
    busy = true;
    try {
      taskId = await start();
      if (!alive) { await api.cancelTask(taskId); return; }
      const result = await waitTask(taskId);
      if (alive && result.state === "done") await refresh();
    } catch (e) { showError(e, "背景操作失败"); }
    finally { busy = false; taskId = null; }
  }

  async function scan(roots = library.roots, forceRefresh = false) {
    if (!alive || busy) return;
    busy = true;
    scanStatus = "扫描本地壁纸库…";
    try {
      const result = await backgroundApi.scan([...roots], forceRefresh);
      if (!alive) return;
      scanStatus = result.from_cache ? "已读取视频壁纸缓存；官方自带项目已屏蔽。" : `扫描完成，发现 ${result.entries.length} 个视频壁纸，已排除官方自带项目。`;
      await refresh();
    } catch (e) { if (alive) showError(e, "扫描失败，保留原素材库"); }
    finally { busy = false; }
  }
  async function addDirectory() {
    const path = await open({ directory: true, title: "选择 Steam 库、Wallpaper Engine 项目目录或单个项目" });
    if (typeof path === "string") await scan([...new Set([...library.roots, path])], true);
  }
  async function importFile(kind: "image" | "video") {
    const path = await open({ title: kind === "image" ? "导入背景图片" : "导入背景视频", multiple: false,
      filters: [{ name: kind === "image" ? "图片" : "视频", extensions: kind === "image" ? ["jpg","jpeg","png","webp"] : ["mp4","webm"] }] });
    if (typeof path === "string") { source = "local"; page = 0; await run(() => backgroundApi.import(path)); }
  }
  function preview(asset: BackgroundAsset) {
    background.retry++;
    draft = { ...draft, selection: asset.id };
    background.preview = { ...draft };
  }
  function tune() { background.preview = { ...draft }; }
  function cancel() { background.preview = null; draft = { ...background.saved }; }
  async function apply() {
    const submittedPreview = background.preview;
    saving = true;
    try { await applyBackground(draft); library.appearance = { ...background.saved }; toast("ok", "背景已保存"); }
    catch (e) { if (background.preview === submittedPreview) cancel(); showError(e, "保存背景失败，保留原选择"); }
    finally { saving = false; }
  }
  async function remove(asset: BackgroundAsset) {
    busy = true;
    try {
      if (draft.selection === asset.id) cancel();
      await backgroundApi.remove(asset.id);
      toast("ok", asset.source === "local" ? "已移除导入副本，原文件保留" : "已移除记录，Wallpaper 文件保留");
    } catch (e) { showError(e, "移除素材失败"); }
    finally { await refresh().catch(e => showError(e)); busy = false; }
  }
  async function cropped(asset: BackgroundAsset, sourceId: string) {
    await refresh();
    cropRevision++;
    source = "local";
    page = Math.max(0, Math.ceil(library.assets.filter(a => a.source === "local").length / 24) - 1);
    if (cropping?.id === sourceId) {
      draft = { ...draft, fit: "cover" };
      preview(asset);
    }
    toast("ok", "已更新这张素材的裁剪范围");
  }
</script>

<div class="background-picker">
  <div class="intro"><strong>背景</strong><span class="help">扫描后手动选择；只有点击“应用并保存”才会持久化。</span></div>
  <div class="toolbar">
    <button class="btn sm" class:active={source === "wallpaperEngine"} onclick={() => { source = "wallpaperEngine"; page = 0; }}>Wallpaper Engine</button>
    <button class="btn sm" class:active={source === "local"} onclick={() => { source = "local"; page = 0; }}>本地素材</button>
    <button class="btn sm" disabled={busy || saving} onclick={() => scan(library.roots, true)}>刷新扫描</button>
    <button class="btn sm" disabled={busy || saving} onclick={addDirectory}>添加目录</button>
    <button class="btn sm" disabled={busy || saving} onclick={() => importFile("image")}>导入图片</button>
    <button class="btn sm" disabled={busy || saving} onclick={() => importFile("video")}>导入视频</button>
  </div>
  {#if busy}<div class="status" role="status">{task?.stage ?? "扫描或准备中…"}
    {#if taskId}<button class="btn sm" onclick={() => api.cancelTask(taskId!)}>取消任务</button>{/if}
  </div>{/if}
  {#if scanStatus && !busy}<p class="help" role="status">{scanStatus}</p>{/if}
  {#if background.error}<div class="error" role="alert">{background.error}</div>{/if}
  <div class="asset-grid">
    {#each shown as asset (asset.id)}
      <article class:selected={draft.selection === asset.id}>
        <div class="cover">
          {#key `${asset.id}-${cropRevision}`}<BackgroundThumbnail {asset} />{/key}
        </div>
        <strong title={asset.title}>{asset.title}</strong>
        <span class="help">{asset.kind} · {asset.reason ?? "可用"}{background.saved.selection === asset.id ? " · 已应用" : ""}</span>
        <div class="toolbar">
          <button class="btn sm" disabled={!!asset.reason || saving} onclick={() => preview(asset)}>预览</button>
          {#if asset.source === "local" && asset.kind === "image"}
            <button class="btn sm" disabled={!!asset.reason || busy || saving} onclick={() => cropping = asset}>裁剪</button>
          {/if}
          <button class="btn sm ghost" disabled={busy || saving} onclick={() => remove(asset)}>移除{asset.source === "local" ? "副本" : "记录"}</button>
        </div>
      </article>
    {:else}<div class="empty">{source === "local" ? "还没有本地素材，请导入图片或视频。" : "未发现本地壁纸，可添加 Steam 库或项目目录。"}</div>{/each}
  </div>
  {#if assets.length > 24}<div class="toolbar">
    <button class="btn sm" disabled={activePage === 0} onclick={() => page = activePage - 1}>上一页</button>
    <span>{activePage + 1} / {Math.ceil(assets.length / 24)}</span>
    <button class="btn sm" disabled={(activePage + 1) * 24 >= assets.length} onclick={() => page = activePage + 1}>下一页</button>
  </div>{/if}
  <fieldset class="controls" disabled={saving}>
    <label>缩放<select bind:value={draft.fit} onchange={tune}><option value="cover">居中裁切填满</option><option value="contain">完整显示</option></select></label>
    <label>动态模式<select bind:value={draft.motion} onchange={tune}><option value="auto">自动播放（静音）</option><option value="static">静态</option></select></label>
    <label>亮度 {draft.brightness.toFixed(2)}<input type="range" min="0.2" max="1.2" step="0.05" bind:value={draft.brightness} oninput={tune} /></label>
    <label>背景不透明度 {Math.round(draft.backgroundOpacity * 100)}%<input type="range" min="0" max="1" step="0.01" bind:value={draft.backgroundOpacity} oninput={tune} /></label>
    <label>背景模糊度 {draft.blurPx}px<input type="range" min="0" max="24" step="1" bind:value={draft.blurPx} oninput={tune} /></label>
    <label>遮罩强度 {Math.round(draft.overlayOpacity * 100)}%<input type="range" min="0" max="0.8" step="0.05" bind:value={draft.overlayOpacity} oninput={tune} /></label>
  </fieldset>
  <div class="toolbar actions">
    <button class="btn primary sm" disabled={saving || busy || !dirty || (!!draft.selection && (!!background.error || (!background.ready && draft.motion === "auto")))} onclick={apply}>应用并保存</button>
    <button class="btn sm" disabled={saving} onclick={cancel}>取消预览</button>
    <button class="btn sm" disabled={saving} onclick={() => { draft = defaults(); tune(); }}>恢复默认（预览）</button>
    {#if dirty}<span class="help">预览中，尚未保存</span>{/if}
  </div>
  {#if library.roots.length}<details><summary>手动目录（{library.roots.length}）</summary>
    {#each library.roots as root}<div class="manual-root"><span>{root}</span><button class="btn sm ghost" disabled={busy} onclick={() => scan(library.roots.filter(p => p !== root), true)}>移除目录记录</button></div>{/each}
  </details>{/if}
  {#if library.scanNotes.length}<details><summary>扫描说明（{library.scanNotes.length}）</summary><ul>{#each library.scanNotes as note}<li>{note}</li>{/each}</ul></details>{/if}
  <p class="help">Wallpaper Engine 仅列出视频壁纸，屏蔽官方自带项目和场景、网页等类型。视频解码取决于系统 WebView2，建议 H.264 MP4。</p>
</div>

{#if cropping}<ImageCropper asset={cropping} initial={library.crops?.[cropping.id]} onclose={() => cropping = null} onsaved={cropped} />{/if}

<style>
  .background-picker { margin-top: 18px; padding-top: 16px; border-top: 1px solid var(--rule); }
  .intro { display: grid; gap: 3px; margin-bottom: 12px; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .active { box-shadow: inset 0 -3px var(--accent); }
  .asset-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px; margin: 16px 0; max-height: 430px; overflow-y: auto; padding: 3px; }
  article { min-width: 0; padding: 10px; display: grid; gap: 8px; background: var(--card); border: 1px solid var(--rule); }
  article.selected { outline: 2px solid var(--accent); }
  article strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cover { height: 95px; background: var(--paper-2); display: grid; place-items: center; overflow: hidden; }
  .help { font-size: 12px; color: var(--muted); overflow-wrap: anywhere; }
  .controls { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px 24px; border: 0; margin: 0; padding: 0; min-width: 0; }
  label { display: grid; gap: 5px; font-size: 12px; }
  input { width: 100%; accent-color: var(--accent); }
  .actions, details, p { margin-top: 16px; }
  details { font-size: 12px; overflow-wrap: anywhere; }
  .manual-root { display: flex; justify-content: space-between; align-items: center; gap: 10px; padding: 5px 0; }
  .status, .error, .empty { padding: 12px; background: var(--paper-2); font-size: 12px; }
  .error { color: var(--danger); }
</style>
