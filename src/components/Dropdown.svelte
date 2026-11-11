<script lang="ts" generics="T extends string | null">
  // 下拉选择：点击后列表像抽屉一样从按钮下方滑出，箭头顺时针旋转 180°。
  // 代替系统的 <select>（它的列表不跟随主题，深色主题与自定义背景下很突兀）。
  import { tick } from "svelte";
  import type { TransitionConfig } from "svelte/transition";
  import { dur } from "../lib/motion";
  import Icon from "./Icon.svelte";

  let {
    value = $bindable(),
    options,
    placeholder = "请选择",
    title,
    disabled = false,
    onchange,
  }: {
    value: T;
    options: { value: T; label: string; hint?: string }[];
    placeholder?: string;
    title?: string;
    disabled?: boolean;
    onchange?: (v: T) => void;
  } = $props();

  let open = $state(false);
  let button = $state<HTMLButtonElement | null>(null);
  let panel = $state<HTMLDivElement | null>(null);
  // 键盘上下键移动的高亮项
  let active = $state(-1);
  // 列表的位置（相对窗口）；空间不够时向上展开
  let place = $state({ left: 0, top: 0, width: 0, maxHeight: 320, up: false });

  const current = $derived(options.find((o) => o.value === value));

  function measure() {
    if (!button) return;
    const r = button.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 12;
    const above = r.top - 12;
    const up = below < 180 && above > below;
    place = {
      left: r.left,
      top: up ? r.top - 4 : r.bottom + 4,
      width: r.width,
      maxHeight: Math.max(120, Math.min(320, up ? above : below)),
      up,
    };
  }

  async function show() {
    if (disabled || open) return;
    measure();
    active = options.findIndex((o) => o.value === value);
    open = true;
    await tick();
    panel?.querySelector(".option.selected")?.scrollIntoView({ block: "nearest" });
  }

  function hide(focus = false) {
    if (!open) return;
    open = false;
    if (focus) button?.focus();
  }

  function pick(v: T) {
    hide(true);
    if (v === value) return;
    value = v;
    onchange?.(v);
  }

  function onKey(e: KeyboardEvent) {
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        void show();
      }
      return;
    }
    if (e.key === "Escape") {
      // 不让外层的弹窗也跟着关闭
      e.preventDefault();
      e.stopPropagation();
      hide(true);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = options.length;
      if (!n) return;
      active = (active + (e.key === "ArrowDown" ? 1 : -1) + n) % n;
      void tick().then(() => panel?.querySelector(".option.active")?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (options[active]) pick(options[active].value);
    } else if (e.key === "Tab") {
      hide();
    }
  }

  // 展开期间：点击别处、滚动页面、窗口大小变化或失去焦点时收起
  $effect(() => {
    if (!open) return;
    const outside = (e: Event) => {
      const t = e.target as Node;
      if (!button?.contains(t) && !panel?.contains(t)) hide();
    };
    const scrolled = (e: Event) => {
      if (!panel?.contains(e.target as Node)) hide();
    };
    const close = () => hide();
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("scroll", scrolled, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("scroll", scrolled, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  });

  // 挂到 #app 根部，不被页面或弹窗的滚动区域裁掉（与 Modal 相同）
  function portal(node: HTMLElement) {
    (document.getElementById("app") ?? document.body).appendChild(node);
    return { destroy: () => node.remove() };
  }

  /** 抽屉：从按钮一侧滑出。裁剪范围向外留出余量，阴影随列表一起展开而不是最后突然出现；只触发合成。 */
  function drawer(_node: Element, { up = false, duration = 280 } = {}): TransitionConfig {
    return {
      duration: dur(duration),
      // 先快后慢，结尾没有停顿
      easing: (t) => 1 - Math.pow(1 - t, 5),
      css: (t) => {
        const edge = `calc(${(1 - t) * 100}% - ${t * 60}px)`;
        const clip = up ? `inset(${edge} -60px -60px -60px)` : `inset(-60px -60px ${edge} -60px)`;
        return `opacity:${Math.min(1, t * 2)};clip-path:${clip};transform:translate3d(0,${(1 - t) * (up ? 10 : -10)}px,0)`;
      },
    };
  }
</script>

<button
  type="button"
  class="input dropdown"
  class:open
  bind:this={button}
  {disabled}
  {title}
  aria-haspopup="listbox"
  aria-expanded={open}
  onclick={() => (open ? hide() : show())}
  onkeydown={onKey}
>
  <span class="value" class:placeholder={!current}>{current?.label ?? placeholder}</span>
  <span class="arrow"><Icon name="chevron" size={12} /></span>
</button>

{#if open}
  <div
    class="dropdown-panel"
    class:up={place.up}
    use:portal
    bind:this={panel}
    role="listbox"
    style:left="{place.left}px"
    style:top={place.up ? undefined : `${place.top}px`}
    style:bottom={place.up ? `${window.innerHeight - place.top}px` : undefined}
    style:min-width="{place.width}px"
    style:max-height="{place.maxHeight}px"
    in:drawer={{ up: place.up }}
    out:drawer={{ up: place.up, duration: 180 }}
  >
    {#each options as o, i (o.value)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="option"
        class:selected={o.value === value}
        class:active={i === active}
        role="option"
        tabindex="-1"
        aria-selected={o.value === value}
        onclick={() => pick(o.value)}
        onpointerenter={() => (active = i)}
      >
        <span class="text">
          <span class="label">{o.label}</span>
          {#if o.hint}<span class="hint">{o.hint}</span>{/if}
        </span>
        {#if o.value === value}<Icon name="check" size={12} />{/if}
      </div>
    {/each}
    {#if options.length === 0}
      <div class="none">没有可选项</div>
    {/if}
  </div>
{/if}

<style>
  .dropdown {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    cursor: pointer;
    text-align: left;
    color: var(--ink);
  }
  .dropdown:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .dropdown.open {
    border-color: var(--accent);
  }
  .value {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .value.placeholder {
    color: var(--muted);
  }
  /* 图标本身朝右：收起时转 90° 朝下，展开时再顺时针转 180° 朝上 */
  .arrow {
    flex: none;
    display: grid;
    place-items: center;
    color: var(--muted);
    transform: rotate(90deg);
    transition: transform var(--t-mid, 220ms) var(--ease, ease);
  }
  .open .arrow {
    transform: rotate(270deg);
    color: var(--accent);
  }
  /* 不描边：靠略微抬亮的底色与柔和的阴影和页面分出层次 */
  .dropdown-panel {
    position: fixed;
    z-index: 70;
    max-width: min(520px, calc(100vw - 24px));
    overflow-y: auto;
    padding: 6px;
    background: color-mix(in srgb, var(--card) 94%, var(--ink));
    color: var(--ink);
    border: 0;
    border-radius: max(var(--radius), 6px);
    box-shadow:
      0 18px 40px -8px rgba(0, 0, 0, 0.38),
      0 4px 12px rgba(0, 0, 0, 0.18);
    will-change: transform, clip-path, opacity;
    scrollbar-width: thin;
  }
  .option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 7px 10px;
    border-radius: max(calc(var(--radius) * 0.7), 4px);
    cursor: pointer;
    font-size: 13px;
    transition: background-color var(--t-fast) var(--ease);
  }
  .option.active {
    background: var(--accent-soft);
  }
  .option.selected {
    color: var(--accent);
    font-weight: 600;
  }
  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .label,
  .hint {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .hint {
    font-size: 11px;
    font-weight: 400;
    color: var(--muted);
  }
  .none {
    padding: 10px;
    font-size: 12px;
    color: var(--muted);
  }
  @media (prefers-reduced-motion: reduce) {
    .arrow {
      transition: none;
    }
  }
</style>
