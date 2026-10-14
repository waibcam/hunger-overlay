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

# icône : celle du site Hunger Companion (assets/icon_source.png, 512x512), coins arrondis
src = Image.open(os.path.join(here, 'icon_source.png')).convert('RGBA').resize((256, 256), Image.LANCZOS)
mask = Image.new('L', (256, 256), 0)
ImageDraw.Draw(mask).rounded_rectangle((0, 0, 255, 255), 48, fill=255)
big = Image.new('RGBA', (256, 256), (0, 0, 0, 0))
big.paste(src, (0, 0), mask)
big.save(os.path.join(here, 'icon.ico'), sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 256)])
print('icon ok')
