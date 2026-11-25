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
};

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
