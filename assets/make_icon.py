"""生成 assets/app.ico：黑色圆角方块 + 白色 sparkle。改颜色改下面的常量重跑即可。"""
from math import cos, radians, sin

from PIL import Image, ImageDraw

SIZE = 1024
BG = (29, 29, 31, 255)
FG = (255, 255, 255, 255)


def sparkle(cx, cy, r, inner=0.16):
    pts = []
    for i in range(4):
        a = -90 + i * 90
        pts.append((cx + r * cos(radians(a)), cy + r * sin(radians(a))))
        b = a + 45
        ir = r * inner * 1.414
        pts.append((cx + ir * cos(radians(b)), cy + ir * sin(radians(b))))
    return pts


img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([0, 0, SIZE - 1, SIZE - 1], radius=int(SIZE * 0.22), fill=BG)

d.polygon(sparkle(0.44 * SIZE, 0.57 * SIZE, 0.30 * SIZE), fill=FG)
d.polygon(sparkle(0.68 * SIZE, 0.33 * SIZE, 0.15 * SIZE), fill=FG)

img.save("assets/app.ico", format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
img.resize((256, 256), Image.LANCZOS).save("assets/app_preview.png")
print("ok")
