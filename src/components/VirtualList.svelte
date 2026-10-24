<script lang="ts" generics="T">
  // 固定行高的虚拟滚动列表：只渲染可见区域（加上下缓冲）的行，用 transform 定位。
  import type { Snippet } from "svelte";

  let {
    items,
    rowHeight,
    overscan = 6,
    row,
    onEnd,
    key = (_: T, i: number) => i,
    follow = false,
  }: {
    items: T[];
    rowHeight: number;
    overscan?: number;
    row: Snippet<[T, number]>;
    /** 滚动到底部附近时触发（用于加载更多） */
    onEnd?: () => void;
    key?: (item: T, i: number) => unknown;
    /** 新数据到来时，若当前在底部则自动滚到底（日志） */
    follow?: boolean;
  } = $props();

  let viewport: HTMLDivElement;
  let scrollTop = $state(0);
  let height = $state(400);
  let atBottom = true;

  const start = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - overscan));
  const end = $derived(Math.min(items.length, Math.ceil((scrollTop + height) / rowHeight) + overscan));
  const visible = $derived(items.slice(start, end));

  function onScroll() {
    scrollTop = viewport.scrollTop;
    atBottom = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < rowHeight * 2;
    if (onEnd && viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < rowHeight * 8) {
      onEnd();
    }
  }

  $effect(() => {
    const ro = new ResizeObserver(() => (height = viewport.clientHeight));
    ro.observe(viewport);
    return () => ro.disconnect();
  });

  $effect(() => {
    // 依赖数组本身：日志达到上限后长度不变，但内容仍在更新
    void items;
    if (follow && atBottom && viewport) {
      requestAnimationFrame(() => {
        viewport.scrollTop = viewport.scrollHeight;
      });
    }
  });

  export function scrollToTop() {
    if (viewport) viewport.scrollTop = 0;
  }
</script>

<div class="viewport" bind:this={viewport} onscroll={onScroll}>
  <div class="spacer" style:height="{items.length * rowHeight}px">
    <div class="window" style:transform="translateY({start * rowHeight}px)">
      {#each visible as item, i (key(item, start + i))}
        <div class="row" style:height="{rowHeight}px">
          {@render row(item, start + i)}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .viewport {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    contain: strict;
  }
  .spacer {
    position: relative;
  }
  .window {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    will-change: transform;
  }
  .row {
    overflow: hidden;
  }
</style>
