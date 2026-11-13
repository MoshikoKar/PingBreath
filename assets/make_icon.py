import sys, os
from PIL import Image, ImageDraw

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
S = 1024


def draw():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # rounded dark tile with subtle gradient-ish border
    d.rounded_rectangle((24, 24, S - 24, S - 24), radius=220, fill=(17, 24, 39, 255))
    d.rounded_rectangle((24, 24, S - 24, S - 24), radius=220, outline=(61, 220, 132, 255), width=26)
    # pulse / heartbeat line
    pts = [(150, 560), (340, 560), (420, 400), (520, 760), (620, 300), (700, 560), (874, 560)]
    d.line(pts, fill=(61, 220, 132, 255), width=62, joint="curve")
    for p in (pts[0], pts[-1]):
        d.ellipse((p[0] - 31, p[1] - 31, p[0] + 31, p[1] + 31), fill=(61, 220, 132, 255))
    return img


big = draw()
sizes = [16, 24, 32, 48, 64, 128, 256]
big.resize((256, 256), Image.LANCZOS).save(os.path.join(out, "icon.png"))
big.save(os.path.join(out, "icon.ico"), sizes=[(s, s) for s in sizes])
big.resize((32, 32), Image.LANCZOS).save(os.path.join(out, "tray32.png"))
with open(os.path.join(out, "tray32.rgba"), "wb") as f:
    f.write(big.resize((32, 32), Image.LANCZOS).tobytes())
print("ok")
