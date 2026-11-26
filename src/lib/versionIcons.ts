// 默认版本图标，取自 Plain Craft Launcher（© 龙腾猫跃，https://github.com/Meloong-Git/PCL），
// 按《PCL 存储库合理使用指南》的「轻度使用」署名使用；选择规则与 PCL 一致。
import grass from "../assets/version-icons/Grass.png";
import commandBlock from "../assets/version-icons/CommandBlock.png";
import cobblestone from "../assets/version-icons/CobbleStone.png";
import grassPath from "../assets/version-icons/GrassPath.png";
import anvil from "../assets/version-icons/Anvil.png";
import neoforge from "../assets/version-icons/NeoForge.png";
import fabric from "../assets/version-icons/Fabric.png";
import quilt from "../assets/version-icons/Quilt.png";

export type DefaultIconKind = "vanilla" | "snapshot" | "old" | "optifine" | "forge" | "neoforge" | "fabric" | "quilt";

export const DEFAULT_ICONS: Record<DefaultIconKind, string> = {
  vanilla: grass,
  snapshot: commandBlock,
  old: cobblestone,
  optifine: grassPath,
  forge: anvil,
  neoforge,
  fabric,
  quilt,
};

/**
 * 按版本类型选默认图标：有 Mod 加载器用加载器图标（Forge 铁砧、NeoForge、Fabric、Quilt），
 * 仅装 OptiFine 用草径，快照用命令方块，远古版（Alpha / Beta）用圆石，其余原版用草方块。
 */
export function defaultIconKind(inst: { loader: { kind: string } | null; optifine: string | null; kind?: string }): DefaultIconKind {
  const loader = inst.loader?.kind;
  if (loader === "forge" || loader === "neoforge" || loader === "fabric" || loader === "quilt") return loader;
  if (inst.optifine) return "optifine";
  if (inst.kind === "snapshot") return "snapshot";
  if (inst.kind === "old_beta" || inst.kind === "old_alpha") return "old";
  return "vanilla";
}
