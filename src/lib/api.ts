// 与 Rust 后端通信：类型定义 + command 封装 + 事件订阅。

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DownloadSource = "auto" | "official" | "bmclapi";
export type Isolation = "off" | "modded" | "nonRelease" | "moddedAndNonRelease" | "all";
export type AfterLaunch = "keep" | "hide" | "close";
export type Theme = "system" | "light" | "dark";

export interface Settings {
  gameDir: string | null;
  downloadSource: DownloadSource;
  concurrency: number;
  isolation: Isolation;
  javaPath: string | null;
  autoDownloadJava: boolean;
  memoryAuto: boolean;
  maxMemoryMb: number;
  minMemoryMb: number;
  jvmArgs: string;
  gameArgs: string;
  customResolution: boolean;
  windowWidth: number;
  windowHeight: number;
  fullscreen: boolean;
  server: string;
  afterLaunch: AfterLaunch;
  theme: Theme;
  accent: string;
  selectedInstance: string | null;
  selectedAccount: string | null;
  msClientId: string;
  defaultChinese: boolean;
  curseforgeApiKey: string;
}

export interface AppInfo {
  name: string;
  version: string;
  portable: boolean;
  configDir: string;
  gameDir: string;
  defaultGameDir: string;
  memory: { totalMb: number; availableMb: number };
  os: string;
  arch: string;
}

export interface VersionItem {
  id: string;
  kind: "release" | "snapshot" | "old_beta" | "old_alpha" | string;
  releaseTime: string;
}

export interface VersionList {
  latestRelease: string;
  latestSnapshot: string;
  versions: VersionItem[];
}

export type LoaderKind = "fabric" | "quilt" | "forge" | "neoforge";

export interface InstanceInfo {
  id: string;
  mcVersion: string;
  kind: string;
  loader: { kind: LoaderKind; version: string } | null;
  releaseTime: string;
  error: string | null;
  isolated: boolean;
  gameDir: string;
  lastPlayed: number | null;
  running: boolean;
}

export interface LoaderVersion {
  version: string;
  display: string;
  stable: boolean;
}

export type Platform = "modrinth" | "curseforge";
export type ContentType = "mod" | "resourcepack" | "shader" | "modpack";
export type SortBy = "relevance" | "downloads" | "updated" | "newest";

export interface SearchQuery {
  query: string;
  contentType: ContentType;
  gameVersion: string | null;
  loader: LoaderKind | null;
  sort: SortBy;
  offset: number;
  limit: number;
}

export interface ProjectHit {
  platform: Platform;
  id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  iconUrl: string | null;
  downloads: number;
  categories: string[];
  contentType: ContentType;
  updated: string;
}

export interface ContentFile {
  url: string;
  filename: string;
  sha1: string | null;
  size: number;
  primary: boolean;
}

export interface ContentVersion {
  platform: Platform;
  id: string;
  projectId: string;
  name: string;
  versionNumber: string;
  gameVersions: string[];
  loaders: string[];
  releaseType: string;
  published: string;
  downloads: number;
  files: ContentFile[];
  dependencies: { projectId: string | null; versionId: string | null; kind: string }[];
}

export interface PlanItem {
  projectId: string;
  title: string;
  version: ContentVersion;
  installed: boolean;
  requiredBy: string | null;
}

export interface MissingDep {
  projectId: string;
  title: string;
  requiredBy: string;
  reason: string;
}

export interface Plan {
  items: PlanItem[];
  optional: PlanItem[];
  incompatible: MissingDep[];
  missing: MissingDep[];
}

export interface InstanceContext {
  id: string;
  mcVersion: string;
  loader: LoaderKind | null;
  gameDir: string;
}

export interface VersionRef {
  projectId: string;
  versionId: string;
}

export interface InstanceConfig {
  mcVersion: string | null;
  isolation: boolean | null;
  javaPath: string | null;
  maxMemoryMb: number | null;
  jvmArgs: string | null;
  server: string | null;
  lastPlayed: number | null;
}

export type AccountKind =
  | { type: "offline" }
  | { type: "microsoft" }
  | { type: "yggdrasil"; server: string; serverName: string };

export interface Account {
  id: string;
  kind: AccountKind;
  name: string;
  uuid: string;
  skin?: string | null;
}

