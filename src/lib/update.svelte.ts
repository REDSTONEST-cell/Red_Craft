// 启动器自身的更新：检查到新版本后弹出确认框，用户确认后才下载并安装。

import { api, type UpdateInfo } from "./api";
import { app, saveSettings, showError, toast, waitTask } from "./state.svelte";

export const updater = $state({
  info: null as UpdateInfo | null,
  /** 确认框是否打开 */
  open: false,
  checking: false,
  /** 下载任务 id */
  taskId: null as number | null,
  error: "",
  busy: false,
  downloaded: false,
});

/**
 * 检查更新。`manual` 为 true 时（设置页点击）没有新版本也会提示，并忽略「跳过此版本」。
 */
export async function checkUpdate(manual: boolean) {
  if (updater.checking || updater.busy) return;
  updater.checking = true;
  try {
    const info = await api.checkUpdate();
    if (!info) {
      if (manual) toast("ok", "已是最新版本", `当前版本 ${(window as unknown as { RclGame?: { appVersion?: () => string } }).RclGame?.appVersion?.() ?? app.info?.version ?? ""}`);
      return;
    }
    if (!manual && app.settings?.skippedUpdate === info.version) return;
    if (updater.info?.version !== info.version) updater.downloaded = false;
    updater.info = info;
    updater.error = "";
    updater.open = true;
  } catch (e) {
    // 自动检查失败（例如没有网络）时不打扰用户
    if (manual) showError(e, "检查更新失败");
  } finally {
    updater.checking = false;
  }
}

export async function skipVersion() {
  if (updater.info) await saveSettings({ skippedUpdate: updater.info.version });
  updater.open = false;
}

/** 用户确认后：下载、校验，完成后自动安装并重启。 */
export async function downloadAndInstall() {
  if (updater.busy || updater.checking) return;
  updater.busy = true;
  updater.error = "";
  try {
    if (updater.downloaded && updater.info?.kind === "apk") {
      await installDownloaded();
      return;
    }
    const id = await api.downloadUpdate();
    updater.taskId = id;
    const t = await waitTask(id);
    updater.taskId = null;
    if (t.state === "done") {
      updater.downloaded = true;
      await installDownloaded();
    } else if (t.state === "failed") {
      updater.error = t.error?.message ?? "下载失败";
      updater.open = true;
    }
  } catch (e) {
    updater.taskId = null;
    updater.error = (e as { message?: string }).message ?? String(e);
    updater.open = true;
  } finally {
    updater.busy = false;
  }
}

async function installDownloaded() {
  await api.applyUpdate();
  if (updater.info?.kind === "apk") {
    const { installUpdate } = await import("./androidFiles");
    await installUpdate();
    toast("ok", "已打开系统安装界面", "请在系统界面确认更新；取消后可再次点击安装。");
  }
}
