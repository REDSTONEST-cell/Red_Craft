<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import type { InstanceInfo } from "../lib/api";
  import { defaultIconKind, DEFAULT_ICONS } from "../lib/versionIcons";

  // 版本图标：有自定义图标（含整合包图标）时显示图片；否则按版本类型显示默认图标——
  // 原版草方块、快照指令方块、Mod 版本显示对应加载器的图标。图标本身无背景、无边框。
  let {
    inst,
    size = 44,
  }: { inst: Pick<InstanceInfo, "loader" | "optifine" | "icon" | "kind">; size?: number } = $props();

  const fallback = $derived(DEFAULT_ICONS[defaultIconKind(inst)]);
  const custom = $derived(inst.icon ? convertFileSrc(inst.icon) : "");
  // 图片丢失或损坏时回退到默认图标；换了图标后重新尝试
  let failed = $state("");
  const src = $derived(custom && failed !== custom ? custom : fallback);
  // 默认图标是像素画：放大显示时保持像素清晰，缩小时平滑缩放
  const pixelated = $derived(src === fallback && size * (globalThis.devicePixelRatio || 1) >= 64);
</script>

<img
  class="icon"
  class:custom={src === custom}
  class:pixelated
  {src}
  alt=""
  draggable="false"
  style:--size="{size}px"
  aria-hidden="true"
  onerror={() => custom && (failed = custom)}
/>

<style>
  .icon {
    width: var(--size);
    height: var(--size);
    flex: none;
    display: block;
    object-fit: contain;
    user-select: none;
    -webkit-user-drag: none;
  }
  .icon.pixelated {
    image-rendering: pixelated;
  }
  .icon.custom {
    object-fit: cover;
  }
</style>
