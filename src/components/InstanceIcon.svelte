<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import type { InstanceInfo } from "../lib/api";
  import { LOADER_LABEL } from "../lib/format";

  // 版本图标：有自定义图标时显示图片，否则显示按加载器着色的默认字母徽章
  let {
    inst,
    size = 44,
    current = false,
  }: { inst: Pick<InstanceInfo, "loader" | "optifine" | "icon">; size?: number; current?: boolean } = $props();

  const loader = $derived(inst.loader?.kind ?? (inst.optifine ? "optifine" : "vanilla"));
  const letter = $derived(inst.loader ? LOADER_LABEL[inst.loader.kind][0] : inst.optifine ? "O" : "V");
  const src = $derived(inst.icon ? convertFileSrc(inst.icon) : "");
  // 图片丢失或损坏时回退到默认徽章；换了图标后重新尝试
  let failed = $state("");
</script>

<div
  class="badge"
  class:current
  class:custom={!!src && failed !== src}
  data-loader={loader}
  style:--size="{size}px"
  aria-hidden="true"
>
  {#if src && failed !== src}
    <img {src} alt="" draggable="false" onerror={() => (failed = src)} />
  {:else}
    <span>{letter}</span>
  {/if}
</div>

<style>
  .badge {
    width: var(--size);
    height: var(--size);
    display: grid;
    place-items: center;
    border: 1.5px solid var(--ink);
    box-shadow: var(--shadow-sm);
    font-family: var(--serif);
    font-weight: 900;
    font-size: calc(var(--size) * 0.45);
    line-height: 1;
    background: var(--card);
    color: var(--ink);
    flex: none;
    overflow: hidden;
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
  .badge.current {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .badge.custom {
    background: var(--card);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    image-rendering: auto;
  }
</style>
