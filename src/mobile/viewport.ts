// 手机版自动缩放与安全区域。
//
// 缩放：以短边 360 像素为基准，按 WebView 实际可用的尺寸整体缩放页面，小屏手机缩小、平板放大，
// 各种屏幕上的比例保持一致。需要 WebView 开启宽视口（Android 端 MainActivity 中设置 useWideViewPort）。
//
// 尺寸与挖孔区域由 Android 端的 RclDisplay 提供（单位 dp，即页面未缩放时的 CSS 像素）：
// 窗口铺满整个屏幕（包括挖孔所在的一侧），页面背景画到屏幕边缘，可交互的内容按
// --m-safe-l/t/r/b 避开挖孔、曲面边缘与系统栏，旋转 180° 后挖孔换到另一侧时自动跟随。
// 不能用 screen.width/height：挖孔、分屏、无法隐藏的导航栏都会让 WebView 比屏幕小。

const BASE = 360;
const MIN_SCALE = 0.8;
const MAX_SCALE = 1.6;

interface DisplayInfo {
  /** 可用宽高（dp），不含输入法 */
  w: number;
  h: number;
  /** 四边需要避让的宽度（dp） */
  l: number;
  t: number;
  r: number;
  b: number;
  /** 屏幕圆角半径（dp） */
  corner: number;
}

declare global {
  interface Window {
    RclDisplay?: { info(): string };
    __rclDisplay?: (d: DisplayInfo) => void;
  }
}

function nativeInfo(): DisplayInfo | null {
  try {
    const raw = window.RclDisplay?.info();
    if (!raw) return null;
    const d = JSON.parse(raw) as DisplayInfo;
    return d.w > 0 && d.h > 0 ? d : null;
  } catch {
    return null;
  }
}

/** 没有原生信息时（浏览器预览）：当前可见区域乘以页面缩放，换算回未缩放的尺寸。 */
function pageInfo(): DisplayInfo {
  const vv = window.visualViewport;
  const z = vv?.scale || 1;
  return {
    w: (vv?.width ?? window.innerWidth) * z,
    h: (vv?.height ?? window.innerHeight) * z,
    l: 0,
    t: 0,
    r: 0,
    b: 0,
    corner: 0,
  };
}

export function fitViewport() {
  let meta = document.querySelector<HTMLMetaElement>('meta[name="viewport"]');
  if (!meta) {
    meta = document.createElement("meta");
    meta.name = "viewport";
    document.head.appendChild(meta);
  }
  const root = document.documentElement;
  let native: DisplayInfo | null = null;
  let last = "";

  const apply = () => {
    const d = native ?? nativeInfo() ?? pageInfo();
    if (!(d.w > 0 && d.h > 0)) return;
    const scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, Math.min(d.w, d.h) / BASE));
    // 向下取整：宽度不足时浏览器会自动补齐到可见宽度，向上取整则会多出一点被裁掉
    const width = Math.floor(d.w / scale);
    const content = `width=${width}, initial-scale=${scale}, minimum-scale=${scale}, maximum-scale=${scale}, user-scalable=no, viewport-fit=cover`;
    const px = (v: number) => `${Math.round((v / scale) * 10) / 10}px`;
    const key = `${content}|${d.l},${d.t},${d.r},${d.b},${d.corner}`;
    if (key === last) return;
    last = key;
    meta!.content = content;
    root.style.setProperty("--m-scale", String(scale));
    root.style.setProperty("--m-cut-l", px(d.l));
    root.style.setProperty("--m-cut-t", px(d.t));
    root.style.setProperty("--m-cut-r", px(d.r));
    root.style.setProperty("--m-cut-b", px(d.b));
    // 圆角处内容离两条边各留出约 0.3 倍半径即可不被切掉
    root.style.setProperty("--m-corner", px(d.corner * 0.3));
  };

  // Android 端在尺寸、挖孔位置变化时主动通知
  window.__rclDisplay = (d) => {
    if (d && d.w > 0 && d.h > 0) native = d;
    apply();
  };
  apply();

  // 旋转、分屏等变化：Android 端会通知，这里再主动读几次作为保险
  const refresh = () => {
    native = nativeInfo() ?? native;
    apply();
  };
  const later = () => {
    refresh();
    setTimeout(refresh, 150);
    setTimeout(refresh, 600);
  };
  screen.orientation?.addEventListener("change", later);
  // 没有原生信息时，按页面尺寸的变化重新计算
  window.addEventListener("resize", () => {
    if (!native && !window.RclDisplay) apply();
  });
}
