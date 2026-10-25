// 资源的中文显示：图标走后端（镜像加速 + 缓存）、常见 Mod 的中文名、中文关键词搜索。
import { convertFileSrc } from "@tauri-apps/api/core";

/** 资源图标地址：交给后端的 rcicon 协议，同时尝试官方 CDN 与 MCIM 镜像，并缓存到本地。 */
export function iconSrc(url: string | null | undefined): string {
  if (!url) return "";
  if (!/^https:\/\/(cdn\.modrinth\.com|media\.forgecdn\.net|edge\.forgecdn\.net|mediafilez\.forgecdn\.net)\//.test(url)) return url;
  return convertFileSrc(url, "rcicon");
}

// 常见 Mod 的中文名（社区通用译名）：[英文名, 中文名, ...别名（slug 或其他写法）]。
// 英文名同时用作中文搜索时的英文关键词。
const ENTRIES: [string, string, ...string[]][] = [
  ["Sodium", "钠"],
  ["Lithium", "锂"],
  ["Phosphor", "磷"],
  ["Indium", "铟"],
  ["Starlight", "星光"],
  ["Iris Shaders", "鸢尾花光影", "iris"],
  ["Oculus", "Oculus 光影"],
  ["Embeddium", "嵌入钠"],
  ["Rubidium", "铷"],
  ["FerriteCore", "铁氧体磁芯", "ferrite-core"],
  ["Entity Culling", "实体剔除", "entityculling"],
  ["Mod Menu", "模组菜单", "modmenu"],
  ["Dynamic FPS", "动态帧率", "dynamic-fps"],
  ["LambDynamicLights", "动态光源"],
  ["Distant Horizons", "遥远的地平线"],
  ["No Chat Reports", "禁用聊天举报"],
  ["3D Skin Layers", "3D 皮肤层", "3dskinlayers"],
  ["Just Enough Items", "JEI 物品管理器", "jei"],
  ["Roughly Enough Items", "REI 物品管理器", "rei"],
  ["EMI", "EMI 物品管理器"],
  ["Jade", "玉"],
  ["AppleSkin", "苹果皮"],
  ["Mouse Tweaks", "鼠标手势"],
  ["JourneyMap", "旅行地图"],
  ["Xaero's Minimap", "Xaero 的小地图", "xaeros-minimap"],
  ["Xaero's World Map", "Xaero 的世界地图", "xaeros-world-map"],
  ["Create", "机械动力"],
  ["Farmer's Delight", "农夫乐事", "farmers-delight"],
  ["Waystones", "传送石碑"],
  ["Traveler's Backpack", "旅行者背包", "travelersbackpack"],
  ["Sophisticated Backpacks", "精妙背包"],
  ["Mekanism", "通用机械"],
  ["Applied Energistics 2", "应用能源 2", "ae2"],
  ["Thermal Expansion", "热力膨胀"],
  ["Thermal Foundation", "热力基本"],
  ["The Twilight Forest", "暮色森林", "twilightforest"],
  ["Botania", "植物魔法"],
  ["Tinkers' Construct", "匠魂", "tinkers-construct", "tconstruct"],
  ["Immersive Engineering", "沉浸工程"],
  ["IndustrialCraft 2", "工业 2", "ic2"],
  ["BuildCraft", "建筑"],
  ["Alex's Mobs", "Alex 的生物", "alexs-mobs"],
  ["Quark", "夸克"],
  ["Supplementaries", "锦致装饰"],
  ["Biomes O' Plenty", "超多生物群系", "biomesoplenty"],
  ["Iron Chests", "铁箱子"],
  ["Storage Drawers", "储物抽屉"],
  ["Refined Storage", "精致存储"],
  ["Ice and Fire", "冰火传说", "iceandfire"],
  ["The Aether", "天境", "aether"],
  ["Better End", "更好的末地", "betterend"],
  ["Better Nether", "更好的下界", "betternether"],
  ["Simple Voice Chat", "简单的语音聊天", "voicechat"],
  ["Better Combat", "更好的战斗"],
  ["Epic Fight", "史诗战斗"],
  ["Torchmaster", "火把大师"],
  ["Carpet", "地毯", "fabric-carpet"],
  ["Litematica", "投影"],
  ["WorldEdit", "创世神"],
  ["Chisel", "凿子"],
  ["Pam's HarvestCraft", "潘马斯丰收工艺"],
  ["Ender IO", "末影接口", "enderio"],
  ["Ars Nouveau", "新生魔艺"],
  ["Blood Magic", "血魔法"],
  ["Thaumcraft", "神秘时代"],
  ["Astral Sorcery", "星辉魔法"],
  ["Draconic Evolution", "龙之研究"],
  ["Mystical Agriculture", "神秘农业"],
  ["SecurityCraft", "安全工艺"],
  ["YUNG's Better Dungeons", "YUNG 的更好的地牢"],
  ["YUNG's Better Mineshafts", "YUNG 的更好的废弃矿井"],
  ["Continuity", "连接纹理"],
  ["Iron's Spells 'n Spellbooks", "铁魔法"],
  ["Timeless and Classics Zero", "永恒枪械工坊：零", "tacz"],
];

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
