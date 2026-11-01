// 资源搜索结果与版本信息的内存缓存（参照 PCL：每类资源各自保存搜索词与结果，
// 切换标签、离开再回到资源页时直接显示已有结果，只有搜索条件变化才重新加载）。缓存只在本次运行期间有效。
import type { InstanceContext, ProjectHit, ZhMatch } from "./api";

export interface CachedSearch {
  hits: ProjectHit[];
  total: number;
  cursor: { modrinthOffset: number; curseforgeOffset: number; modrinthTotal: number | null; curseforgeTotal: number | null };
  converted: ZhMatch | null;
  alternatives: ZhMatch[];
  at: number;
}

/** 超过这个时间的缓存不再使用（长时间后重新获取，避免数据过旧）。 */
const MAX_AGE = 30 * 60 * 1000;
const MAX_ENTRIES = 48;

const searches = new Map<string, CachedSearch>();

export function getSearch(key: string): CachedSearch | null {
  const e = searches.get(key);
  if (!e) return null;
  if (Date.now() - e.at > MAX_AGE) {
    searches.delete(key);
    return null;
  }
  return e;
}

export function putSearch(key: string, entry: Omit<CachedSearch, "at"> & { at?: number }) {
  searches.delete(key);
  searches.set(key, { ...entry, at: entry.at ?? Date.now() });
  // 只保留最近使用的若干条
  while (searches.size > MAX_ENTRIES) searches.delete(searches.keys().next().value!);
}

/** 更新已缓存结果中的条目（例如补充了中文简介），不改变缓存时间。 */
export function patchSearch(key: string, hits: ProjectHit[]) {
  const e = searches.get(key);
  if (e) e.hits = hits;
}

const contexts = new Map<string, InstanceContext>();
export const getContext = (id: string) => contexts.get(id) ?? null;
export const putContext = (ctx: InstanceContext) => contexts.set(ctx.id, ctx);

/** 每类资源各自的搜索词（与 PCL 每个资源页有自己的搜索框相同）。 */
const queries = new Map<string, string>();
export const getQuery = (type: string) => queries.get(type) ?? "";
export const putQuery = (type: string, q: string) => queries.set(type, q);
