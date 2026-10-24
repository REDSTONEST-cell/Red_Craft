// 全局响应式状态（Svelte 5 runes）。

import { listen } from "@tauri-apps/api/event";
import {
  api,
  onGame,
  onTask,
  AppError,
  type Account,
  type AppInfo,
  type GameExit,
  type InstanceInfo,
  type LogLine,
  type Settings,
  type TaskEvent,
} from "./api";

export type Page = "launch" | "instances" | "download" | "mods" | "accounts" | "settings";

export interface Toast {
  id: number;
  kind: "info" | "ok" | "error";
  title: string;
  body?: string;
}

export interface GameSession {
  instance: string;
  pid: number;
  command: string;
  started: boolean;
  running: boolean;
  exit: GameExit | null;
}

const MAX_LOG_LINES = 8000;

// 日志量很大，不放进深层响应式对象：行数据存普通数组，用版本号通知界面刷新
const logLines = new Map<string, LogLine[]>();
export const logVersion = $state({ value: 0 });

export function gameLogs(instance: string): LogLine[] {
  return logLines.get(instance) ?? [];
}

export const app = $state({
  page: "launch" as Page,
  info: null as AppInfo | null,
  settings: null as Settings | null,
  instances: [] as InstanceInfo[],
  instancesLoaded: false,
  accounts: [] as Account[],
  selectedAccount: null as string | null,
  tasks: [] as TaskEvent[],
  games: {} as Record<string, GameSession>,
  toasts: [] as Toast[],
  /** 正在查看日志的实例 */
  logViewer: null as string | null,
  /** 崩溃提示 */
  crash: null as { instance: string; exit: GameExit } | null,
});

let toastId = 0;

export function toast(kind: Toast["kind"], title: string, body?: string, ms = 4200) {
  const id = ++toastId;
  app.toasts.push({ id, kind, title, body });
  setTimeout(() => dismissToast(id), kind === "error" ? Math.max(ms, 7000) : ms);
}

export function dismissToast(id: number) {
  const i = app.toasts.findIndex((t) => t.id === id);
  if (i >= 0) app.toasts.splice(i, 1);
}

/** 展示后端错误：原因 + 解决办法。 */
export function showError(e: unknown, title?: string) {
  const err = e instanceof AppError ? e : new AppError({ message: String(e), hint: "" });
  toast("error", title ?? err.message, title ? `${err.message}${err.hint ? "\n" + err.hint : ""}` : err.hint);
}

export function selectedInstance(): InstanceInfo | null {
  const id = app.settings?.selectedInstance;
  return app.instances.find((i) => i.id === id) ?? app.instances[0] ?? null;
}

export function selectedAccount(): Account | null {
  return app.accounts.find((a) => a.id === app.selectedAccount) ?? null;
}

export async function refreshInstances() {
  try {
    app.instances = await api.listInstances();
  } catch (e) {
    showError(e, "读取实例列表失败");
  } finally {
    app.instancesLoaded = true;
  }
}

export async function refreshAccounts() {
  const r = await api.listAccounts();
  app.accounts = r.accounts;
  app.selectedAccount = r.selected;
}

export async function saveSettings(patch: Partial<Settings>) {
  if (!app.settings) return;
  const next = { ...app.settings, ...patch };
  app.settings = next;
  try {
    await api.saveSettings(next);
  } catch (e) {
    showError(e, "保存设置失败");
  }
}

export async function selectInstance(id: string) {
  if (app.settings) app.settings.selectedInstance = id;
  await api.selectInstance(id).catch((e) => showError(e));
}

// ─── 任务 ───

const taskWaiters = new Map<number, (t: TaskEvent) => void>();

/** 等待任务结束。 */
export function waitTask(id: number): Promise<TaskEvent> {
  const t = app.tasks.find((t) => t.id === id);
  if (t && t.state !== "running") return Promise.resolve(t);
  return new Promise((resolve) => taskWaiters.set(id, resolve));
}

function onTaskEvent(e: TaskEvent) {
  const i = app.tasks.findIndex((t) => t.id === e.id);
  if (i >= 0) app.tasks[i] = e;
  else app.tasks.push(e);
  if (e.state !== "running") {
    taskWaiters.get(e.id)?.(e);
    taskWaiters.delete(e.id);
    if (e.state === "failed" && e.error) {
      toast("error", `${e.title}失败`, `${e.error.message}${e.error.hint ? "\n" + e.error.hint : ""}`);
    } else if (e.state === "done" && !e.title.startsWith("启动")) {
      toast("ok", `${e.title}完成`);
    }
    // 结束的任务稍后从列表移除
    setTimeout(() => {
      const j = app.tasks.findIndex((t) => t.id === e.id && t.state !== "running");
      if (j >= 0) app.tasks.splice(j, 1);
    }, e.state === "failed" ? 8000 : 2500);
  }
}

// ─── 游戏 ───

function onGameEvent(e: Parameters<Parameters<typeof onGame>[0]>[0]) {
  switch (e.type) {
    case "spawned":
      logLines.set(e.instance, []);
      logVersion.value++;
      app.games[e.instance] = {
        instance: e.instance,
        pid: e.pid,
        command: e.command,
        started: false,
        running: true,
        exit: null,
      };
      markRunning(e.instance, true);
      break;
    case "started": {
      const g = app.games[e.instance];
      if (g) g.started = true;
      break;
    }
    case "log": {
      const lines = logLines.get(e.instance);
      if (!lines) break;
      lines.push(...e.lines);
      if (lines.length > MAX_LOG_LINES) lines.splice(0, lines.length - MAX_LOG_LINES);
      logVersion.value++;
      break;
    }
    case "exit": {
      const g = app.games[e.instance];
      if (g) {
        g.running = false;
        g.exit = e.exit;
      }
      markRunning(e.instance, false);
      if (e.exit.crashed) {
        app.crash = { instance: e.instance, exit: e.exit };
      }
      break;
    }
  }
}

function markRunning(id: string, running: boolean) {
  const inst = app.instances.find((i) => i.id === id);
  if (inst) inst.running = running;
}

export async function init() {
  await Promise.all([
    onTask(onTaskEvent),
    onGame(onGameEvent),
    listen("accounts-changed", () => refreshAccounts()),
  ]);
  const [info, settings] = await Promise.all([api.appInfo(), api.getSettings()]);
  app.info = info;
  app.settings = settings;
  await Promise.all([refreshAccounts(), refreshInstances()]);
}
