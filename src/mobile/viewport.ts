// 手机版自动缩放：以横屏短边 360 像素为基准，按实际屏幕整体缩放页面，
// 小屏手机缩小、平板放大，各种屏幕上的比例保持一致。
// 需要 WebView 开启宽视口（Android 端 MainActivity 中设置 useWideViewPort）。

const BASE = 360;
const MIN_SCALE = 0.8;
const MAX_SCALE = 1.6;

export function fitViewport() {
  let meta = document.querySelector<HTMLMetaElement>('meta[name="viewport"]');
  if (!meta) {
    meta = document.createElement("meta");
    meta.name = "viewport";
    document.head.appendChild(meta);
  }
  const update = () => {
    // screen 的尺寸不受页面缩放影响；启动器固定横屏，长边为宽
    const w = Math.max(screen.width, screen.height);
    const h = Math.min(screen.width, screen.height);
    const scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, h / BASE));
    const width = Math.round(w / scale);
    meta!.content = `width=${width}, initial-scale=${scale}, minimum-scale=${scale}, maximum-scale=${scale}, user-scalable=no, viewport-fit=cover`;
    document.documentElement.style.setProperty("--m-scale", String(scale));
  };
  update();
  screen.orientation?.addEventListener("change", update);
  window.addEventListener("resize", update);
}
