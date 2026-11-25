// 联机（陶瓦联机 Terracotta）：命令封装、状态类型与房间号校验。

import { invoke } from "@tauri-apps/api/core";

export interface MultiplayerInfo {
  available: boolean;
  /** macOS：需要先安装陶瓦官方安装包 */
  needsInstall: boolean;
  reason: string | null;
  version: string;
}

export interface RoomMember {
  machine_id: string;
  name: string;
  vendor: string;
  /** HOST 房主 / LOCAL 本机 / GUEST 房客 */
  kind: "HOST" | "LOCAL" | "GUEST";
}

/** 陶瓦 `/state` 的返回；`stopped` 表示联机组件没有运行 */
export type MultiplayerState =
  | { state: "stopped" | "waiting" | "host-scanning" | "guest-connecting"; index?: number; room?: string }
  | { state: "host-starting"; index: number; room: string }
  | { state: "host-ok"; index: number; room: string; profiles: RoomMember[] }
  | { state: "guest-starting"; index: number; room: string; difficulty: "UNKNOWN" | "EASIEST" | "SIMPLE" | "MEDIUM" | "TOUGH" }
  | { state: "guest-ok"; index: number; url: string; profiles: RoomMember[] }
  | { state: "exception"; index: number; type: number };

export const multiplayerApi = {
  info: () => invoke<MultiplayerInfo>("multiplayer_info"),
  state: () => invoke<MultiplayerState>("multiplayer_state"),
  host: (player: string | null) => invoke<void>("multiplayer_host", { player }),
  join: (room: string, player: string | null) => invoke<string>("multiplayer_join", { room, player }),
  leave: () => invoke<void>("multiplayer_leave"),
  install: () => invoke<void>("multiplayer_install"),
  /** 分析最近一次联机失败的原因 */
  diagnose: () => invoke<Diagnosis>("multiplayer_diagnose"),
  /** 导出陶瓦日志到启动器的 logs 文件夹并打开，返回文件路径 */
  exportLog: () => invoke<string>("multiplayer_export_log"),
  /** 加入房间后与房主之间的连接方式；查不到时为 null */
  link: () => invoke<Link | null>("multiplayer_link"),
};

export interface Link {
  ipVersion: "IPv4" | "IPv6" | null;
  /** 对方是局域网地址 */
  lan: boolean;
  /** 隧道协议：tcp / udp / quic 等 */
  protocol: string;
  latencyMs: number | null;
  /** 经过其他成员转发，没有与房主直连 */
  relayed: boolean;
}

/** 连接方式的文字说明，例如「IPv6 直连 · TCP · 延迟 24 ms」 */
export function describeLink(link: Link): string {
  const parts: string[] = [];
  if (link.relayed) parts.push("经其他成员转发");
  else parts.push(`${link.ipVersion ?? "未知网络"} ${link.lan ? "局域网直连" : "直连"}`);
  if (link.protocol && !link.relayed) parts.push(link.protocol.toUpperCase());
  if (link.latencyMs != null) parts.push(`延迟 ${Math.round(link.latencyMs)} ms`);
  return parts.join(" · ");
}

export interface Diagnosis {
  /** 本机无法在房主的游戏端口上建立转发（Windows 上多为该端口被系统保留） */
  portBlocked: boolean;
  port: number | null;
}

const CHARS = "0123456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/**
 * 规范化并校验房间号（与陶瓦 / Scaffolding 协议一致）：`U/XXXX-XXXX-XXXX-XXXX`，
 * 可省略 `U/`，不区分大小写，I/O 视为 1/0；按 34 进制（第一位为最低位）换算后须能被 7 整除。
 * 不合法时返回 null。
 */
export function normalizeRoom(input: string): string | null {
  const text = input.replace(/\s+/g, "").toUpperCase();
  const body = (text.startsWith("U/") ? text.slice(2) : text).replace(/I/g, "1").replace(/O/g, "0");
  if (!/^[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}$/.test(body)) return null;
  let value = 0n;
  for (let i = body.length - 1; i >= 0; i--) {
    if (body[i] === "-") continue;
    const digit = CHARS.indexOf(body[i]);
    if (digit < 0) return null;
    value = value * 34n + BigInt(digit);
  }
  return value % 7n === 0n ? `U/${body}` : null;
}

export const DIFFICULTY: Record<string, string> = {
  UNKNOWN: "正在判断网络状况",
  EASIEST: "网络状况很好，马上就能连上",
  SIMPLE: "网络状况良好",
  MEDIUM: "网络状况一般，可能需要稍等",
  TOUGH: "网络状况较差，可能需要较长时间，请耐心等待",
};

export const EXCEPTION: Record<number, string> = {
  0: "无法连接到房主。请确认房间号正确，并且房主的房间仍然开着。",
  1: "与房主的连接中断了。",
  2: "本机的联机组件意外退出。",
  3: "房主的联机组件意外退出。",
  4: "房主的游戏已关闭或不再对局域网开放。",
  5: "房主使用的联机程序不兼容。",
};

/** 各类联机错误的处理建议 */
export function exceptionTips(type: number, diagnosis: Diagnosis | null): string[] {
  if (type === 2 && diagnosis?.portBlocked) {
    const port = diagnosis.port ? `端口 ${diagnosis.port}` : "房主的游戏端口";
    return [
      `Windows 不允许在本机的${port}上建立转发，这个端口很可能被系统保留了（Hyper-V、WSL、Docker 等会保留一段端口）。`,
      "请房主退出存档后重新进入，在「对局域网开放」中把「端口号」改为 25565，再创建局域网世界和房间，然后重新加入。",
      "也可以在本机以管理员身份打开命令提示符，依次执行 net stop winnat 和 net start winnat 释放保留的端口（可能让 WSL、Docker 的网络暂时断开）。",
    ];
  }
  switch (type) {
    case 0:
      return ["确认房间号没有输错，并且房主仍在游戏中、房间没有关闭。", "双方网络都比较严格时可能无法直连，可以换一个网络环境或开启 IPv6 后重试。"];
    case 1:
      return ["可能是网络波动，请返回后重新加入。"];
    case 2:
      return ["请完全退出启动器后重新打开再加入。", "仍然失败时请点「导出联机日志」，把日志发给开发者排查。"];
    case 3:
    case 4:
      return ["请房主重新「对局域网开放」并创建房间，把新的房间号发给你。"];
    case 5:
      return ["请房主把启动器或陶瓦联机更新到最新版本。"];
    default:
      return ["请返回后重试；仍然失败时请导出联机日志。"];
  }
}
