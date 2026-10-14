"""Génère les assets embarqués dans l'exe :
- assets/ref_Map0X.bin : vignette 128x128 en niveaux de gris (u8) de la texture de carte du jeu, pour identifier la carte ouverte.
- assets/ref512_Map0X.bin : la même en 512x512, pour le recalage visuel.
- assets/icon.ico : icône de l'appli.
Usage : python tools/make_assets.py <dossier atlas>   (ex. HungerDB/export/maps/atlas)
"""
import sys, os
import numpy as np
from PIL import Image, ImageDraw

atlas = sys.argv[1]
here = os.path.join(os.path.dirname(__file__), '..', 'assets')
N = 128
for m in ('Map01', 'Map02', 'Map03'):
    im = Image.open(os.path.join(atlas, f'T_UI_Map_{m}_01a.webp')).convert('L')
    small = im.resize((N, N), Image.BOX)
    np.asarray(small, dtype=np.uint8).tofile(os.path.join(here, f'ref_{m}.bin'))
    # 512x512 : recalage visuel (position exacte de la carte quand elle est zoomée/déplacée)
    big = im.resize((512, 512), Image.BOX)
    np.asarray(big, dtype=np.uint8).tofile(os.path.join(here, f'ref512_{m}.bin'))
    print(m, small.size, big.size)

# icône : épingle de carte rouge sur parchemin
sizes = [16, 24, 32, 48, 64, 256]
big = Image.new('RGBA', (256, 256), (0, 0, 0, 0))
d = ImageDraw.Draw(big)
d.rounded_rectangle((8, 8, 248, 248), 40, fill=(232, 222, 200, 255), outline=(60, 50, 40, 255), width=10)
d.ellipse((78, 40, 178, 140), fill=(196, 40, 40, 255), outline=(40, 20, 20, 255), width=8)
d.polygon([(90, 118), (166, 118), (128, 216)], fill=(196, 40, 40, 255))
d.line([(90, 118), (128, 216), (166, 118)], fill=(40, 20, 20, 255), width=8)
d.ellipse((110, 72, 146, 108), fill=(240, 235, 220, 255))
big.save(os.path.join(here, 'icon.ico'), sizes=[(s, s) for s in sizes])
print('icon ok')
