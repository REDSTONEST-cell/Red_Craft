<script lang="ts" generics="T extends string">
  // 分段选择器：选中块用 transform 滑动。
  let {
    value = $bindable(),
    options,
    onchange,
  }: { value: T; options: { value: T; label: string }[]; onchange?: (v: T) => void } = $props();

  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)));
</script>

<div class="seg" style:--n={options.length}>
  <span class="thumb" style:transform="translateX({index * 100}%)"></span>
  {#each options as o}
    <button
      class:active={o.value === value}
      onclick={() => {
        value = o.value;
        onchange?.(o.value);
      }}>{o.label}</button
    >
  {/each}
</div>

<style>
  .seg {
    position: relative;
    display: inline-grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    border: 1.5px solid var(--ink);
    background: var(--card);
    height: 32px;
  }
  .thumb {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: calc(100% / var(--n));
    background: var(--ink);
    transition: transform var(--t-mid) var(--ease-out);
    will-change: transform;
  }
  button {
    position: relative;
    border: 0;
    background: transparent;
    padding: 0 14px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    color: var(--ink-2);
    transition: color var(--t-mid);
    white-space: nowrap;
  }
  button.active {
    color: var(--paper);
  }
</style>
