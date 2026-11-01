// 手机版的资源图标加载。
//
// 电脑版通过 rcicon 协议交给后端获取并缓存图标。Android 上同样的地址是 http://rcicon.localhost/…，
// 由 WebView 的 shouldInterceptRequest 转给后端：这个回调会一直占着 WebView 的请求线程，直到后端
// 下载完成才返回，图标多时会排队，还会拖慢同样走这条路的其他请求；返回的内容又被强制标记为
// no-store，WebView 不会缓存，列表滚动、切换页面后每次都要重新请求。
//
// 所以手机上让 WebView 直接加载图标（参照 FCL 用 Glide 加载资源图标的做法）：
// - 优先使用 MCIM 镜像，设置为「官方源」时优先官方地址；
// - 由 WebView 自己的网络栈并发加载，可见区域的图片优先，列表只渲染可见的几行；
// - 磁盘缓存：WebView 的 HTTP 缓存，Android 端设置为有缓存时直接使用（不重新验证）；
// - 加载失败或长时间没有完成时，依次换用另一个地址，最后交给后端（rcicon，带本地缓存）。

import { convertFileSrc } from "@tauri-apps/api/core";

const MCIM = "https://mod.mcimirror.top";
const HOSTS = ["https://cdn.modrinth.com", "https://media.forgecdn.net", "https://edge.forgecdn.net", "https://mediafilez.forgecdn.net"];
/** 一个地址多久没有加载完成就换下一个 */
const STALL_MS = 7000;

/** 是否优先官方地址（设置中社区资源选择「官方源」时），由 MobileApp 根据设置更新 */
export const iconPrefs = { official: false };

/** 候选地址 → 原始图标地址与它在候选列表中的位置 */
const known = new Map<string, { url: string; index: number }>();

function candidates(url: string): string[] {
  const host = HOSTS.find((h) => url.startsWith(h + "/"));
  const mirror = host ? MCIM + url.slice(host.length) : null;
  const list = iconPrefs.official ? [url, mirror] : [mirror, url];
  return [...list.filter((u): u is string => !!u), convertFileSrc(url, "rcicon")];
}

function remember(url: string, index: number): string {
  const list = candidates(url);
  const src = list[Math.min(index, list.length - 1)];
  known.set(src, { url, index });
  return src;
}

/** 手机版的图标地址（第一个候选地址）。 */
export function mobileIconSrc(url: string): string {
  return remember(url, 0);
}

/** 换用下一个候选地址；已经是最后一个时返回 false。 */
function advance(img: HTMLImageElement): boolean {
  const k = known.get(img.getAttribute("src") ?? "");
  if (!k) return false;
  const list = candidates(k.url);
  if (k.index + 1 >= list.length) return false;
  img.src = remember(k.url, k.index + 1);
  watch(img);
  return true;
}

const timers = new WeakMap<HTMLImageElement, number>();

/** 地址长时间没有加载完成（网络不通时可能要等很久才报错）时换下一个。 */
function watch(img: HTMLImageElement) {
  clearTimeout(timers.get(img));
  const src = img.getAttribute("src");
  if (!src || !known.has(src)) return;
  timers.set(
    img,
    window.setTimeout(() => {
      timers.delete(img);
      if (img.isConnected && img.getAttribute("src") === src && !(img.complete && img.naturalWidth > 0)) advance(img);
    }, STALL_MS),
  );
}

function scan(node: Node) {
  if (node instanceof HTMLImageElement) watch(node);
  else if (node instanceof Element) node.querySelectorAll("img").forEach(watch);
}

/** 安装加载失败的回退与超时检查（手机版启动时调用一次）。 */
export function installIconFallback() {
  // error 事件不冒泡，在捕获阶段统一处理
  document.addEventListener(
    "error",
    (e) => {
      if (e.target instanceof HTMLImageElement) advance(e.target);
    },
    true,
  );
  document.addEventListener(
    "load",
    (e) => {
      if (e.target instanceof HTMLImageElement) clearTimeout(timers.get(e.target));
    },
    true,
  );
  new MutationObserver((records) => {
    for (const r of records) {
      if (r.type === "attributes") scan(r.target);
      else r.addedNodes.forEach(scan);
    }
  }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["src"] });
}
