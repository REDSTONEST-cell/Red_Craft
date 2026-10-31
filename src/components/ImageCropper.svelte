<script lang="ts">
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import { backgroundApi, mediaUrl, type BackgroundAsset, type CropRect } from "../lib/background";
  import { drawCrop, moveCrop, resizeCrop } from "../lib/crop";

  let { asset, initial, onclose, onsaved }: { asset: BackgroundAsset; initial?: CropRect; onclose: () => void; onsaved: (asset: BackgroundAsset, sourceId: string) => Promise<void> } = $props();
  let alive = true;
  let open = $state(true);
  let url = $state("");
  let loaded = $state(false);
  let error = $state("");
  let saving = $state(false);
  let size = $state({ width: 1, height: 1 });
  let rect = $state<CropRect>({ x: 0.1, y: 0.1, width: 0.8, height: 0.8 });
  let stage: HTMLDivElement;
  let gesture: { id: number; x: number; y: number; box: DOMRect; rect: CropRect; mode: string } | null = null;
  onMount(() => {
    if (initial) rect = { ...initial };
    void backgroundApi.original(asset.id).then(value => {
      if (alive && value.media) url = mediaUrl(value.media);
    }).catch(() => { if (alive) error = "无法读取图片，请关闭后重新导入。"; });
    return () => { alive = false; };
  });
  function start(event: PointerEvent) {
    if (!loaded || saving || event.button !== 0) return;
    const box = stage.getBoundingClientRect();
    const x = (event.clientX - box.left) / box.width;
    const y = (event.clientY - box.top) / box.height;
    const target = event.target as HTMLElement;
    const mode = target.dataset.corner ?? (target.closest(".selection") ? "move" : "draw");
    gesture = { id: event.pointerId, x, y, box, rect: { ...rect }, mode };
    stage.setPointerCapture(event.pointerId);
    event.preventDefault();
  }
  function move(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.id) return;
    const { box, rect: original, mode, x, y } = gesture;
    const px = (event.clientX - box.left) / box.width;
    const py = (event.clientY - box.top) / box.height;
    if (mode === "move") rect = moveCrop(original, px - x, py - y);
    else if (mode === "draw") rect = drawCrop(x, y, px, py);
    else rect = resizeCrop(original, mode, px, py);
  }
  function end(event: PointerEvent) {
    if (gesture?.id === event.pointerId) {
      if (stage.hasPointerCapture(event.pointerId)) stage.releasePointerCapture(event.pointerId);
      gesture = null;
    }
  }
  function key(event: KeyboardEvent) {
    if (!saving && (event.key === "Enter" || event.key === " ")) {
      event.preventDefault(); rect = { x: 0.1, y: 0.1, width: 0.8, height: 0.8 }; return;
    }
    const step = event.shiftKey ? 0.05 : 0.01;
    const shifts: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    if (!saving && shifts[event.key]) { event.preventDefault(); rect = moveCrop(rect, ...shifts[event.key]); }
  }
  async function save() {
    saving = true; error = "";
    try {
      await backgroundApi.crop(asset.id, { ...rect });
      await onsaved(asset, asset.id);
      if (alive) onclose();
    } catch { error = "裁剪保存失败，原图已保留。请检查图片和可用空间后重试。"; }
    finally { saving = false; }
  }
</script>

<Modal bind:open title="裁剪背景图片" width={800} {onclose}>
  <p class="help">在图片上框选区域，拖动选区可移动，拖动四角可调整大小。方向键移动，Shift 加速。保存只更新这张素材的裁剪范围；重新编辑始终使用原图，不新增素材。</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="stage-wrap">
    <div class="stage" bind:this={stage} style:width="min(100%, {42 * size.width / size.height}vh)" style:aspect-ratio="{size.width} / {size.height}"
      role="button" aria-label="图片裁剪区域，拖动或方向键移动选区，回车重置选区" tabindex="0"
      onpointerdown={start} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={() => gesture = null} onkeydown={key}>
      {#if url}<img src={url} alt="待裁剪的背景" draggable="false"
        onload={event => { const image = event.currentTarget as HTMLImageElement; size = { width: image.naturalWidth, height: image.naturalHeight }; loaded = true; }}
        onerror={() => error = "图片无法显示，请重新导入。"} />{/if}
      {#if loaded}<div class="selection" style:left="{rect.x * 100}%" style:top="{rect.y * 100}%" style:width="{rect.width * 100}%" style:height="{rect.height * 100}%">
        {#each ["nw", "ne", "sw", "se"] as corner}<span class="handle {corner}" data-corner={corner}></span>{/each}
      </div>{/if}
    </div>
  </div>
  <p class="help">选区：{Math.ceil(rect.width * size.width)} × {Math.ceil(rect.height * size.height)} 像素。窗口与选区比例不同时，可用背景“完整显示”保留全部裁剪内容。</p>
  {#snippet actions()}
    <button class="btn sm" disabled={saving} onclick={() => rect = { x: 0, y: 0, width: 1, height: 1 }}>恢复整张</button>
    <button class="btn sm" disabled={saving} onclick={() => rect = { x: 0.1, y: 0.1, width: 0.8, height: 0.8 }}>重置选区</button>
    <button class="btn sm" disabled={saving} onclick={onclose}>取消</button>
    <button class="btn primary sm" disabled={!loaded || saving} onclick={save}>{saving ? "正在保存…" : "保存裁剪"}</button>
  {/snippet}
</Modal>

<style>
  .help { font-size: 12px; color: var(--muted); margin: 8px 0; }
  .error { color: var(--danger); }
  .stage-wrap { display: flex; justify-content: center; min-height: 100px; }
  .stage { position: relative; overflow: hidden; touch-action: none; cursor: crosshair; background: var(--paper-2); }
  img { display: block; width: 100%; height: 100%; user-select: none; }
  .selection { position: absolute; border: 2px solid white; outline: 1px solid #111; box-shadow: 0 0 0 2000px #0008; cursor: move; }
  .handle { position: absolute; width: 18px; height: 18px; background: white; border: 1px solid #111; }
  .nw { left: -3px; top: -3px; cursor: nwse-resize; }
  .ne { right: -3px; top: -3px; cursor: nesw-resize; }
  .sw { left: -3px; bottom: -3px; cursor: nesw-resize; }
  .se { right: -3px; bottom: -3px; cursor: nwse-resize; }
  @media (pointer: coarse) { .handle { width: 28px; height: 28px; } }
</style>
