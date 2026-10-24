// 过渡动画：只改变 transform 与 opacity。系统开启「减少动态效果」时时长为 0。

import type { TransitionConfig } from "svelte/transition";

const reduce =
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const easeOut = (t: number) => 1 - Math.pow(1 - t, 4);

export function dur(ms: number) {
  return reduce ? 0 : ms;
}

/** 页面切换：轻微上浮 + 淡入。 */
export function page(_node: Element, { y = 10, duration = 240 } = {}): TransitionConfig {
  return {
    duration: dur(duration),
    easing: easeOut,
    css: (t) => `opacity:${t};transform:translate3d(0,${(1 - t) * y}px,0)`,
  };
}

/** 弹窗：缩放 + 淡入。 */
export function pop(_node: Element, { from = 0.94, duration = 200 } = {}): TransitionConfig {
  return {
    duration: dur(duration),
    easing: easeOut,
    css: (t) => `opacity:${t};transform:scale(${from + (1 - from) * t})`,
  };
}

export function fade(_node: Element, { duration = 160 } = {}): TransitionConfig {
  return { duration: dur(duration), css: (t) => `opacity:${t}` };
}

/** 从右侧滑入（通知）。 */
export function slide(_node: Element, { x = 24, duration = 220 } = {}): TransitionConfig {
  return {
    duration: dur(duration),
    easing: easeOut,
    css: (t) => `opacity:${t};transform:translate3d(${(1 - t) * x}px,0,0)`,
  };
}
