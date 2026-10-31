<script lang="ts" generics="T">
  // 固定行高的虚拟滚动列表：只渲染可见区域（加上下缓冲）的行，用 transform 定位。
  import type { Snippet } from "svelte";
  import { dur } from "../lib/motion";

  let {
    items,
    rowHeight,
    overscan = 6,
    row,
    onEnd,
    key = (_: T, i: number) => i,
    follow = false,
    appear = false,
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
    /** 新出现的行淡入：每个 key 只在第一次渲染时播放，滚动时复用或滚回来的行不再重复 */
    appear?: boolean;
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

  // 已经显示过的行（按 key 记录，不需要响应式）
  const seen = new Set<unknown>();
  // 同一次渲染中新出现的行依次错开一点点
  let batch = 0;
  function enter(node: HTMLElement, k: unknown) {
    if (!appear || seen.has(k)) return;
    seen.add(k);
    if (batch === 0) queueMicrotask(() => (batch = 0));
    const delay = dur(Math.min(batch++, 8) * 16);
    if (delay) node.style.animationDelay = `${delay}ms`;
    node.classList.add("enter");
    const done = (e: AnimationEvent) => {
      if (e.target !== node) return;
      node.removeEventListener("animationend", done);
      node.classList.remove("enter");
      node.style.animationDelay = "";
    };
    node.addEventListener("animationend", done);
  }

  export function scrollToTop() {
    if (viewport) viewport.scrollTop = 0;
  }
</script>

<div class="viewport" bind:this={viewport} onscroll={onScroll}>
  <div class="spacer" style:height="{items.length * rowHeight}px">
    <div class="window" style:transform="translateY({start * rowHeight}px)">
      {#each visible as item, i (key(item, start + i))}
        <div class="row" style:height="{rowHeight}px" use:enter={key(item, start + i)}>
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
  .row:global(.enter) {
    animation: row-in var(--t-mid) var(--ease-out) both;
  }
  @keyframes row-in {
    from {
      opacity: 0;
      transform: translate3d(0, 6px, 0);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
</style>
