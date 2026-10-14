"""Generates the assets embedded in the executable:
- assets/ref_Map0X.bin: 128x128 grayscale (u8) thumbnail of the game's map texture, used to identify which map is open.
- assets/ref512_Map0X.bin: the same at 512x512, used for visual alignment.
- assets/icon.ico: the application icon.
Usage: python tools/make_assets.py <atlas folder>   (e.g. HungerDB/export/maps/atlas)
"""
import sys, os, io
import numpy as np
from PIL import Image, ImageDraw

atlas = sys.argv[1]  # folder holding the game's map textures (T_UI_Map_Map0X_01a.webp)
here = os.path.join(os.path.dirname(__file__), '..', 'assets')  # output folder: <repo>/assets
N = 128
# --- Map reference textures: one per raid map ---
for m in ('Map01', 'Map02', 'Map03'):
    im = Image.open(os.path.join(atlas, f'T_UI_Map_{m}_01a.webp')).convert('L')
    # 512x512: visual alignment (exact position of the map when it is zoomed/panned).
    # Stored on 4 bits (2 pixels per byte) to keep the exe small; the correlation is insensitive to this.
    big = np.asarray(im.resize((512, 512), Image.BOX), dtype=np.uint8)
    q = (big.astype(np.uint16) * 15 + 127) // 255
    packed = (q[:, 0::2] << 4 | q[:, 1::2]).astype(np.uint8)
    packed.tofile(os.path.join(here, f'ref512_{m}.bin'))
    print(m, big.shape)

# --- Application icon ---
# "HC" icon (assets/icon_source.png = original image, not committed because it is large; otherwise assets/icon_512.png).
# Large sizes = full image; small sizes (<= 32 px) = cropped to the "HC" letters, otherwise unreadable.
from PIL import ImageEnhance
src_path = os.path.join(here, 'icon_source.png')
if not os.path.exists(src_path):
    src_path = os.path.join(here, 'icon_512.png')
full = Image.open(src_path).convert('RGBA')
full.resize((512, 512), Image.LANCZOS).save(os.path.join(here, 'icon_512.png'), optimize=True)
W = full.width
crop = full.crop((int(0.29 * W), int(0.345 * W), int(0.70 * W), int(0.755 * W)))
crop = ImageEnhance.Contrast(crop).enhance(1.25)
frames = {}
for size in (16, 20, 24, 32, 48, 64, 256):
    base = crop if size <= 32 else full
    frames[size] = base.resize((size, size), Image.LANCZOS)
# Pillow writes every size from the single image it is given: we assemble the ICO by hand to keep both variants.
import struct
entries, blobs = [], []
for size, img in frames.items():
    # 256-colour palette (the icon is opaque): much lighter PNG, with no visible difference.
    b = io.BytesIO(); img.convert('RGB').quantize(256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG).save(b, 'PNG', optimize=True); data = b.getvalue()
    entries.append((size, data))
# ICO layout: 6-byte ICONDIR header, then one 16-byte ICONDIRENTRY per image, then the PNG blobs.
offset = 6 + 16 * len(entries)
out = struct.pack('<HHH', 0, 1, len(entries))
body = b''
for size, data in entries:
    # width/height (256 is stored as 0), no palette, reserved, 1 plane, 32 bpp, blob size, blob offset.
    out += struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(data), offset + len(body))
    body += data
open(os.path.join(here, 'icon.ico'), 'wb').write(out + body)
print('icon ok', [s for s, _ in entries])
