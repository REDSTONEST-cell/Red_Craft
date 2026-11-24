import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export interface Appearance {
  selection: string | null;
  fit: "cover" | "contain";
  brightness: number;
  backgroundOpacity: number;
  blurPx: number;
  overlayOpacity: number;
  panelOpacity: number;
  /** auto：自动播放，系统要求减少动态效果时暂停；always：始终播放；static：静态 */
  motion: "auto" | "always" | "static";
  /** 场景实时渲染的最大帧率 */
  fps: number;
}
export interface BackgroundAsset {
  id: string; title: string; source: "local" | "wallpaperEngine";
  kind: string; media: string | null; cover: string | null; reason: string | null;
  root?: string;
  /** 不影响使用的说明，例如「应用程序壁纸不会在启动器中运行，已使用预览图」 */
  scanWarning?: string | null;
}
/** Wallpaper Engine 的壁纸类型 */
export const WE_KINDS = ["video", "scene", "web", "image", "application"] as const;
export const KIND_LABEL: Record<string, string> = {
  video: "视频", scene: "场景", web: "网页", image: "图片", application: "应用程序",
};
export const kindLabel = (kind: string) => KIND_LABEL[kind] ?? kind;
export interface CropRect { x: number; y: number; width: number; height: number }
/** 本地素材与用户自己的 Wallpaper Engine 壁纸（任何类型）；屏蔽官方自带的 defaultprojects。 */
export function isVisibleBackgroundAsset(asset: BackgroundAsset): boolean {
  return asset.source === "local" || !/(^|[\\/])defaultprojects([\\/]|$)/i.test(asset.root ?? "");
}
export interface BackgroundLibrary {
  crops?: Record<string, CropRect>;
  /** 改用预览图显示的场景壁纸 */
  scenePreview?: string[];
  appearance: Appearance; assets: BackgroundAsset[]; roots: string[]; scanNotes: string[];
}
/** kind 是显示方式：image / video / web（场景与应用程序壁纸以图片显示）。 */
export interface ResolvedBackground {
  id: string; kind: string; media: string | null; cover: string | null;
  /** 网页壁纸入口（相对壁纸目录，已编码） */
  entry?: string | null;
  /** Windows：WebView2 虚拟主机名；为空时使用 rcwall 协议 */
  host?: string | null;
  /** 背景静态画面的平均亮度 0..1（没有静态画面时为空） */
  luminance?: number | null;
}

/**
 * 背景上的文字该用白字还是黑字：把背景画面的亮度按「亮度 × 不透明度 + 主题底色」
 * 混合，再压上黑色遮罩，得到实际显示出来的明暗。足够亮时用黑字，否则用白字。
 */
export function textOnBackground(luminance: number | null | undefined, a: Appearance, darkTheme: boolean): "light" | "dark" {
  if (luminance == null || !Number.isFinite(luminance)) return "light";
  const base = darkTheme ? 0.09 : 0.93;
  const image = Math.min(1, Math.max(0, luminance) * a.brightness);
  const shown = (image * a.backgroundOpacity + base * (1 - a.backgroundOpacity)) * (1 - a.overlayOpacity);
  return shown > 0.58 ? "dark" : "light";
}
// 对外扫描 DTO 使用需求约定的 snake_case；存储格式保留兼容。
export interface WallpaperEntry {
  id: string;
  source: "local" | "wallpaper_engine";
  kind: string;
  title: string;
  file_path: string | null;
  thumbnail_path: string | null;
}
export interface WallpaperScanResult {
  entries: WallpaperEntry[];
  warnings: string[];
  from_cache: boolean;
}
export const defaults = (): Appearance => ({ selection: null, fit: "cover", brightness: 0.85, backgroundOpacity: 0.75,
  blurPx: 0, overlayOpacity: 0.25, panelOpacity: 0.94, motion: "auto", fps: 30 });
