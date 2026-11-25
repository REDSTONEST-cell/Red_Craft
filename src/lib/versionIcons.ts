// 默认版本图标（原创像素画，由 scripts/gen-version-icons.py 生成）
import vanilla from "../assets/version-icons/vanilla.png";
import snapshot from "../assets/version-icons/snapshot.png";
import optifine from "../assets/version-icons/optifine.png";
import fabric from "../assets/version-icons/fabric.png";
import quilt from "../assets/version-icons/quilt.png";
import forge from "../assets/version-icons/forge.png";
import neoforge from "../assets/version-icons/neoforge.png";

export type DefaultIconKind = "vanilla" | "snapshot" | "optifine" | "fabric" | "quilt" | "forge" | "neoforge";

export const DEFAULT_ICONS: Record<DefaultIconKind, string> = { vanilla, snapshot, optifine, fabric, quilt, forge, neoforge };

/** 按版本类型选默认图标：有 Mod 加载器用加载器图标，仅装 OptiFine 用小路方块，快照用指令方块，其余原版用草方块 */
export function defaultIconKind(inst: { loader: { kind: string } | null; optifine: string | null; kind?: string }): DefaultIconKind {
  const loader = inst.loader?.kind;
  if (loader === "forge" || loader === "neoforge" || loader === "fabric" || loader === "quilt") return loader;
  if (inst.optifine) return "optifine";
  if (inst.kind === "snapshot") return "snapshot";
  return "vanilla";
}
