import type { CropRect } from "./background";
export const clamp = (n: number, min = 0, max = 1) => Math.max(min, Math.min(max, n));
export function drawCrop(x: number, y: number, endX: number, endY: number): CropRect {
  const width = Math.max(0.000001, Math.abs(clamp(endX) - clamp(x)));
  const height = Math.max(0.000001, Math.abs(clamp(endY) - clamp(y)));
  return { x: clamp(Math.min(x, endX), 0, 1 - width), y: clamp(Math.min(y, endY), 0, 1 - height), width, height };
}
export function moveCrop(rect: CropRect, dx: number, dy: number): CropRect {
  return { ...rect, x: clamp(rect.x + dx, 0, 1 - rect.width), y: clamp(rect.y + dy, 0, 1 - rect.height) };
}
export function resizeCrop(rect: CropRect, corner: string, x: number, y: number): CropRect {
  const left = corner.includes("w") ? clamp(x, 0, rect.x + rect.width - 0.000001) : rect.x;
  const top = corner.includes("n") ? clamp(y, 0, rect.y + rect.height - 0.000001) : rect.y;
  const right = corner.includes("e") ? clamp(x, rect.x + 0.000001, 1) : rect.x + rect.width;
  const bottom = corner.includes("s") ? clamp(y, rect.y + 0.000001, 1) : rect.y + rect.height;
  return { x: left, y: top, width: right - left, height: bottom - top };
}
