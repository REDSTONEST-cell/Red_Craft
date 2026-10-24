"""生成应用图标源文件 app-icon.png（像素风 R 字方块）。之后运行 `pnpm tauri icon app-icon.png` 生成各尺寸。"""
from PIL import Image, ImageDraw

S = 1024
PAPER = (244, 239, 230, 255)
INK = (27, 26, 23, 255)
ORANGE = (232, 89, 12, 255)
LIGHT = (255, 146, 43, 255)
DARK = (176, 62, 4, 255)

# 12x12 像素网格的 R
R = [
    "............",
    "..XXXXXXX...",
    "..XXXXXXXX..",
    "..XX....XX..",
    "..XX....XX..",
    "..XXXXXXXX..",
    "..XXXXXXX...",
    "..XX..XXX...",
    "..XX...XXX..",
    "..XX....XX..",
    "..XX....XX..",
    "............",
]

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
pad = 64
d.rounded_rectangle([pad, pad, S - pad, S - pad], radius=150, fill=INK)
inner = 96
d.rounded_rectangle([pad + inner // 3, pad + inner // 3, S - pad - inner // 3, S - pad - inner // 3], radius=110, fill=PAPER)

cell = (S - 2 * pad - 2 * inner) // 12
ox = oy = pad + inner
for y, row in enumerate(R):
    for x, ch in enumerate(row):
        if ch != "X":
            continue
        x0, y0 = ox + x * cell, oy + y * cell
        d.rectangle([x0, y0, x0 + cell - 1, y0 + cell - 1], fill=ORANGE)
        # 方块质感：上沿高光、下沿阴影
        above = y > 0 and R[y - 1][x] == "X"
        below = y < 11 and R[y + 1][x] == "X"
        t = max(cell // 7, 6)
        if not above:
            d.rectangle([x0, y0, x0 + cell - 1, y0 + t], fill=LIGHT)
        if not below:
            d.rectangle([x0, y0 + cell - 1 - t, x0 + cell - 1, y0 + cell - 1], fill=DARK)

img.save("app-icon.png")