/** 场景最大帧率可选项 */
export const FPS_OPTIONS = [15, 24, 30, 45, 60, 90, 120, 144];
export const mediaUrl = (path: string | null) => path ? convertFileSrc(path) : "";
/** 网页壁纸地址：由后端的 rcwall 协议按「素材 ID / 相对路径」提供项目内文件。 */
/** 场景实时渲染页（WebWallGL），由后端的 rcwall 协议提供；与启动器不同源。 */
export const sceneUrl = (id: string, fit: string, fps: number) =>
  `${convertFileSrc("", "rcwall")}__engine/scene.html?${new URLSearchParams({ id, fit, fps: String(fps) })}`;
export const webUrl = (id: string, entry: string, host?: string | null) =>
  host ? `https://${host}/${entry}` : `${convertFileSrc("", "rcwall")}${encodeURIComponent(id)}/${entry}`;
export const backgroundApi = {
  list: () => invoke<BackgroundLibrary>("background_list"),
  resolve: (id: string) => invoke<ResolvedBackground>("background_resolve", { id }),
  apply: (appearance: Appearance) => invoke<Appearance>("background_apply", { appearance }),
  import: (path: string) => invoke<number>("background_import", { path }),
  crop: (id: string, rect: CropRect | null) => invoke<void>("background_crop", { id, rect }),
  original: (id: string) => invoke<ResolvedBackground>("background_resolve", { id, original: true }),
  scan: (roots: string[], forceRefresh = false) => invoke<WallpaperScanResult>("background_scan", { roots, forceRefresh }),
  thumbnail: (id: string) => invoke<string | null>("background_thumbnail", { id }),
  remove: (id: string) => invoke<void>("background_remove", { id }),
  sceneMode: (id: string, preview: boolean) => invoke<void>("background_scene_mode", { id, preview }),
  visible: () => invoke<boolean>("background_window_state"),
};

export function shouldPlay(visible: boolean, motion: Appearance["motion"], reduced: boolean): boolean {
  return visible && (motion === "always" || (motion === "auto" && !reduced));
}

/** 游戏运行期间，启动器不在前台时暂停动态背景：视频解码会和游戏争用显卡与 CPU。回到启动器时继续播放。 */
export function yieldsToGame(gameRunning: boolean, focused: boolean): boolean {
  return gameRunning && !focused;
}

export function releaseVideo(node: HTMLVideoElement): void {
  node.pause();
  node.removeAttribute("src");
  node.load();
}

export function controlVideo(node: HTMLVideoElement, enabled: boolean, onError: (e: unknown) => void): () => void {
  let active = true;
  node.muted = true;
  node.defaultMuted = true;
  node.volume = 0;
  if (enabled) {
    void node.play().catch((e: unknown) => {
      if (active && !(e instanceof DOMException && e.name === "AbortError")) onError(e);
    });
  } else node.pause();
  return () => { active = false; node.pause(); };
}

export function observeVisibility(source: {
  query: () => Promise<boolean>;
  subscribe: (receive: (visible: boolean) => void) => Promise<() => void>;
  hidden: () => boolean;
  change: (visible: boolean) => void;
}): { refresh: () => Promise<void>; dispose: () => void } {
  let disposed = false;
  let revision = 0;
  let unlisten: (() => void) | undefined;
  const refresh = async () => {
    const current = ++revision;
    try {
      const visible = await source.query();
      if (!disposed && current === revision) source.change(visible && !source.hidden());
    } catch {
      if (!disposed && current === revision) source.change(false);
    }
  };
  void source.subscribe(visible => {
    if (disposed) return;
    revision++; // 原生事件比已发出的 IPC 查询更新。
    source.change(visible && !source.hidden());
  }).then(stop => {
    if (disposed) stop();
    else { unlisten = stop; void refresh(); }
  }).catch(() => { if (!disposed) void refresh(); });
  return {
    refresh,
    dispose() { disposed = true; revision++; unlisten?.(); },
  };
}
