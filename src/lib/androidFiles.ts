// 手机版的文件导入导出：通过 Android 原生的 RclGame 接口调用系统文件选择器。
// 系统返回的是 content:// 地址，启动器无法按路径读写，所以由原生层先把选中的文件复制到
// 应用缓存目录，再把本地路径交给启动器导入；导出时把打包好的文件交给系统保存到用户选择的位置。

type Result = { paths?: string[]; ok?: boolean; error?: string; cancelled?: boolean };

interface Host {
  installUpdate(id: string): void;
  pickFiles(id: string, multiple: boolean): void;
  pickFolder(id: string): void;
  saveFile(id: string, path: string, name: string, mime: string): void;
}

/** 打开已校验的更新 APK；权限拒绝或安装取消后可以重试。 */
export async function installUpdate(): Promise<void> {
  const r = await call((h, id) => h.installUpdate(id));
  if (r.cancelled) throw new Error("尚未允许安装，请点击安装重试");
}

const pending = new Map<string, (r: Result) => void>();
let seq = 0;

(window as unknown as { __rclFiles: (id: string, r: Result) => void }).__rclFiles = (id, r) => {
  pending.get(id)?.(r);
  pending.delete(id);
};

function call(run: (host: Host, id: string) => void): Promise<Result> {
  const host = (window as unknown as { RclGame?: Host }).RclGame;
  if (!host) return Promise.reject(new Error("当前环境不支持系统文件选择"));
  const id = `f${++seq}`;
  return new Promise<Result>((resolve) => {
    pending.set(id, resolve);
    run(host, id);
  }).then((r) => {
    if (r.error) throw new Error(r.error);
    return r;
  });
}

/** 选择文件，返回复制到缓存目录后的路径；取消时返回空数组。 */
export async function pickFiles(multiple = true): Promise<string[]> {
  const r = await call((h, id) => h.pickFiles(id, multiple));
  return r.paths ?? [];
}

/** 选择文件夹，返回复制到缓存目录后的路径；取消时返回 null。 */
export async function pickFolder(): Promise<string | null> {
  const r = await call((h, id) => h.pickFolder(id));
  return r.paths?.[0] ?? null;
}

/** 让用户选择位置保存文件；保存成功返回 true，取消返回 false。 */
export async function saveFile(path: string, name: string, mime: string): Promise<boolean> {
  const r = await call((h, id) => h.saveFile(id, path, name, mime));
  return r.ok === true;
}
