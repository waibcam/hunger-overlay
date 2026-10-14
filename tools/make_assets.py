"""Génère les assets embarqués dans l'exe :
- assets/ref_Map0X.bin : vignette 128x128 en niveaux de gris (u8) de la texture de carte du jeu, pour identifier la carte ouverte.
- assets/ref512_Map0X.bin : la même en 512x512, pour le recalage visuel.
- assets/icon.ico : icône de l'appli.
Usage : python tools/make_assets.py <dossier atlas>   (ex. HungerDB/export/maps/atlas)
"""
import sys, os, io
import numpy as np
from PIL import Image, ImageDraw

atlas = sys.argv[1]
here = os.path.join(os.path.dirname(__file__), '..', 'assets')
N = 128
for m in ('Map01', 'Map02', 'Map03'):
    im = Image.open(os.path.join(atlas, f'T_UI_Map_{m}_01a.webp')).convert('L')
    # 512x512 : recalage visuel (position exacte de la carte quand elle est zoomée/déplacée).
    # Stockée sur 4 bits (2 pixels par octet) pour alléger l'exe ; la corrélation y est insensible.
    big = np.asarray(im.resize((512, 512), Image.BOX), dtype=np.uint8)
    q = (big.astype(np.uint16) * 15 + 127) // 255
    packed = (q[:, 0::2] << 4 | q[:, 1::2]).astype(np.uint8)
    packed.tofile(os.path.join(here, f'ref512_{m}.bin'))
    print(m, big.shape)

# icône « HC » (assets/icon_source.png, image d'origine, non versionnée car lourde ; sinon assets/icon_512.png).
# Grandes tailles = image complète ; petites tailles (≤ 32 px) = recadrage sur les lettres « HC », sinon illisible.
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
# Pillow écrit chaque taille depuis l'image fournie : on assemble l'ICO à la main pour garder les deux variantes.
import struct
entries, blobs = [], []
for size, img in frames.items():
    # Palette de 256 couleurs (l'icône est opaque) : PNG bien plus léger, sans différence visible.
    b = io.BytesIO(); img.convert('RGB').quantize(256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG).save(b, 'PNG', optimize=True); data = b.getvalue()
    entries.append((size, data))
offset = 6 + 16 * len(entries)
out = struct.pack('<HHH', 0, 1, len(entries))
body = b''
for size, data in entries:
    out += struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(data), offset + len(body))
    body += data
open(os.path.join(here, 'icon.ico'), 'wb').write(out + body)
print('icon ok', [s for s, _ in entries])
