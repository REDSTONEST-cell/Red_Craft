"""生成 RCL 默认版本图标（原创像素画，透明背景）。

用法：python3 scripts/gen-version-icons.py src/assets/version-icons

等轴测方块：世界坐标 x 向右前、z 向左前、y 向上，方块占 0..16。
投影：sx = 16 + x - z，sy = (x + z) / 2 + (16 - y)，画布 32×32 单位。
先按 SS 倍超采样绘制，再缩小到 OUT 像素，边缘平滑、像素格清晰。
"""
import random
import sys
from pathlib import Path
from PIL import Image, ImageDraw

OUT = 128
SS = 4
UNIT = OUT * SS / 32  # 每个投影单位的像素数


def P(x, y, z):
    return ((16 + x - z) * UNIT, ((x + z) / 2 + (16 - y)) * UNIT)


def shade(c, k):
    return tuple(max(0, min(255, round(v * k))) for v in c[:3]) + (255,)


class Canvas:
    def __init__(self):
        self.img = Image.new("RGBA", (OUT * SS, OUT * SS), (0, 0, 0, 0))
        self.d = ImageDraw.Draw(self.img)

    def quad(self, pts, color):
        # 稍微外扩，消除相邻像素格之间的缝
        cx = sum(p[0] for p in pts) / 4
        cy = sum(p[1] for p in pts) / 4
        grown = [(cx + (p[0] - cx) * 1.06, cy + (p[1] - cy) * 1.06) for p in pts]
        self.d.polygon(grown, fill=color)

    def cuboid(self, x0, y0, z0, x1, y1, z1, top, left, right):
        """top/left/right 为函数 (i, j) -> RGBA，i、j 为该面上的格坐标（整数）。"""
        # 右侧面 x = x1：沿 z 与 y
        for zi in range(int(z0), int(z1)):
            for yi in range(int(y0), int(y1)):
                c = right(zi - int(z0), int(y1) - 1 - yi)
                if c:
                    self.quad([P(x1, yi + 1, zi), P(x1, yi + 1, zi + 1), P(x1, yi, zi + 1), P(x1, yi, zi)], shade(c, 0.62))
        # 左侧面 z = z1：沿 x 与 y
        for xi in range(int(x0), int(x1)):
            for yi in range(int(y0), int(y1)):
                c = left(xi - int(x0), int(y1) - 1 - yi)
                if c:
                    self.quad([P(xi, yi + 1, z1), P(xi + 1, yi + 1, z1), P(xi + 1, yi, z1), P(xi, yi, z1)], shade(c, 0.8))
        # 顶面 y = y1
        for xi in range(int(x0), int(x1)):
            for zi in range(int(z0), int(z1)):
                c = top(xi - int(x0), zi - int(z0))
                if c:
                    self.quad([P(xi, y1, zi), P(xi + 1, y1, zi), P(xi + 1, y1, zi + 1), P(xi, y1, zi + 1)], shade(c, 1.0))

    def save(self, path):
        self.img.resize((OUT, OUT), Image.LANCZOS).save(path, optimize=True)


def noise(seed, palette, weights=None):
    rnd = random.Random(seed)
    cache = {}

    def f(i, j):
        if (i, j) not in cache:
            cache[(i, j)] = rnd.choices(palette, weights)[0]
        return cache[(i, j)]

    return f


# ───────────── 草方块（原版） ─────────────
GRASS = [(96, 160, 62), (84, 146, 54), (110, 174, 70), (74, 132, 48), (102, 166, 66)]
DIRT = [(134, 96, 67), (121, 85, 58), (150, 108, 74), (108, 76, 52), (140, 100, 70)]


def grass_side(seed):
    rnd = random.Random(seed)
    edge = [rnd.choice([2, 3, 3, 4]) for _ in range(16)]
    g = noise(seed + 1, GRASS)
    d = noise(seed + 2, DIRT)
    return lambda i, j: g(i, j) if j < edge[i] else d(i, j)


def grass_block():
    c = Canvas()
    c.cuboid(0, 0, 0, 16, 16, 16, noise(1, GRASS), grass_side(2), grass_side(3))
    return c


# ───────────── 小路方块（仅 OptiFine） ─────────────
PATH = [(178, 150, 92), (165, 138, 84), (190, 162, 102), (156, 128, 76)]


def path_side(seed):
    p = noise(seed + 1, PATH)
    d = noise(seed + 2, DIRT)
    return lambda i, j: p(i, j) if j < 2 else d(i, j)


def path_block():
    c = Canvas()
    c.cuboid(0, 0, 0, 16, 15, 16, noise(4, PATH), path_side(5), path_side(6))
    return c


# ───────────── 指令方块（快照） ─────────────
RUST = [(196, 118, 72), (182, 106, 64), (206, 130, 80), (170, 98, 58)]
FRAME = (120, 70, 44)
PANEL = [(150, 150, 150), (138, 138, 138), (162, 162, 162)]


