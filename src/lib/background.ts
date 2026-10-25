import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export interface Appearance {
  selection: string | null;
  fit: "cover" | "contain";
  brightness: number;
  backgroundOpacity: number;
  blurPx: number;
  overlayOpacity: number;
  panelOpacity: number;
  motion: "auto" | "static";
}
export interface BackgroundAsset {
  id: string; title: string; source: "local" | "wallpaperEngine";
  kind: string; media: string | null; cover: string | null; reason: string | null;
  root?: string;
}
export function isVisibleBackgroundAsset(asset: BackgroundAsset): boolean {
  return asset.source === "local" || (asset.kind === "video" && !!asset.media
    && !/(^|[\\/])defaultprojects([\\/]|$)/i.test(asset.root ?? ""));
}
export interface BackgroundLibrary {
  appearance: Appearance; assets: BackgroundAsset[]; roots: string[]; scanNotes: string[];
}
export interface ResolvedBackground { id: string; kind: string; media: string | null; cover: string | null }
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
  blurPx: 0, overlayOpacity: 0.25, panelOpacity: 0.94, motion: "auto" });
export const mediaUrl = (path: string | null) => path ? convertFileSrc(path) : "";
export const backgroundApi = {
  list: () => invoke<BackgroundLibrary>("background_list"),
  resolve: (id: string) => invoke<ResolvedBackground>("background_resolve", { id }),
  apply: (appearance: Appearance) => invoke<Appearance>("background_apply", { appearance }),
  import: (path: string) => invoke<number>("background_import", { path }),
  scan: (roots: string[], forceRefresh = false) => invoke<WallpaperScanResult>("background_scan", { roots, forceRefresh }),
  thumbnail: (id: string) => invoke<string | null>("background_thumbnail", { id }),
  remove: (id: string) => invoke<void>("background_remove", { id }),
  visible: () => invoke<boolean>("background_window_state"),
};

export function shouldPlay(visible: boolean, motion: Appearance["motion"], reduced: boolean): boolean {
  return visible && motion === "auto" && !reduced;
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
