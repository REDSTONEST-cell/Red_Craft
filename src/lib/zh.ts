// 资源的中文显示：图标走后端（镜像加速 + 缓存）、常见 Mod 的中文名、中文关键词搜索。
import { convertFileSrc } from "@tauri-apps/api/core";
import names from "./zh-names.json";

/** 资源图标地址：交给后端的 rcicon 协议，同时尝试官方 CDN 与 MCIM 镜像，并缓存到本地。 */
export function iconSrc(url: string | null | undefined): string {
  if (!url) return "";
  if (!/^https:\/\/(cdn\.modrinth\.com|media\.forgecdn\.net|edge\.forgecdn\.net|mediafilez\.forgecdn\.net)\//.test(url)) return url;
  return convertFileSrc(url, "rcicon");
}

// 常见 Mod 的中文名（社区通用译名）：[英文名, 中文名, ...别名（slug 或其他写法）]，
// 英文名同时用作中文搜索时的英文关键词。
// 收录在 zh-names.json 中，后端给 Mod 文件名加中文名时也使用这份表。
const ENTRIES = names as [string, string, ...string[]][];

function key(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]/g, "");
}

const NAMES: Record<string, string> = {};
for (const [en, zh, ...aliases] of ENTRIES) {
  for (const k of [en, ...aliases]) NAMES[key(k)] = zh;
}

/** 返回资源的中文名；没有收录时返回 null。 */
export function zhName(title: string, slug?: string | null): string | null {
  return (slug && NAMES[key(slug)]) || NAMES[key(title)] || null;
}

const hasCjk = (s: string) => /[㐀-鿿]/.test(s);

/**
 * 中文关键词搜索：Modrinth / CurseForge 只能用英文搜索，
 * 这里把收录的中文名换成对应的英文关键词。返回 null 表示无需转换。
 */
export function zhQuery(query: string): { query: string; from: string } | null {
  const q = query.trim();
  if (!q || !hasCjk(q)) return null;
  let best: [string, string] | null = null;
  for (const [en, zh] of ENTRIES) {
    if (zh === q) return { query: en, from: zh };
    // 部分匹配时取最短（最精确）的中文名
    if ((zh.includes(q) || q.includes(zh)) && (!best || zh.length < best[1].length)) best = [en, zh];
  }
  return best ? { query: best[0], from: best[1] } : null;
}