def machine_face(seed, arrow=False):
    r = noise(seed, RUST)
    p = noise(seed + 9, PANEL)

    def f(i, j):
        if i in (0, 15) or j in (0, 15):
            return FRAME
        if 4 <= i <= 11 and 4 <= j <= 11:
            if i in (4, 11) or j in (4, 11):
                return (92, 92, 96)
            if arrow and ((j == 6 and 6 <= i <= 8) or (j == 7 and 7 <= i <= 9) or (j == 8 and 6 <= i <= 8)):
                return (220, 220, 220)
            return p(i, j)
        return r(i, j)

    return f


def command_block():
    c = Canvas()
    c.cuboid(0, 0, 0, 16, 16, 16, machine_face(7, True), machine_face(8, True), machine_face(9, True))
    return c


# ───────────── 布料方块（Fabric） ─────────────
CLOTH = [(222, 208, 170), (210, 194, 154), (230, 218, 184)]
STITCH = (120, 104, 78)


def cloth_face(seed):
    n = noise(seed, CLOTH)

    def f(i, j):
        if i in (1, 14) and 1 <= j <= 14 and j % 2 == 0:
            return STITCH
        if j in (1, 14) and 1 <= i <= 14 and i % 2 == 0:
            return STITCH
        if i in (0, 15) or j in (0, 15):
            return (186, 168, 128)
        if (i + j) % 4 == 0:
            return (200, 184, 144)
        return n(i, j)

    return f


def fabric_block():
    c = Canvas()
    c.cuboid(0, 0, 0, 16, 16, 16, cloth_face(11), cloth_face(12), cloth_face(13))
    return c


# ───────────── 拼布方块（Quilt） ─────────────
PATCHES = [(150, 102, 214), (88, 156, 222), (232, 122, 168), (240, 186, 82)]


def patch_face(rot):
    def f(i, j):
        q = (1 if i >= 8 else 0) + (2 if j >= 8 else 0)
        base = PATCHES[(q + rot) % 4]
        li, lj = i % 8, j % 8
        if li in (0, 7) or lj in (0, 7):
            return shade(base, 0.78)
        if (li + lj) % 3 == 0:
            return shade(base, 1.1)
        return base

    return f


def quilt_block():
    c = Canvas()
    c.cuboid(0, 0, 0, 16, 16, 16, patch_face(0), patch_face(1), patch_face(2))
    return c


# ───────────── 铁砧（Forge） ─────────────
IRON = [(92, 94, 100), (82, 84, 90), (102, 104, 110), (74, 76, 82)]


def iron(seed):
    return noise(seed, IRON)


def anvil():
    c = Canvas()
    # 底座、两层腰、砧面：x 方向为长边
    c.cuboid(3, 0, 4, 13, 3, 12, iron(21), iron(22), iron(23))
    c.cuboid(4, 3, 5, 12, 4, 11, iron(24), iron(25), iron(26))
    c.cuboid(6, 4, 6, 10, 9, 10, iron(27), iron(28), iron(29))
    top = noise(30, [(118, 120, 126), (108, 110, 116), (128, 130, 136)])
    c.cuboid(0, 9, 3, 16, 15, 13, top, iron(31), iron(32))
    return c


# ───────────── 狐狸头（NeoForge） ─────────────
FOX = """
................
.OO..........OO.
.ODO........ODO.
.ODDO......ODDO.
.OFDDOOOOOODDFO.
.OFFOOOOOOOOFFO.
OOOOOOOOOOOOOOOO
OOOOOOOOOOOOOOOO
OOKKOOOOOOOOKKOO
OOKWOOOOOOOOKWOO
OOOOOOWWWWOOOOOO
.OOOOWWWWWWOOOO.
..WWWWWNNWWWWW..
...WWWWWWWWWW...
.....WWWWWW.....
................
"""
FOX_COLORS = {
    "O": (226, 120, 40),
    "D": (178, 82, 26),
    "F": (246, 214, 176),
    "K": (40, 30, 28),
    "W": (246, 240, 230),
    "N": (40, 30, 28),
}


def fox():
    rows = [r for r in FOX.strip("\n").split("\n")]
    img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
    for j, row in enumerate(rows):
        for i, ch in enumerate(row):
            if ch in FOX_COLORS:
                img.putpixel((i, j), FOX_COLORS[ch] + (255,))
    # 加一点明暗：下半部分略暗
    for j in range(16):
        for i in range(16):
            px = img.getpixel((i, j))
            if px[3] and j >= 8 and px[:3] == FOX_COLORS["O"]:
                img.putpixel((i, j), shade(px, 0.9))
    # 放大为 7 倍并居中，四周留白，与方块图标的视觉大小一致
    big = img.resize((16 * 7, 16 * 7), Image.NEAREST)
    canvas = Image.new("RGBA", (OUT, OUT), (0, 0, 0, 0))
    canvas.alpha_composite(big, ((OUT - 112) // 2, (OUT - 112) // 2 + 2))
    return canvas


def main(out):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    for name, make in [
        ("vanilla", grass_block),
        ("snapshot", command_block),
        ("optifine", path_block),
        ("fabric", fabric_block),
        ("quilt", quilt_block),
        ("forge", anvil),
    ]:
        make().save(out / f"{name}.png")
    fox().save(out / "neoforge.png", optimize=True)


if __name__ == "__main__":
    main(sys.argv[1])
