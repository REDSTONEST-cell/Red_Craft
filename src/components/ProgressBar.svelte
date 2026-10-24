<script lang="ts">
  // 方块分段进度条。填充层用 scaleX 变化，配合 transition 平滑过渡。
  let { value = 0, indeterminate = false, size = "md" }: { value?: number; indeterminate?: boolean; size?: "sm" | "md" | "lg" } =
    $props();
  const v = $derived(Math.max(0, Math.min(1, value)));
</script>

<div class="bar {size}" class:indeterminate role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(v * 100)}>
  <div class="fill" style:transform="scaleX({indeterminate ? 1 : v})"></div>
  <div class="grid"></div>
</div>

<style>
  .bar {
    position: relative;
    height: 12px;
    border: 1.5px solid var(--ink);
    background: var(--paper-2);
    overflow: hidden;
  }
  .bar.sm {
    height: 8px;
    border-width: 1px;
  }
  .bar.lg {
    height: 16px;
  }
  .fill {
    position: absolute;
    inset: 0;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 320ms var(--ease-out);
    will-change: transform;
  }
  /* 分段格线，营造方块感 */
  .grid {
    position: absolute;
    inset: 0;
    background: repeating-linear-gradient(90deg, transparent 0 10px, var(--paper-2) 10px 12px);
    opacity: 0.85;
  }
  /* 动画的优先级高于内联样式，会覆盖上面的 scaleX */
  .indeterminate .fill {
    width: 30%;
    animation: slide 1.1s steps(12) infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
</style>