export interface MsLoginStart {
  flowId: number;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

export interface JavaInstall {
  path: string;
  version: string;
  major: number;
  arch: string;
}

export interface Progress {
  totalFiles: number;
  doneFiles: number;
  failedFiles: number;
  totalBytes: number;
  doneBytes: number;
  speed: number;
}

export interface ErrorInfo {
  message: string;
  hint: string;
}

export type TaskState = "running" | "done" | "failed" | "cancelled";

export interface TaskEvent {
  id: number;
  title: string;
  stage: string;
  progress: Progress | null;
  state: TaskState;
  error: ErrorInfo | null;
}

export interface LogLine {
  text: string;
  stderr: boolean;
}

export interface GameExit {
  code: number | null;
  crashed: boolean;
  crashReport: string | null;
  causes: { reason: string; solution: string }[];
  tail: string[];
}

export type GameEvent =
  | { type: "spawned"; instance: string; pid: number; command: string }
  | { type: "started"; instance: string }
  | { type: "log"; instance: string; lines: LogLine[] }
  | { type: "exit"; instance: string; exit: GameExit };

/** 后端返回的错误（中文原因 + 解决办法）。 */
export class AppError extends Error {
  hint: string;
  constructor(info: ErrorInfo) {
    super(info.message);
    this.hint = info.hint;
  }
}

function toError(e: unknown): AppError {
  if (e instanceof AppError) return e;
  if (e && typeof e === "object" && "message" in e) {
    const o = e as { message: string; hint?: string };
    return new AppError({ message: o.message, hint: o.hint ?? "" });
  }
  return new AppError({ message: String(e), hint: "" });
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toError(e);
  }
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  appReady: () => call<void>("app_ready"),
  getSettings: () => call<Settings>("get_settings"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),

  versionList: (refresh = false) => call<VersionList>("version_list", { refresh }),

  listInstances: () => call<InstanceInfo[]>("list_instances"),
  loaderVersions: (kind: LoaderKind, mc: string) => call<LoaderVersion[]>("loader_versions", { kind, mc }),
  installInstance: (
    version: string,
    name: string,
    loader: { kind: LoaderKind; version: string } | null,
    withApi = true,
  ) => call<number>("install_instance", { version, name, loader, withApi }),
  repairInstance: (id: string) => call<number>("repair_instance", { id }),
  deleteInstance: (id: string) => call<void>("delete_instance", { id }),
  getInstanceConfig: (id: string) => call<InstanceConfig>("get_instance_config", { id }),
  saveInstanceConfig: (id: string, config: InstanceConfig) =>
    call<void>("save_instance_config", { id, config }),
  selectInstance: (id: string | null) => call<void>("select_instance", { id }),
  openInstanceDir: (id: string, sub = "root") => call<void>("open_instance_dir", { id, sub }),
  openGameDir: () => call<void>("open_game_dir"),
  openFile: (path: string) => call<void>("open_file", { path }),
  readGameFile: (path: string) => call<string>("read_game_file", { path }),

  listAccounts: () => call<{ accounts: Account[]; selected: string | null }>("list_accounts"),
  addOfflineAccount: (name: string) =>
    call<{ account: Account; warning: string | null }>("add_offline_account", { name }),
  removeAccount: (id: string) => call<void>("remove_account", { id }),
  refreshOfflineSkin: (id: string) => call<Account>("refresh_offline_skin", { id }),
  selectAccount: (id: string) => call<void>("select_account", { id }),

  msLoginStart: () => call<MsLoginStart>("ms_login_start"),
  msLoginFinish: (flowId: number) => call<Account>("ms_login_finish", { flowId }),
  msLoginCancel: (flowId: number) => call<void>("ms_login_cancel", { flowId }),
  msRefreshAccount: (id: string) => call<Account>("ms_refresh_account", { id }),

  contentSearch: (platform: Platform, query: SearchQuery) =>
    call<{ hits: ProjectHit[]; total: number }>("content_search", { platform, query }),
  contentInstance: (id: string) => call<InstanceContext>("content_instance", { id }),
  contentVersions: (platform: Platform, projectId: string, contentType: ContentType, instance: string | null) =>
    call<ContentVersion[]>("content_versions", { platform, projectId, contentType, instance }),
  contentProject: (platform: Platform, projectId: string) => call<ProjectHit>("content_project", { platform, projectId }),
  contentPlan: (platform: Platform, instance: string, contentType: ContentType, roots: VersionRef[]) =>
    call<Plan>("content_plan", { platform, instance, contentType, roots }),
  contentInstall: (platform: Platform, instance: string, contentType: ContentType, roots: VersionRef[]) =>
    call<number>("content_install", { platform, instance, contentType, roots }),

  scanJava: (refresh = false) => call<JavaInstall[]>("scan_java", { refresh }),
  probeJava: (path: string) => call<JavaInstall>("probe_java", { path }),

  cancelTask: (id: number) => call<boolean>("cancel_task", { id }),
  launch: (id: string) => call<number>("launch_instance", { id }),
  killGame: (id: string) => call<void>("kill_game", { id }),
};

export function onTask(cb: (e: TaskEvent) => void): Promise<UnlistenFn> {
  return listen<TaskEvent>("task", (e) => cb(e.payload));
}

export function onGame(cb: (e: GameEvent) => void): Promise<UnlistenFn> {
  return listen<GameEvent>("game", (e) => cb(e.payload));
}
