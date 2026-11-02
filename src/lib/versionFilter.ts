// 资源版本的分类：按游戏版本、加载器、类型筛选资源 / 整合包的版本（电脑版与手机版的详情共用）。
import type { ContentVersion } from "./api";

export type VersionFilter = { game: string | null; loader: string | null; type: string | null };
export type FilterKey = keyof VersionFilter;

export const TYPE_LABEL: Record<string, string> = { release: "正式", beta: "测试", alpha: "早期" };

// 正式版号（如 1.21.10）按数值从新到旧；快照等其他版本号排在后面，保持出现顺序（版本列表本身按发布时间从新到旧）
const releaseLike = (v: string) => /^\d+(\.\d+)*$/.test(v);
export function compareGameVersion(a: string, b: string): number {
  const ra = releaseLike(a);
  const rb = releaseLike(b);
  if (ra !== rb) return ra ? -1 : 1;
  if (!ra) return 0;
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pb[i] ?? -1) - (pa[i] ?? -1);
    if (d) return d;
  }
  return 0;
}

/** 版本是否符合筛选；skip 指定的分类不参与判断（用于计算该分类各选项的数量） */
export function matchesFilter(v: ContentVersion, f: VersionFilter, skip?: FilterKey): boolean {
  return (
    (skip === "game" || f.game === null || v.gameVersions.includes(f.game)) &&
    (skip === "loader" || f.loader === null || v.loaders.includes(f.loader)) &&
    (skip === "type" || f.type === null || v.releaseType === f.type)
  );
}

function tally(list: ContentVersion[], pick: (v: ContentVersion) => string[]): [string, number][] {
  const counts = new Map<string, number>();
  for (const v of list) for (const k of new Set(pick(v))) counts.set(k, (counts.get(k) ?? 0) + 1);
  return [...counts];
}

/** 各分类的选项与数量。数量按另外两个分类的当前选择计算，不会出现「选了之后没有结果」的选项 */
export function filterOptions(versions: ContentVersion[], f: VersionFilter) {
  const others = (skip: FilterKey) => versions.filter((v) => matchesFilter(v, f, skip));
  return {
    game: tally(others("game"), (v) => v.gameVersions).sort((a, b) => compareGameVersion(a[0], b[0])),
    gameTotal: others("game").length,
    loader: tally(others("loader"), (v) => v.loaders),
    type: ["release", "beta", "alpha"]
      .map((t) => [t, others("type").filter((v) => v.releaseType === t).length] as [string, number])
      .filter(([, n]) => n > 0),
  };
}
