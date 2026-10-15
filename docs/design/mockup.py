"""Hunger Overlay - map overlay redesign mockups (see ../DESIGN.md).

Renders the proposed panel, markers and tooltips ON TOP OF real in-game screenshots:

    mockup_default.png   default map view (base_default.png = z0.png) - tracked quests, Quests layer,
                         extraction layer, an item search, tooltip on an untracked quest spot
    mockup_zoomed.png    zoomed map (base_zoomed.png = S1.png, old overlay painted out) - most layers on,
                         place labels, tooltip on a tracked container, "now tracking" toast
    panel_collapsed.png  panel states: collapsed bar, empty state, API-key problem, overflow footer
    markers_sheet.png    every marker type, normal + hover, at 2x on parchment

Only Python + Pillow + numpy. Marker positions are REAL: they come from the overlay's cached pack
(%APPDATA%\\HungerOverlay\\pack.json), projected with the transform measured on the screenshots.
Everything is drawn with the overlay's Direct2D primitive set: filled/stroked circles (optionally dashed),
rounded rects, lines, diamonds (filled = 4-point polygon) and text. All sizes are in px at 1440p (sc = 1).

Run:  python docs/design/mockup.py
"""

import json
import math
import os

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
FONTS = r"C:\Windows\Fonts"
SS = 2  # supersampling factor (anti-aliasing)

# ----------------------------------------------------------------------------------------------
# Design tokens (mirrors DESIGN.md section 3)
# ----------------------------------------------------------------------------------------------
INK_950 = "#110E0A"   # panel / tooltip background
INK_900 = "#1A1510"   # inset fields
INK_800 = "#241D16"   # chips
INK_700 = "#30271D"   # hover / "on" pills
HALO = "#0B0907"      # marker halo
LINE = "#C8A765"      # gold hairlines
GOLD = "#D4AF6A"      # headings, icons
GOLD_HI = "#EBCB8B"   # hover / focus
ACCENT = "#D9A54A"    # interactive accent (same as Settings window)
T100 = "#F1EADB"
T200 = "#D9D1C2"
T300 = "#B3A995"
T500 = "#857C6D"
WARN = "#F0A24B"
OK = "#86C27E"

C_EXTRACT = "#3E9EDB"
C_SPAWN = "#F3ECDD"
C_TRAP = "#F07A2E"
C_SOUND = "#A46BE0"
C_BOSS = "#E0453A"
C_KEY = "#E3B341"
C_LOCK = "#A9BCC6"
C_LOOT = "#F2C230"
C_HARV = {"Conservator": "#D9739F", "Naturalist": "#62C26A", "Scavenger": "#D1A23A"}
C_QUEST_AVAIL = "#F0D58C"
C_PLACE = GOLD

# Owner badge glyphs: quests 1..9, crafting / searches A..Z (colour-blind safe identity).


def rgba(c, a=1.0):
    c = c.lstrip("#")
    return (int(c[0:2], 16), int(c[2:4], 16), int(c[4:6], 16), max(0, min(255, int(round(a * 255)))))


def lum(c):
    r, g, b, _ = rgba(c)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


_font_cache = {}


def font(name, size):
    key = (name, round(size * SS, 2))
    if key not in _font_cache:
        _font_cache[key] = ImageFont.truetype(os.path.join(FONTS, name), max(1, round(size * SS)))
    return _font_cache[key]


# Font roles (file, px at 1440p)
F_DISPLAY = "palab.ttf"     # Palatino Linotype Bold - section titles, caps + tracking
F_UI = "segoeui.ttf"
F_UI_SB = "seguisb.ttf"     # Segoe UI Semibold
F_UI_B = "segoeuib.ttf"
F_UI_I = "segoeuii.ttf"
F_ICON = "SegoeIcons.ttf"   # Segoe Fluent Icons (Win11) / segmdl2.ttf on Win10
F_SYM = "seguisym.ttf"
F_SERIF_B = "georgiab.ttf"

ICON = {"settings": "\uE713", "search": "\uE721", "down": "\uE70D", "right": "\uE76C", "close": "\uE711",
        "add": "\uE710", "remove": "\uE738", "eye": "\uE890", "hide": "\uED1A", "globe": "\uE774",
        "warn": "\uE7BA", "check": "\uE73E", "info": "\uE946", "undo": "\uE7A7"}


# ----------------------------------------------------------------------------------------------
# Canvas: Direct2D-like primitives, drawn supersampled and composited with real alpha blending
# ----------------------------------------------------------------------------------------------
class Canvas:
    def __init__(self, w, h):
        self.w, self.h = w, h
        self.im = Image.new("RGBA", (w * SS, h * SS), (0, 0, 0, 0))
        self.fade = 1.0

    def _tmp(self, x0, y0, x1, y1):
        X0 = max(0, int(math.floor(x0 * SS)) - 2)
        Y0 = max(0, int(math.floor(y0 * SS)) - 2)
        X1 = min(self.w * SS, int(math.ceil(x1 * SS)) + 2)
        Y1 = min(self.h * SS, int(math.ceil(y1 * SS)) + 2)
        if X1 <= X0 or Y1 <= Y0:
            return None
        t = Image.new("RGBA", (X1 - X0, Y1 - Y0), (0, 0, 0, 0))
        return t, ImageDraw.Draw(t), X0, Y0

    def _commit(self, t, X0, Y0):
        if self.fade < 0.999:
            a = t.getchannel("A").point(lambda v: int(v * self.fade))
            t.putalpha(a)
        self.im.alpha_composite(t, (X0, Y0))

    # -- shapes ---------------------------------------------------------------------------------
    def circle(self, x, y, r, c, a=1.0):
        tt = self._tmp(x - r, y - r, x + r, y + r)
        if not tt:
            return
        t, d, X0, Y0 = tt
        d.ellipse([x * SS - r * SS - X0, y * SS - r * SS - Y0, x * SS + r * SS - X0, y * SS + r * SS - Y0], fill=rgba(c, a))
        self._commit(t, X0, Y0)

    def ring(self, x, y, r, w, c, a=1.0, dashed=False):
        ro = r + w / 2
        tt = self._tmp(x - ro, y - ro, x + ro, y + ro)
        if not tt:
            return
        t, d, X0, Y0 = tt
        box = [x * SS - ro * SS - X0, y * SS - ro * SS - Y0, x * SS + ro * SS - X0, y * SS + ro * SS - Y0]
        if dashed:
            circ = 2 * math.pi * r
            n = max(6, int(circ / (w * 4.5)))
            step = 360 / n
            for i in range(n):
                d.arc(box, i * step, i * step + step * 0.55, fill=rgba(c, a), width=max(1, round(w * SS)))
        else:
            d.ellipse(box, outline=rgba(c, a), width=max(1, round(w * SS)))
        self._commit(t, X0, Y0)

    def line(self, x0, y0, x1, y1, w, c, a=1.0):
        self.polyline([(x0, y0), (x1, y1)], w, c, a)

    def polyline(self, pts, w, c, a=1.0, closed=False):
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        tt = self._tmp(min(xs) - w, min(ys) - w, max(xs) + w, max(ys) + w)
        if not tt:
            return
        t, d, X0, Y0 = tt
        P = [(px * SS - X0, py * SS - Y0) for px, py in pts]
        if closed:
            P = P + [P[0]]
        col = rgba(c, 1.0)
        d.line(P, fill=col, width=max(1, round(w * SS)), joint="curve")
        rr = w * SS / 2
        for px, py in P:  # round caps / joins
            d.ellipse([px - rr, py - rr, px + rr, py + rr], fill=col)
        if a < 1.0:
            t.putalpha(t.getchannel("A").point(lambda v: int(v * a)))
        self._commit(t, X0, Y0)

    def poly(self, pts, c, a=1.0):
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        tt = self._tmp(min(xs), min(ys), max(xs), max(ys))
        if not tt:
            return
        t, d, X0, Y0 = tt
        d.polygon([(px * SS - X0, py * SS - Y0) for px, py in pts], fill=rgba(c, a))
        self._commit(t, X0, Y0)

    def diamond(self, x, y, r, c, a=1.0):  # filled
        self.poly([(x, y - r), (x + r, y), (x, y + r), (x - r, y)], c, a)

    def diamond_stroke(self, x, y, r, w, c, a=1.0):
        self.polyline([(x, y - r), (x + r, y), (x, y + r), (x - r, y)], w, c, a, closed=True)

    def rrect(self, l, t_, r, b, rad, fill=None, fa=1.0, stroke=None, sa=1.0, sw=1.0):
        tt = self._tmp(l - sw, t_ - sw, r + sw, b + sw)
        if not tt:
            return
        t, d, X0, Y0 = tt
        box = [l * SS - X0, t_ * SS - Y0, r * SS - X0, b * SS - Y0]
        if fill:
            d.rounded_rectangle(box, radius=rad * SS, fill=rgba(fill, fa))
        if stroke:
            # stroke centred on the edge, like Direct2D
            h = sw * SS / 2
            box2 = [box[0] - h, box[1] - h, box[2] + h, box[3] + h]
            t2 = Image.new("RGBA", t.size, (0, 0, 0, 0))
            ImageDraw.Draw(t2).rounded_rectangle(box2, radius=rad * SS + h, outline=rgba(stroke, sa), width=max(1, round(sw * SS)))
            t.alpha_composite(t2)
        self._commit(t, X0, Y0)

    # -- text -----------------------------------------------------------------------------------
    def measure(self, s, fnt, tracking=0.0):
        if not s:
            return 0.0
        return fnt.getlength(s) / SS + tracking * max(0, len(s) - 1)

    def text(self, x, y, s, fnt, c, a=1.0, anchor="lm", tracking=0.0):
        """Draws a single line; y is the vertical anchor (middle by default). Returns the width."""
        w = self.measure(s, fnt, tracking)
        asc, desc = fnt.getmetrics()
        hpx = (asc + desc) / SS
        x0 = x if anchor[0] == "l" else (x - w / 2 if anchor[0] == "m" else x - w)
        tt = self._tmp(x0 - 4, y - hpx - 4, x0 + w + 4, y + hpx + 4)
        if not tt:
            return w
        t, d, X0, Y0 = tt
        va = anchor[1]
        if tracking == 0:
            d.text((x0 * SS - X0, y * SS - Y0), s, font=fnt, fill=rgba(c, a), anchor="l" + va)
        else:
            cx = x0
            for ch in s:
                d.text((cx * SS - X0, y * SS - Y0), ch, font=fnt, fill=rgba(c, a), anchor="l" + va)
                cx += fnt.getlength(ch) / SS + tracking
        self._commit(t, X0, Y0)
        return w

    def ellipsize(self, s, fnt, maxw):
        if self.measure(s, fnt) <= maxw:
            return s
        while s and self.measure(s + "…", fnt) > maxw:
            s = s[:-1]
        return s.rstrip() + "…"


def composite(base, cv):
    """Box-downsamples the premultiplied overlay and blends it over the base screenshot."""
    base_a = np.asarray(base.convert("RGB"), dtype=np.float32) / 255.0
    ov = np.asarray(cv.im, dtype=np.uint8)
    H, W = base_a.shape[:2]
    out = np.empty_like(base_a)
    band = 240
    for y0 in range(0, H, band):
        y1 = min(H, y0 + band)
        o = ov[y0 * SS:y1 * SS].astype(np.float32) / 255.0
        a = o[..., 3:4]
        prem = (o[..., :3] * a).reshape(y1 - y0, SS, W, SS, 3).mean(axis=(1, 3))
        aa = a.reshape(y1 - y0, SS, W, SS, 1).mean(axis=(1, 3))
        out[y0:y1] = prem + base_a[y0:y1] * (1.0 - aa)
    return Image.fromarray((np.clip(out, 0, 1) * 255 + 0.5).astype(np.uint8))


# ----------------------------------------------------------------------------------------------
# Marker system (DESIGN.md section 5). s = size multiplier (marker_scale * sc), h = hovered.
# ----------------------------------------------------------------------------------------------
def focus_ring(cv, x, y, r):
    cv.ring(x, y, r + 4.5, 4.4, HALO, 0.55)
    cv.ring(x, y, r + 4.5, 2.2, GOLD_HI, 1.0)


def badge_text(cv, x, y, ch, fill, size):
    col = "#17120C" if lum(fill) > 120 else "#FFFFFF"
    cv.text(x, y + 0.5, ch, font(F_UI_B, size), col, anchor="mm")


def m_tracked_disc(cv, x, y, color, badge=None, s=1.0, h=False, optional=False):
    r = 10 * s * (1.25 if h else 1)
    cv.circle(x, y, r + 2.5 * s, HALO, 0.6)
    cv.circle(x, y, r, color)
    cv.ring(x, y, r - 1.0 * s, 2.0 * s, "#FFFFFF", 0.95)
    if badge:
        badge_text(cv, x, y, badge, color, 11.5 * s * (1.25 if h else 1))
    else:
        cv.circle(x, y, r * 0.28, "#FFFFFF", 0.95)
    if optional:
        cv.ring(x, y, r + 5 * s, 1.5 * s, color, 1.0, dashed=True)
    if h:
        focus_ring(cv, x, y, r + 2 * s)


def m_tracked_square(cv, x, y, color, badge=None, s=1.0, h=False, locked=False):
    k = 9 * s * (1.25 if h else 1)
    cv.rrect(x - k - 2.5 * s, y - k - 2.5 * s, x + k + 2.5 * s, y + k + 2.5 * s, 4 * s, HALO, 0.6)
    fill = "#77716A" if locked else color
    cv.rrect(x - k, y - k, x + k, y + k, 3 * s, fill, 1.0 if not locked else 0.9)
    cv.rrect(x - k + 1 * s, y - k + 1 * s, x + k - 1 * s, y + k - 1 * s, 2.2 * s, None, stroke="#FFFFFF", sa=0.95 if not locked else 0.5, sw=2.0 * s)
    if badge:
        badge_text(cv, x, y, badge, fill, 11.5 * s * (1.25 if h else 1))
    if h:
        focus_ring(cv, x, y, k * 1.25 + 2 * s)


def m_tracked_diamond(cv, x, y, color, badge=None, s=1.0, h=False):
    r = 12 * s * (1.25 if h else 1)
    cv.diamond(x, y, r + 3.5 * s, HALO, 0.6)
    cv.diamond(x, y, r, color)
    cv.diamond_stroke(x, y, r - 1.4 * s, 2.0 * s, "#FFFFFF", 0.95)
    if badge:
        badge_text(cv, x, y, badge, color, 11 * s)
    if h:
        focus_ring(cv, x, y, r)


def count_pill(cv, x, y, n, color, s=1.0):
    """Cluster count, top-right of the marker (x, y = marker centre, r ~ 12)."""
    t = f"{n}"
    f = font(F_UI_SB, 11 * s)
    tw = cv.measure(t, f)
    l, top = x + 6 * s, y - 20 * s
    cv.rrect(l, top, l + tw + 10 * s, top + 15 * s, 7.5 * s, INK_950, 0.95, stroke=color, sa=0.95, sw=1.2 * s)
    cv.text(l + 5 * s + tw / 2, top + 7.5 * s, t, f, T100, anchor="mm")


def m_cluster(cv, x, y, color, badge, n, s=1.0, shape="disc"):
    if shape == "square":
        m_tracked_square(cv, x, y, color, badge, s * 1.12)
    else:
        m_tracked_disc(cv, x, y, color, badge, s * 1.18)
    count_pill(cv, x, y, n, color, s)


def m_done(cv, x, y, s=1.0, h=False):
    r = 6.5 * s * (1.25 if h else 1)
    cv.circle(x, y, r + 2 * s, HALO, 0.45)
    cv.circle(x, y, r, "#6F6A62", 0.85)
    cv.ring(x, y, r - 0.6 * s, 1.2 * s, "#F3ECDD", 0.6)
    cv.polyline([(x - 2.8 * s, y + 0.2 * s), (x - 0.8 * s, y + 2.4 * s), (x + 3.0 * s, y - 2.2 * s)], 1.6 * s, "#F3ECDD", 0.9)


def token(cv, x, y, ring_color, r=8.5, s=1.0, h=False, ring_w=1.8):
    r = r * s * (1.25 if h else 1)
    cv.circle(x, y, r + 2.2 * s, HALO, 0.5)
    cv.circle(x, y, r, "#16120D", 0.92)
    cv.ring(x, y, r - ring_w * s / 2, ring_w * s, ring_color)
    if h:
        focus_ring(cv, x, y, r)
    return r


def m_quest_avail(cv, x, y, s=1.0, h=False):
    r = token(cv, x, y, C_QUEST_AVAIL, 8.5, s, h)
    cv.text(x + 0.2 * s, y + 0.3 * s, "!", font(F_SERIF_B, 12.5 * s * r / (8.5 * s)), C_QUEST_AVAIL, anchor="mm")


def m_trap(cv, x, y, s=1.0, h=False):
    r = token(cv, x, y, C_TRAP, 8, s, h)
    k = 3.3 * s * r / (8 * s)
    cv.line(x - k, y - k, x + k, y + k, 2.1 * s, "#FF9C5C")
    cv.line(x - k, y + k, x + k, y - k, 2.1 * s, "#FF9C5C")


def m_boss(cv, x, y, s=1.0, h=False):
    r = token(cv, x, y, C_BOSS, 9, s, h, ring_w=2.2)
    cv.text(x, y + 0.6 * s, "\u2620", font(F_SYM, 12.5 * s * r / (9 * s)), "#F8DCD4", anchor="mm")


def m_key(cv, x, y, s=1.0, h=False):
    r = token(cv, x, y, C_KEY, 8.5, s, h)
    k = s * r / (8.5 * s)
    cv.ring(x - 2.4 * k, y, 2.1 * k, 1.6 * k, "#F2CF68")
    cv.line(x - 0.3 * k, y, x + 4.6 * k, y, 1.6 * k, "#F2CF68")
    cv.line(x + 2.6 * k, y, x + 2.6 * k, y + 2.4 * k, 1.4 * k, "#F2CF68")
    cv.line(x + 4.5 * k, y, x + 4.5 * k, y + 2.0 * k, 1.4 * k, "#F2CF68")


def m_lock(cv, x, y, s=1.0, h=False):
    r = token(cv, x, y, C_LOCK, 8.5, s, h)
    k = s * r / (8.5 * s)
    cv.ring(x, y - 1.6 * k, 2.5 * k, 1.5 * k, "#D5E1E7")
    cv.rrect(x - 3.9 * k, y - 0.6 * k, x + 3.9 * k, y + 4.3 * k, 1.2 * k, "#D5E1E7")
    cv.circle(x, y + 1.7 * k, 0.9 * k, "#16120D")


def m_extract(cv, x, y, s=1.0, h=False):
    r = token(cv, x, y, C_EXTRACT, 11, s, h, ring_w=2.6)
    k = s * r / (11 * s)
    cv.polyline([(x - 4.6 * k, y + 0.2 * k), (x, y - 4.4 * k), (x + 4.6 * k, y + 0.2 * k)], 2.3 * k, "#EAF4FB")
    cv.line(x, y - 3.4 * k, x, y + 5.0 * k, 2.3 * k, "#EAF4FB")


def m_spawn(cv, x, y, s=1.0, h=False):
    r = 7 * s * (1.25 if h else 1)
    cv.ring(x, y, r, 4.6 * s, HALO, 0.55)
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        cv.line(x + dx * (r + 1.5 * s), y + dy * (r + 1.5 * s), x + dx * (r + 5.5 * s), y + dy * (r + 5.5 * s), 4.0 * s, HALO, 0.55)
    cv.ring(x, y, r, 2.0 * s, C_SPAWN)
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        cv.line(x + dx * (r + 1.5 * s), y + dy * (r + 1.5 * s), x + dx * (r + 5.5 * s), y + dy * (r + 5.5 * s), 2.0 * s, C_SPAWN)
    cv.circle(x, y, 2.0 * s, C_SPAWN)
    if h:
        focus_ring(cv, x, y, r + 4 * s)


def m_sound(cv, x, y, s=1.0, h=False, dust=False):
    if dust:  # dense / overview mode
        cv.circle(x, y, 3.2 * s, C_SOUND, 0.8)
        return
    r = 3.2 * s * (1.25 if h else 1)
    cv.circle(x, y, r + 1.4 * s, HALO, 0.5)
    cv.circle(x, y, r, C_SOUND)
    cv.ring(x, y, 7.5 * s, 1.3 * s, C_SOUND, 0.9, dashed=True)
    if h:
        focus_ring(cv, x, y, 8 * s)


def m_loot(cv, x, y, s=1.0, h=False, color=C_LOOT):
    r = 7 * s * (1.25 if h else 1)
    cv.diamond(x, y, r + 1.6 * s, HALO, 0.75)
    cv.diamond(x, y, r, color)
    cv.diamond(x, y, r * 0.35, "#FFF6D8", 0.85)
    if h:
        focus_ring(cv, x, y, r)


def m_harvest(cv, x, y, prof="Scavenger", s=1.0, h=False, locked=False):
    k = 5.5 * s * (1.25 if h else 1)
    cv.rrect(x - k - 1.6 * s, y - k - 1.6 * s, x + k + 1.6 * s, y + k + 1.6 * s, 2.6 * s, HALO, 0.75 if not locked else 0.45)
    if locked:
        cv.rrect(x - k, y - k, x + k, y + k, 1.8 * s, "#8C857A", 0.75)
        cv.line(x - k + 2 * s, y + k - 2 * s, x + k - 2 * s, y - k + 2 * s, 1.2 * s, "#2A241D", 0.8)
    else:
        cv.rrect(x - k, y - k, x + k, y + k, 1.8 * s, C_HARV[prof])
    if h:
        focus_ring(cv, x, y, k + 1 * s)


def m_place(cv, x, y, s=1.0, h=False):
    r = 8.5 * s * (1.25 if h else 1)
    cv.diamond(x, y, r + 2.4 * s, HALO, 0.65)
    cv.diamond(x, y, r, "#1A1510", 0.92)
    cv.diamond_stroke(x, y, r - 1.1 * s, 2.2 * s, C_PLACE)
    cv.diamond(x, y, 2.2 * s, C_PLACE)
    if h:
        focus_ring(cv, x, y, r)


def label_plaque(cv, x, y, text, s=1.0, serif=False):
    """Map label plaque, left edge at x, vertically centred on y. Returns its rect."""
    f = font(F_UI_SB, 12.5 * s)
    tw = cv.measure(text, f)
    l, t, r, b = x, y - 10 * s, x + tw + 12 * s, y + 10 * s
    cv.rrect(l, t, r, b, 4 * s, "#15110C", 0.82, stroke=LINE, sa=0.30, sw=1 * s)
    cv.text(l + 6 * s, y + 0.5 * s, text, f, T100)
    return (l, t, r, b)


# ----------------------------------------------------------------------------------------------
# Tooltip (DESIGN.md section 6)
# ----------------------------------------------------------------------------------------------
def tooltip(cv, mx, my, mr, accent, eyebrow, title, lines, footer=None, chance=None, avoid=None, s=1.0):
    pad_x, pad_y = 12 * s, 10 * s
    f_eye = font(F_UI_SB, 11 * s)
    f_title = font(F_UI_SB, 15.5 * s)
    f_line = font(F_UI, 13.5 * s)
    f_foot = font(F_UI, 12.5 * s)
    maxw = 320 * s
    w = max(cv.measure(eyebrow.upper(), f_eye, 1.1 * s) + 14 * s, cv.measure(title, f_title))
    for k, v in lines:
        w = max(w, cv.measure(k, f_line) + (cv.measure(v, f_line) + 8 * s if v else 0))
    if chance is not None:
        w = max(w, 220 * s)
    if footer:
        w = max(w, cv.measure(footer, f_foot) + 50 * s)
    w = min(w, maxw)
    bw = w + 2 * pad_x + 4 * s
    bh = pad_y + 16 * s + 24 * s + len(lines) * 20 * s + (26 * s if chance is not None else 0) + (30 * s if footer else 0) + pad_y - 2 * s
    bx = mx + mr + 14 * s
    by = my - 24 * s
    if bx + bw > cv.w - 8 or (avoid and bx + bw > avoid[0] and by < avoid[3] and bx < avoid[2]):
        bx = mx - mr - 14 * s - bw
    by = max(8, min(by, cv.h - bh - 8))
    # shadow + body + hairline + accent bar
    cv.rrect(bx + 2 * s, by + 3 * s, bx + bw + 2 * s, by + bh + 3 * s, 8 * s, "#000000", 0.35)
    cv.rrect(bx, by, bx + bw, by + bh, 8 * s, INK_950, 0.95, stroke=LINE, sa=0.38, sw=1 * s)
    cv.rrect(bx + 4 * s, by + 8 * s, bx + 7 * s, by + bh - 8 * s, 1.5 * s, accent)
    x = bx + pad_x + 4 * s
    y = by + pad_y + 7 * s
    cv.text(x, y, eyebrow.upper(), f_eye, T300, tracking=1.1 * s)
    y += 20 * s
    cv.text(x, y, cv.ellipsize(title, f_title, w), f_title, T100)
    y += 22 * s
    for k, v in lines:
        kw = cv.text(x, y, k, f_line, T300)
        if v:
            cv.text(x + kw + 6 * s, y, v, f_line, T100)
        y += 20 * s
    if chance is not None:
        y += 2 * s
        cv.text(x, y, "Chance", f_line, T300)
        bl = x + 62 * s
        br = x + w - 52 * s
        cv.rrect(bl, y - 3.5 * s, br, y + 3.5 * s, 3.5 * s, "#FFFFFF", 0.10)
        cv.rrect(bl, y - 3.5 * s, bl + (br - bl) * chance, y + 3.5 * s, 3.5 * s, accent)
        cv.text(x + w, y, f"{chance * 100:.0f} %", font(F_UI_SB, 13.5 * s), T100, anchor="rm")
        y += 24 * s
    if footer:
        y += 4 * s
        cv.line(x, y - 9 * s, x + w, y - 9 * s, 1 * s, LINE, 0.22)
        y += 4 * s
        kc = font(F_UI_SB, 11.5 * s)
        kw = cv.measure("Ctrl", kc)
        cv.rrect(x, y - 9 * s, x + kw + 12 * s, y + 9 * s, 4 * s, INK_700, 1.0, stroke=T500, sa=0.8, sw=1 * s)
        cv.text(x + 6 * s, y, "Ctrl", kc, T100)
        cv.text(x + kw + 18 * s, y, footer, f_foot, GOLD_HI)
    return (bx, by, bx + bw, by + bh)


# ----------------------------------------------------------------------------------------------
# Panel (DESIGN.md section 4)
# ----------------------------------------------------------------------------------------------
PW = 430
PAD = 14


class Panel:
    def __init__(self, cv, x, y, w=PW, s=1.0):
        self.cv, self.x, self.y0, self.w, self.s = cv, x, y, w, s
        self.ops = []  # deferred draw ops so the background can be sized first
        self.y = 0.0

    # Each op is a closure taking absolute (ox, oy).
    def add(self, fn):
        self.ops.append(fn)

    def finish(self, min_h=0, bottom_fade=False):
        s = self.s
        h = max(self.y + 12 * s, min_h)
        cv, ox, oy, w = self.cv, self.x, self.y0, self.w
        cv.rrect(ox + 3 * s, oy + 5 * s, ox + w + 3 * s, oy + h + 5 * s, 8 * s, "#000000", 0.35)
        cv.rrect(ox, oy, ox + w, oy + h, 7 * s, INK_950, 0.94, stroke="#000000", sa=0.7, sw=1 * s)
        cv.rrect(ox + 3 * s, oy + 3 * s, ox + w - 3 * s, oy + h - 3 * s, 5 * s, None, stroke=LINE, sa=0.32, sw=1 * s)
        for fn in self.ops:
            fn(ox, oy)
        return (ox, oy, ox + w, oy + h)

    # -- building blocks ------------------------------------------------------------------------
    def header(self, title="HUNGER COMPANION", collapsed=False, summary=None, status=OK):
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            cy = oy + y + 23 * s
            # grip: 2x3 dots
            for i in range(3):
                for j in range(2):
                    cv.circle(ox + PAD * s + 2 + j * 5 * s, cy - 5 * s + i * 5 * s, 1.5 * s, T500)
            tx = ox + PAD * s + 16 * s
            tw = cv.text(tx, cy + 1 * s, title, font(F_DISPLAY, 14.5 * s), GOLD, tracking=2.0 * s)
            cv.circle(tx + tw + 10 * s, cy, 3.5 * s, status)
            if summary:
                cv.text(tx + tw + 22 * s, cy + 1 * s, summary, font(F_UI, 13 * s), T300)
            # icon buttons (right): collapse, settings
            for k, ic in enumerate(["settings", "right" if collapsed else "down"]):
                bx = ox + w - PAD * s - 28 * s - k * 32 * s
                cv.rrect(bx, cy - 14 * s, bx + 28 * s, cy + 14 * s, 6 * s, INK_800, 0.9, stroke="#FFFFFF", sa=0.08, sw=1 * s)
                cv.text(bx + 14 * s, cy + 0.5 * s, ICON[ic], font(F_ICON, 13 * s if ic == "settings" else 11 * s), T300, anchor="mm")
            if not collapsed:
                cv.line(ox + PAD * s, oy + y + 46 * s, ox + w - PAD * s, oy + y + 46 * s, 1 * s, LINE, 0.18)
        self.add(op)
        self.y += (46 if not collapsed else 34) * s

    def search_field(self, hover=False):
        s, w = self.s, self.w
        y = self.y + 12 * s

        def op(ox, oy):
            cv = self.cv
            l, t, r, b = ox + PAD * s, oy + y, ox + w - PAD * s, oy + y + 34 * s
            cv.rrect(l, t, r, b, 6 * s, "#0B0906", 0.75, stroke=GOLD if hover else "#FFFFFF", sa=0.5 if hover else 0.10, sw=1 * s)
            cv.text(l + 13 * s, (t + b) / 2 + 0.5 * s, ICON["search"], font(F_ICON, 13 * s), T500, anchor="lm")
            cv.text(l + 34 * s, (t + b) / 2, "Find an item or quest…", font(F_UI, 14 * s), T500)
        self.add(op)
        self.y = y + 34 * s

    def quick_row(self, show_done=False, chance=10, lang="EN"):
        s, w = self.s, self.w
        y = self.y + 8 * s

        def op(ox, oy):
            cv = self.cv
            t, b = oy + y, oy + y + 30 * s
            cy = (t + b) / 2
            f = font(F_UI, 13.5 * s)
            # "Show done" toggle chip
            x = ox + PAD * s
            lab = "Show done"
            cw = cv.measure(lab, f) + 40 * s
            cv.rrect(x, t, x + cw, b, 6 * s, INK_800, 0.95, stroke="#FFFFFF", sa=0.08, sw=1 * s)
            cv.rrect(x + 10 * s, cy - 6.5 * s, x + 23 * s, cy + 6.5 * s, 3 * s, ACCENT if show_done else None, stroke=T300 if not show_done else ACCENT, sa=0.9, sw=1.3 * s)
            if show_done:
                cv.text(x + 16.5 * s, cy + 0.5 * s, ICON["check"], font(F_ICON, 9 * s), "#17120C", anchor="mm")
            cv.text(x + 30 * s, cy, lab, f, T200)
            x += cw + 8 * s
            # chance stepper [−] Chance ≥ 10 % [+]
            mid = f"Chance ≥ {chance} %"
            mw = cv.measure(mid, f) + 20 * s
            sw_ = 28 * s * 2 + mw
            cv.rrect(x, t, x + sw_, b, 6 * s, INK_800, 0.95, stroke="#FFFFFF", sa=0.08, sw=1 * s)
            cv.line(x + 28 * s, t + 6 * s, x + 28 * s, b - 6 * s, 1 * s, "#FFFFFF", 0.10)
            cv.line(x + 28 * s + mw, t + 6 * s, x + 28 * s + mw, b - 6 * s, 1 * s, "#FFFFFF", 0.10)
            cv.text(x + 14 * s, cy + 0.5 * s, ICON["remove"], font(F_ICON, 11 * s), T200, anchor="mm")
            cv.text(x + 28 * s + mw / 2, cy, mid, f, T100, anchor="mm")
            cv.text(x + 42 * s + mw, cy + 0.5 * s, ICON["add"], font(F_ICON, 11 * s), T200, anchor="mm")
            # language chip, right-aligned
            lw = cv.measure(lang, f) + 38 * s
            lx = ox + w - PAD * s - lw
            cv.rrect(lx, t, lx + lw, b, 6 * s, INK_800, 0.95, stroke="#FFFFFF", sa=0.08, sw=1 * s)
            cv.text(lx + 11 * s, cy + 0.5 * s, ICON["globe"], font(F_ICON, 12 * s), T300, anchor="lm")
            cv.text(lx + 29 * s, cy, lang, font(F_UI_SB, 13 * s), T200)
        self.add(op)
        self.y = y + 30 * s

    def toast(self, text, action="Undo"):
        s, w = self.s, self.w
        y = self.y + 10 * s

        def op(ox, oy):
            cv = self.cv
            l, t, r, b = ox + PAD * s, oy + y, ox + w - PAD * s, oy + y + 32 * s
            cv.rrect(l, t, r, b, 6 * s, OK, 0.13, stroke=OK, sa=0.45, sw=1 * s)
            cy = (t + b) / 2
            cv.text(l + 11 * s, cy + 0.5 * s, ICON["check"], font(F_ICON, 12 * s), OK, anchor="lm")
            cv.text(l + 31 * s, cy, text, font(F_UI, 13.5 * s), T100)
            aw = cv.measure(action, font(F_UI_SB, 13 * s))
            cv.text(r - 12 * s - aw, cy, action, font(F_UI_SB, 13 * s), GOLD_HI)
            cv.line(r - 12 * s - aw, cy + 9 * s, r - 12 * s, cy + 9 * s, 1 * s, GOLD_HI, 0.6)
        self.add(op)
        self.y = y + 32 * s

    def section(self, title, count=None, collapsed=False, right=None, icon=None):
        s, w = self.s, self.w
        y = self.y + 12 * s

        def op(ox, oy):
            cv = self.cv
            cv.line(ox + PAD * s, oy + y, ox + w - PAD * s, oy + y, 1 * s, LINE, 0.16)
            cy = oy + y + 19 * s
            cv.text(ox + PAD * s, cy + 0.5 * s, ICON["right" if collapsed else "down"], font(F_ICON, 9 * s), T500, anchor="lm")
            tx = ox + PAD * s + 16 * s
            tw = cv.text(tx, cy + 1 * s, title, font(F_DISPLAY, 12.5 * s), GOLD, tracking=1.8 * s)
            if count is not None:
                t = str(count)
                f = font(F_UI_SB, 11 * s)
                cw = cv.measure(t, f) + 12 * s
                px = tx + tw + 9 * s
                cv.rrect(px, cy - 8 * s, px + cw, cy + 8 * s, 8 * s, INK_700, 1.0)
                cv.text(px + cw / 2, cy + 0.5 * s, t, f, T200, anchor="mm")
            if right:
                cv.text(ox + w - PAD * s, cy, right, font(F_UI, 12.5 * s), T500, anchor="rm")
        self.add(op)
        self.y = y + 34 * s

    def owner_row(self, badge, color, title, right=None, hover=False, removable=False, hidden=False, new=False):
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            t, b = oy + y, oy + y + 28 * s
            cy = (t + b) / 2
            if hover:
                cv.rrect(ox + PAD * s - 6 * s, t, ox + w - PAD * s + 6 * s, b, 5 * s, "#FFFFFF", 0.07)
            a = 0.4 if hidden else 1.0
            bx = ox + PAD * s + 10 * s
            cv.circle(bx, cy, 10 * s, HALO, 0.5 * a)
            cv.circle(bx, cy, 9.5 * s, color, a)
            cv.ring(bx, cy, 9 * s, 1.4 * s, "#FFFFFF", 0.9 * a)
            badge_text(cv, bx, cy, badge, color, 11.5 * s)
            tx = ox + PAD * s + 28 * s
            avail = w - 2 * PAD * s - 28 * s - (90 * s if right or removable else 0)
            tw = cv.text(tx, cy, cv.ellipsize(title, font(F_UI_SB, 15 * s), avail), font(F_UI_SB, 15 * s), T500 if hidden else T100)
            if new:
                f = font(F_UI_SB, 10 * s)
                nx = tx + tw + 8 * s
                nw = cv.measure("NEW", f, 0.8 * s) + 10 * s
                cv.rrect(nx, cy - 8 * s, nx + nw, cy + 8 * s, 4 * s, OK, 0.18, stroke=OK, sa=0.5, sw=1 * s)
                cv.text(nx + 5 * s, cy + 0.5 * s, "NEW", f, OK, tracking=0.8 * s)
            rx = ox + w - PAD * s
            if removable:
                cv.rrect(rx - 22 * s, cy - 11 * s, rx, cy + 11 * s, 5 * s, INK_800, 0.95, stroke="#FFFFFF", sa=0.08, sw=1 * s)
                cv.text(rx - 11 * s, cy + 0.5 * s, ICON["close"], font(F_ICON, 9 * s), T300, anchor="mm")
                rx -= 30 * s
            if hidden:
                cv.text(rx, cy + 0.5 * s, ICON["hide"], font(F_ICON, 13 * s), T500, anchor="rm")
            elif right:
                cv.text(rx, cy, right, font(F_UI, 12.5 * s), T500, anchor="rm")
        self.add(op)
        self.y += 30 * s

    def objective(self, label, state="here", color=None, hint=None):
        """state: here (dot in owner colour) / elsewhere (hollow) / done (check)."""
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            cy = oy + y + 11 * s
            gx = ox + PAD * s + 34 * s
            if state == "here":
                cv.circle(gx, cy, 4 * s, color)
            elif state == "elsewhere":
                cv.ring(gx, cy, 3.6 * s, 1.3 * s, T500)
            else:
                cv.text(gx, cy + 0.5 * s, ICON["check"], font(F_ICON, 10 * s), T500, anchor="mm")
            col = T200 if state == "here" else (T300 if state == "elsewhere" else T500)
            cv.text(gx + 12 * s, cy, cv.ellipsize(label, font(F_UI, 14 * s), w - 2 * PAD * s - 50 * s), font(F_UI, 14 * s), col)
            if hint:
                cv.text(gx + 12 * s, cy + 19 * s, hint, font(F_UI, 12.5 * s), T500)
        self.add(op)
        self.y += (22 + (19 if hint else 0)) * s

    def note(self, parts, indent=28):
        """Dim single line made of (text, colour) parts, e.g. 'Not on this map: ② Still Alive'."""
        s = self.s
        y = self.y

        def op(ox, oy):
            cv = self.cv
            cy = oy + y + 12 * s
            x = ox + PAD * s + indent * s
            for p in parts:
                if isinstance(p, tuple) and p[0] == "badge":
                    _, b, c = p
                    cv.circle(x + 7 * s, cy, 7 * s, c, 0.55)
                    badge_text(cv, x + 7 * s, cy, b, c, 9.5 * s)
                    x += 19 * s
                else:
                    t, c = p
                    x += cv.text(x, cy, t, font(F_UI, 13 * s), c)
        self.add(op)
        self.y += 26 * s

    def warning(self, text, action):
        s, w = self.s, self.w
        y = self.y + 4 * s

        def op(ox, oy):
            cv = self.cv
            l, t, r, b = ox + PAD * s + 28 * s, oy + y, ox + w - PAD * s, oy + y + 34 * s
            cv.rrect(l, t, r, b, 6 * s, WARN, 0.10, stroke=WARN, sa=0.38, sw=1 * s)
            cy = (t + b) / 2
            cv.text(l + 10 * s, cy + 0.5 * s, ICON["warn"], font(F_ICON, 12 * s), WARN, anchor="lm")
            cv.text(l + 30 * s, cy, text, font(F_UI, 13 * s), "#F3C38A")
            f = font(F_UI_SB, 12 * s)
            aw = cv.measure(action, f) + 16 * s
            cv.rrect(r - 6 * s - aw, cy - 11 * s, r - 6 * s, cy + 11 * s, 5 * s, WARN, 0.18, stroke=WARN, sa=0.6, sw=1 * s)
            cv.text(r - 6 * s - aw / 2, cy + 0.5 * s, action, f, "#FFD7A8", anchor="mm")
        self.add(op)
        self.y = y + 34 * s + 6 * s

    def subheader(self, title, count=None, right=None):
        s, w = self.s, self.w
        y = self.y + 6 * s

        def op(ox, oy):
            cv = self.cv
            cy = oy + y + 10 * s
            f = font(F_UI_SB, 11 * s)
            tw = cv.text(ox + PAD * s, cy, title.upper(), f, T500, tracking=1.2 * s)
            if count is not None:
                cv.text(ox + PAD * s + tw + 8 * s, cy, str(count), f, T300)
            if right:
                cv.text(ox + w - PAD * s, cy, right, font(F_UI, 12 * s), T500, anchor="rm")
        self.add(op)
        self.y = y + 22 * s

    def avail_row(self, title, giver, hover=False):
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            t, b = oy + y, oy + y + 28 * s
            cy = (t + b) / 2
            if hover:
                cv.rrect(ox + PAD * s - 6 * s, t, ox + w - PAD * s + 6 * s, b, 5 * s, "#FFFFFF", 0.07)
            m_quest_avail(cv, ox + PAD * s + 10 * s, cy, s * 0.82)
            tx = ox + PAD * s + 28 * s
            f = font(F_UI, 14 * s)
            tw = cv.text(tx, cy, title, f, T200)
            cv.text(tx + tw + 8 * s, cy, giver, font(F_UI, 12.5 * s), T500)
            f2 = font(F_UI_SB, 12 * s)
            lab = "Track"
            bw = cv.measure(lab, f2) + 30 * s
            r = ox + w - PAD * s
            cv.rrect(r - bw, cy - 11 * s, r, cy + 11 * s, 5 * s, GOLD if hover else INK_800, 0.22 if hover else 0.9, stroke=GOLD, sa=0.55, sw=1 * s)
            cv.text(r - bw + 10 * s, cy + 0.5 * s, ICON["add"], font(F_ICON, 9 * s), GOLD_HI, anchor="lm")
            cv.text(r - bw + 23 * s, cy, lab, f2, GOLD_HI)
        self.add(op)
        self.y += 30 * s

    def link_row(self, text, indent=28):
        s = self.s
        y = self.y

        def op(ox, oy):
            cv = self.cv
            cy = oy + y + 11 * s
            x = ox + PAD * s + indent * s
            tw = cv.text(x, cy, text, font(F_UI_SB, 12.5 * s), GOLD)
            cv.text(x + tw + 6 * s, cy + 1 * s, ICON["down"], font(F_ICON, 8 * s), GOLD, anchor="lm")
        self.add(op)
        self.y += 24 * s

    def layer_group(self, title, layers):
        """layers: list of (name, count, on, swatch_fn, tag)."""
        s, w = self.s, self.w
        self.subheader(title)
        y0 = self.y
        colw = (w - 2 * PAD * s - 6 * s) / 2
        rows = (len(layers) + 1) // 2

        def op(ox, oy):
            cv = self.cv
            for i, (name, count, on, sw, tag) in enumerate(layers):
                cx = ox + PAD * s + (i % 2) * (colw + 6 * s)
                t = oy + y0 + (i // 2) * 36 * s
                b = t + 30 * s
                cy = (t + b) / 2
                if on:
                    cv.rrect(cx, t, cx + colw, b, 6 * s, INK_700, 1.0, stroke=GOLD, sa=0.55, sw=1 * s)
                else:
                    cv.rrect(cx, t, cx + colw, b, 6 * s, "#000000", 0.18, stroke="#FFFFFF", sa=0.08, sw=1 * s)
                cv.fade = 1.0 if on else 0.42
                sw(cv, cx + 15 * s, cy, 0.8 * s)
                cv.fade = 1.0
                f = font(F_UI, 13.5 * s)
                cnt = str(count)
                fc = font(F_UI, 12 * s)
                cw = cv.measure(cnt, fc)
                avail = colw - 34 * s - cw - 14 * s
                nm = cv.ellipsize(name, f, avail - (34 * s if tag else 0))
                nw = cv.text(cx + 31 * s, cy, nm, f, T100 if on else T500)
                if tag:
                    ft = font(F_UI_SB, 9.5 * s)
                    tx = cx + 31 * s + nw + 6 * s
                    tw = cv.measure(tag, ft, 0.6 * s) + 8 * s
                    cv.rrect(tx, cy - 7 * s, tx + tw, cy + 7 * s, 3.5 * s, ACCENT, 0.9 if on else 0.35)
                    cv.text(tx + 4 * s, cy + 0.5 * s, tag, ft, "#17120C", tracking=0.6 * s)
                cv.text(cx + colw - 10 * s, cy, cnt, fc, T300 if on else T500, anchor="rm")
        self.add(op)
        self.y = y0 + rows * 36 * s - 2 * s

    def empty(self, lines):
        s, w = self.s, self.w
        y = self.y + 4 * s

        def op(ox, oy):
            cv = self.cv
            cy = oy + y + 11 * s
            for i, (t, c, fn) in enumerate(lines):
                cv.text(ox + PAD * s, cy + i * 21 * s, t, font(fn, 13.5 * s), c)
        self.add(op)
        self.y = y + len(lines) * 21 * s + 4 * s

    def button(self, text, primary=True, indent=0):
        s = self.s
        y = self.y + 8 * s

        def op(ox, oy):
            cv = self.cv
            f = font(F_UI_SB, 13 * s)
            bw = cv.measure(text, f) + 28 * s
            l = ox + PAD * s + indent * s
            cv.rrect(l, oy + y, l + bw, oy + y + 30 * s, 6 * s, ACCENT if primary else INK_800, 1.0, stroke=ACCENT, sa=0.9, sw=1 * s)
            cv.text(l + bw / 2, oy + y + 15 * s, text, f, "#17120C" if primary else T100, anchor="mm")
        self.add(op)
        self.y = y + 30 * s

    def overflow_footer(self, text):
        s, w = self.s, self.w
        y = self.y + 6 * s

        def op(ox, oy):
            cv = self.cv
            # fade-out band suggesting more content, then a "more" line
            for i in range(10):
                cv.rrect(ox + 4 * s, oy + y - 30 * s + i * 3 * s, ox + w - 4 * s, oy + y - 27 * s + i * 3 * s, 0, INK_950, 0.07 * (i + 1))
            cv.text(ox + w / 2, oy + y + 6 * s, text + "   ", font(F_UI_SB, 12.5 * s), GOLD, anchor="mm")
            tw = cv.measure(text, font(F_UI_SB, 12.5 * s))
            cv.text(ox + w / 2 + tw / 2 + 2 * s, oy + y + 7 * s, ICON["down"], font(F_ICON, 9 * s), GOLD, anchor="lm")
        self.add(op)
        self.y = y + 16 * s


# Layer swatches (small versions of the real markers)
SW = {
    "extract": lambda cv, x, y, s: m_extract(cv, x, y, s * 0.82),
    "spawn": lambda cv, x, y, s: m_spawn(cv, x, y, s * 0.82),
    "trap": lambda cv, x, y, s: m_trap(cv, x, y, s),
    "sound": lambda cv, x, y, s: m_sound(cv, x, y, s * 1.05),
    "boss": lambda cv, x, y, s: m_boss(cv, x, y, s * 0.95),
    "loot": lambda cv, x, y, s: m_loot(cv, x, y, s * 1.1),
    "harvest": lambda cv, x, y, s: (m_harvest(cv, x - 4 * s, y + 3 * s, "Naturalist", s * 0.85), m_harvest(cv, x + 3 * s, y - 3 * s, "Scavenger", s * 0.85)),
    "key": lambda cv, x, y, s: m_key(cv, x, y, s),
    "lock": lambda cv, x, y, s: m_lock(cv, x, y, s),
    "quest": lambda cv, x, y, s: m_quest_avail(cv, x, y, s),
    "place": lambda cv, x, y, s: m_place(cv, x, y, s * 0.95),
}


def layers_block(p, on):
    p.layer_group("Survival", [
        ("Extractions", 23, "extractions" in on, SW["extract"], None),
        ("Player spawns", 18, "player_spawns" in on, SW["spawn"], None),
        ("Minibosses", 24, "minibosses" in on, SW["boss"], None),
        ("Traps", 21, "traps" in on, SW["trap"], None),
        ("Sound traps", 289, "sound_traps" in on, SW["sound"], None),
    ])
    p.layer_group("Loot & access", [
        ("Placed loot", 68, "placed_loot" in on, SW["loot"], None),
        ("Harvest · your levels", 123, "harvest" in on, SW["harvest"], None),
        ("Key spots", 5, "keys" in on, SW["key"], None),
        ("Locked", 56, "locks" in on, SW["lock"], None),
    ])
    p.layer_group("World", [
        ("Quests", 25, "quests" in on, SW["quest"], "NEW"),
        ("Places", 31, "places" in on, SW["place"], None),
    ])


# ----------------------------------------------------------------------------------------------
# Data (real markers from the cached pack)
# ----------------------------------------------------------------------------------------------
N_SPOTS = {}


def load_pack():
    path = os.path.join(os.environ.get("APPDATA", ""), "HungerOverlay", "pack.json")
    with open(path, encoding="utf-8") as f:
        return json.load(f)


TRACKED = [  # (quest id, badge, colour) - colours = the website's tracker colours
    ("Quest.Eloise.Q04", "1", "#e9745e"),   # Making Dough
    ("Quest.Louis.Q01", "2", "#5ec5e9"),    # Still Alive (other map)
    ("Quest.MrSweet.Q01", "3", "#7ed957"),  # Sweet Lubrication
]
CRAFT = ("A", "#c77dff")  # Grubber's Purse
SEARCH = ("B", "#F5D547")  # Iron Ore


def gather(pack, min_chance=0.10):
    out = {"tracked": [], "avail": [], "layers": {}}
    tracked_ids = {q for q, _, _ in TRACKED}
    for qid, badge, col in TRACKED:
        q = pack["quests"][qid]
        seen = set()
        for o in q["objectives"]:
            for m in o["markers"]:
                key = (round(m["px"], 1), round(m["py"], 1))
                if key in seen:  # one marker per spot and owner (wine + mead share containers)
                    continue
                seen.add(key)
                if m["map"] == "Map01" and (m.get("chance") is None or m["chance"] >= min_chance):
                    out["tracked"].append(dict(px=m["px"], py=m["py"], badge=badge, color=col, quest=q["title"],
                                               obj=o["label"], container=m.get("container"), chance=m.get("chance")))
    for qid, q in pack["quests"].items():
        if qid in tracked_ids:
            continue
        for o in q["objectives"]:
            ms = [m for m in o["markers"] if m["map"] == "Map01"]
            if not ms:
                continue
            if o.get("kind") == "map_coords" or (o.get("kind") == "unresolved" and len(o["markers"]) <= 4):
                for m in ms:
                    out["avail"].append(dict(px=m["px"], py=m["py"], quest=q["title"], giver=q.get("giver"), obj=o["label"], qid=qid))
    for l in pack["layers"]:
        out["layers"][l["id"]] = [m for m in l["markers"] if m["map"] == "Map01"]
    out["iron"] = [m for m in out["layers"]["harvest_scavenger"] if m.get("node") == "Iron Ore"]
    return out


def cluster(points, radius):
    """Greedy screen-space clustering: points = [(x, y, payload)] -> [(x, y, [payloads])]."""
    res = []
    for x, y, p in points:
        for c in res:
            if (c[0] - x) ** 2 + (c[1] - y) ** 2 <= radius * radius:
                c[2].append(p)
                n = len(c[2])
                c[0] += (x - c[0]) / n
                c[1] += (y - c[1]) / n
                break
        else:
            res.append([x, y, [p]])
    return res


# ----------------------------------------------------------------------------------------------
# Scenes
# ----------------------------------------------------------------------------------------------
def make_proj(scale, tx, ty):
    return lambda px, py: (px * scale + tx, py * scale + ty)


PROJ_DEFAULT = make_proj(0.33342, 597.11, 37.27)      # measured on z0/T2 (17 markers, 0.5 px error)
PROJ_ZOOMED = make_proj(0.83397, -226.97, -788.33)    # measured on S1 (29 markers, 1 px error)
MAP_DEFAULT = (597, 37, 1963, 1403)                    # map texture square on screen (z0)


def inside(x, y, rect, m=0):
    return rect[0] - m <= x <= rect[2] + m and rect[1] - m <= y <= rect[3] + m


def draw_tracked(cv, data, proj, clip, zoom):
    # one cluster pass per owner; at overview zoom clusters are tighter than at close zoom
    rad = 24 if zoom < 1.5 else 16
    by_owner = {}
    for t in data["tracked"]:
        x, y = proj(t["px"], t["py"])
        if inside(x, y, clip):
            by_owner.setdefault((t["badge"], t["color"]), []).append((x, y, t))
    groups = []
    for (b, c), pts in by_owner.items():
        for x, y, items in cluster(pts, rad):
            groups.append((x, y, b, c, items))
    return spread(cv, groups, 22)


def spread(cv, groups, mind):
    """Different owners overlapping at the same spot: fan them out on a small circle around the true
    centroid, each tied to it by a thin leader line (DESIGN.md 5.4 'petals')."""
    sets = []
    for g in groups:
        hit = [st for st in sets if any((g[0] - h[0]) ** 2 + (g[1] - h[1]) ** 2 < mind * mind for h in st)]
        merged = [g]
        for st in hit:
            merged += st
            sets.remove(st)
        sets.append(merged)
    out = []
    for st in sets:
        if len(st) == 1:
            out.append(st[0])
            continue
        cx = sum(g[0] for g in st) / len(st)
        cy = sum(g[1] for g in st) / len(st)
        R = 14 if len(st) == 2 else 18
        st.sort(key=lambda g: g[2])
        cv.circle(cx, cy, 3.2, HALO, 0.7)
        cv.circle(cx, cy, 2.0, "#F3ECDD")
        for i, g in enumerate(st):
            a = -math.pi / 2 + (i - (len(st) - 1) / 2) * (2.1 if len(st) == 2 else 1.6) if len(st) <= 3 else -math.pi / 2 + i * 2 * math.pi / len(st)
            if len(st) == 2:
                a = math.pi if i == 0 else 0.0
            nx, ny = cx + R * math.cos(a), cy + R * math.sin(a)
            cv.line(cx, cy, nx, ny, 2.6, HALO, 0.55)
            cv.line(cx, cy, nx, ny, 1.2, "#F3ECDD", 0.9)
            out.append((nx, ny, g[2], g[3], g[4]))
    return out


def scene_default(pack, data):
    base = Image.open(os.path.join(HERE, "base_default.png")).convert("RGB")
    W, H = base.size
    cv = Canvas(W, H)
    proj = PROJ_DEFAULT
    clip = MAP_DEFAULT
    panel_rect_guess = (1987, 60, 2417, 1050)
    # z-order: layers (extractions) < quest layer < search < tracked < labels < hovered < tooltip
    for m in data["layers"]["extractions"]:
        x, y = proj(m["px"], m["py"])
        if inside(x, y, clip, -6):
            m_extract(cv, x, y)
    hovered = None
    av = [(proj(a["px"], a["py"]) + (a,)) for a in data["avail"]]
    for x, y, items in cluster([(x, y, a) for x, y, a in av if inside(x, y, clip)], 16):
        if any(i["obj"].startswith("Locate the Gunline") for i in items):
            hovered = (x, y, items)
            continue
        m_quest_avail(cv, x, y)
        if len({i["qid"] for i in items}) > 1:
            count_pill(cv, x, y, len({i["qid"] for i in items}), C_QUEST_AVAIL)
    # item search: Iron Ore harvest nodes = squares in the search colour with its badge
    for m in data["iron"]:
        x, y = proj(m["px"], m["py"])
        if inside(x, y, clip):
            m_tracked_square(cv, x, y, SEARCH[1], SEARCH[0], 0.9)
    for x, y, b, c, items in draw_tracked(cv, data, proj, clip, 1.0):
        if len(items) > 1:
            m_cluster(cv, x, y, c, b, len(items))
        else:
            m_tracked_disc(cv, x, y, c, b)
    # hovered quest-layer marker + tooltip
    hx, hy, items = hovered
    m_quest_avail(cv, hx, hy, h=True)

    # Panel
    p = Panel(cv, 1987, 60)
    p.header()
    p.search_field()
    p.quick_row(show_done=False, chance=10, lang="EN")
    n_md = sum(1 for t in data["tracked"] if t["badge"] == "1")
    n_sl = sum(1 for t in data["tracked"] if t["badge"] == "3")
    p.section("QUESTS", 3)
    p.owner_row("1", "#e9745e", "Making Dough", right=f"{n_md} spots")
    p.objective("Find 20x Flour.", "here", "#e9745e")
    p.owner_row("3", "#7ed957", "Sweet Lubrication", right=f"{n_sl} spots")
    p.objective("Find 3x Tavern Wine.", "here", "#7ed957")
    p.objective("Find 3x Thin Mead.", "here", "#7ed957")
    p.note([("Other map:  ", T500), ("badge", "2", "#5ec5e9"), ("Still Alive", T300)])
    n_av = len({a["qid"] for a in data["avail"]})
    p.subheader("Available on this map", n_av, right="Quests layer")
    p.avail_row("Mapping the Front", "Eloise", hover=True)
    p.avail_row("Signs Of Life", "Eloise")
    p.avail_row("A Gift for the Giver", "Louis")
    p.link_row(f"Show {n_av - 3} more")
    p.section("CRAFTING", 1)
    p.owner_row("A", CRAFT[1], "Grubber's Purse")
    p.objective("Bolt of Cotton Cloth ×7", "elsewhere")
    p.objective("Delicate Chain ×1", "elsewhere")
    p.warning("Only below your 10 % filter here", "Show ≥ 1 %")
    p.section("SEARCH", 1)
    p.owner_row("B", SEARCH[1], "Iron Ore", right=f"{len(data['iron'])} spots", removable=True)
    p.section("MAP LAYERS", None, right="2 on")
    layers_block(p, {"extractions", "quests"})
    prect = p.finish()

    # Tooltip on the hovered "!" (Mapping the Front - Locate the Gunline Fort)
    mtf = [a for a in data["avail"] if a["quest"] == "Mapping the Front"]
    tooltip(cv, hx, hy, 11, C_QUEST_AVAIL, "Quest · not tracked", "Mapping the Front",
            [("Locate the Gunline Fort.", ""), ("Giver", "Eloise"), ("On this map", f"{len(mtf)} objectives")],
            footer="+ click to track this quest", avoid=prect)
    out = composite(base, cv)
    out.save(os.path.join(HERE, "mockup_default.png"))
    return prect


def clean_zoomed_base():
    """Paints the OLD overlay out of S1 (coloured markers) so the new design is not drawn over it."""
    base = Image.open(os.path.join(HERE, "base_zoomed.png")).convert("RGB")
    a = np.asarray(base).astype(np.int16)
    sat = a.max(axis=2) - a.min(axis=2)
    mask = sat > 45
    mask[1040:1125, 2490:2560] = False  # keep the game's own red player arrow
    # opening first: paper-texture specks (1-3 px) must not become blotches; markers (16 px discs) survive
    m = Image.fromarray((mask * 255).astype(np.uint8)).filter(ImageFilter.MinFilter(5)).filter(ImageFilter.MaxFilter(17))
    keep = 1.0 - np.asarray(m, dtype=np.float32) / 255.0
    img = np.asarray(base, dtype=np.float32)
    blur = lambda arr: np.asarray(Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), "L").filter(ImageFilter.GaussianBlur(7)), dtype=np.float32)
    num = np.stack([blur(img[..., c] * keep) for c in range(3)], -1)
    den = (blur(keep * 255.0) / 255.0)[..., None]
    fill = num / np.maximum(den, 1e-3)
    out = img * keep[..., None] + fill * (1 - keep[..., None])
    # the old panel (1986..2416 x 60..826) would ghost through the new 94 %-opaque one: replace it with
    # the parchment just to its left (hidden under the new panel anyway)
    out[52:836, 1980:2424] = img[52:836, 1980 - 450:2424 - 450]
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8))


def scene_zoomed(pack, data):
    base = clean_zoomed_base()
    W, H = base.size
    cv = Canvas(W, H)
    proj = PROJ_ZOOMED
    clip = (0, 0, W, H)
    s = 1.0
    L = data["layers"]
    pr = (1987, 60, 2417, 1380)  # panel rect (for label avoidance)
    vis = lambda x, y: inside(x, y, clip, -4) and not inside(x, y, pr, 14)
    # 1. dense / resource layers
    for m in L["sound_traps"]:
        x, y = proj(m["px"], m["py"])
        if vis(x, y):
            m_sound(cv, x, y)
    levels = {"Conservator": 75, "Naturalist": 75, "Scavenger": 75}
    for lid, prof in (("harvest_naturalist", "Naturalist"), ("harvest_scavenger", "Scavenger"), ("harvest_conservator", "Conservator")):
        for m in L[lid]:
            x, y = proj(m["px"], m["py"])
            # layer = "your levels": nodes above the player's level are not drawn (only searched items show them greyed)
            if vis(x, y) and m.get("node") != "Iron Ore" and (m.get("required_level") or 0) <= levels[prof]:
                m_harvest(cv, x, y, prof)
    for m in L["placed_loot"]:
        x, y = proj(m["px"], m["py"])
        if vis(x, y):
            m_loot(cv, x, y)
    obst = []  # marker footprints that labels must not cover
    for lid, fn in (("locks", m_lock), ("keys", m_key), ("traps", m_trap), ("minibosses", m_boss)):
        for m in L[lid]:
            x, y = proj(m["px"], m["py"])
            if vis(x, y):
                fn(cv, x, y)
                obst.append((x - 10, y - 10, x + 10, y + 10))
    # 2. places (diamond + label, labels placed later)
    places = []
    seen = set()
    for m in L["places"]:
        x, y = proj(m["px"], m["py"])
        if vis(x, y) and m["label"] not in seen:
            seen.add(m["label"])
            m_place(cv, x, y)
            places.append((x, y, m["label"]))
    # 3. quest layer
    for x, y, items in cluster([(proj(a["px"], a["py"]) + (a,)) for a in data["avail"]], 16):
        if vis(x, y):
            m_quest_avail(cv, x, y)
            obst.append((x - 10, y - 10, x + 10, y + 10))
            n = len({i["qid"] for i in items})
            if n > 1:
                count_pill(cv, x, y, n, C_QUEST_AVAIL)
    # 4. spawns + extractions
    for m in L["player_spawns"]:
        x, y = proj(m["px"], m["py"])
        if vis(x, y):
            m_spawn(cv, x, y)
    for m in L["extractions"]:
        x, y = proj(m["px"], m["py"])
        if vis(x, y):
            m_extract(cv, x, y)
    # 5. tracked (search, then quests), hovered one kept for last
    for m in data["iron"]:
        x, y = proj(m["px"], m["py"])
        if vis(x, y):
            m_tracked_square(cv, x, y, SEARCH[1], SEARCH[0])
    hover = None
    groups = draw_tracked(cv, data, proj, clip, 2.5)
    for x, y, b, c, items in groups:
        if not vis(x, y):
            continue
        if hover is None and b == "1" and len(items) == 1 and 800 < x < 1300 and 380 < y < 650:
            hover = (x, y, items[0])
            continue
        if len(items) > 1:
            m_cluster(cv, x, y, c, b, len(items))
        else:
            m_tracked_disc(cv, x, y, c, b)
        obst.append((x - 13, y - 13, x + 13, y + 13))
    # 6. labels with collision avoidance (right, left, above, below; drop if no slot)
    taken = [pr] + obst
    f = font(F_UI_SB, 12.5)

    def free(r):
        return all(r[2] < t[0] or r[0] > t[2] or r[3] < t[1] or r[1] > t[3] for t in taken) and r[0] > 4 and r[2] < W - 4 and r[1] > 4 and r[3] < H - 4

    for x, y, lab in places:
        tw = cv.measure(lab, f) + 12
        cands = [(x + 14, y), (x - 14 - tw, y), (x - tw / 2, y - 22), (x - tw / 2, y + 22)]
        for lx, ly in cands:
            r = (lx, ly - 10, lx + tw, ly + 10)
            if free(r):
                label_plaque(cv, lx, ly, lab)
                taken.append((r[0] - 4, r[1] - 3, r[2] + 4, r[3] + 3))
                break
    # 7. hovered tracked container + tooltip
    hx, hy, it = hover
    m_tracked_disc(cv, hx, hy, it["color"], it["badge"], h=True)

    # Panel (state: just tracked "Mapping the Front" from the map with Ctrl+click)
    p = Panel(cv, 1987, 60)
    p.header()
    p.toast("Now tracking  Mapping the Front", "Undo")
    p.search_field()
    p.quick_row(show_done=True, chance=10, lang="EN")
    p.section("QUESTS", 4)
    p.owner_row("1", "#e9745e", "Making Dough", right=f"{N_SPOTS.get('1', 40)} spots", hover=True)
    p.objective("Find 20x Flour.", "here", "#e9745e")
    p.owner_row("3", "#7ed957", "Sweet Lubrication", right=f"{N_SPOTS.get('3', 21)} spots")
    p.objective("Find 3x Tavern Wine.", "here", "#7ed957")
    p.objective("Find 3x Thin Mead.", "here", "#7ed957")
    p.owner_row("4", "#f2a65a", "Mapping the Front", right="2 spots", new=True)
    p.objective("Locate the Gunline Fort.", "here", "#f2a65a")
    p.objective("Southwest Defense Map", "done")
    p.note([("Other map:  ", T500), ("badge", "2", "#5ec5e9"), ("Still Alive", T300)])
    p.subheader("Available on this map", 22, right="Quests layer")
    p.link_row("Show 22 quests", indent=0)
    p.section("CRAFTING", 1, collapsed=True, right="Grubber's Purse")
    p.section("SEARCH", 1)
    p.owner_row("B", SEARCH[1], "Iron Ore", right=f"{len(data['iron'])} spots", removable=True)
    p.section("MAP LAYERS", None, right="all 11 on")
    layers_block(p, {"extractions", "player_spawns", "minibosses", "traps", "sound_traps", "placed_loot", "harvest", "keys", "locks", "quests", "places"})
    prect = p.finish(min_h=770)
    tooltip(cv, hx, hy, 12.5, it["color"], "1 · Making Dough · container", it["container"] or "Container",
            [(it["obj"], "")], chance=it["chance"], avoid=prect)
    out = composite(base, cv)
    out.save(os.path.join(HERE, "mockup_zoomed.png"))


def scene_states():
    """Panel states side by side on the game's dark background (crop of the default screenshot)."""
    base = Image.open(os.path.join(HERE, "base_default.png")).convert("RGB")
    bg = base.crop((0, 0, 1500, 640))
    # darken the crop so the panels read as on the game's dark backdrop
    bg = Image.blend(bg, Image.new("RGB", bg.size, (14, 12, 10)), 0.35)
    W, H = bg.size
    cv = Canvas(W, H)
    cap = font(F_UI_SB, 13)

    # (a) collapsed
    x = 30
    cv.text(x, 26, "A · COLLAPSED (title bar only, 360 px)", cap, T300, tracking=0.8)
    p = Panel(cv, x, 44, w=360)
    p.header(collapsed=True, title="COMPANION", summary="3 quests · 2 layers")
    p.finish()

    # (b) empty state
    x = 30
    cv.text(x, 130, "B · NOTHING TRACKED", cap, T300, tracking=0.8)
    p = Panel(cv, x, 148)
    p.header()
    p.search_field()
    p.quick_row()
    p.section("QUESTS", 0)
    p.empty([("No quest tracked yet.", T200, F_UI_SB),
             ("Track quests on hunger-companion.com, or turn on", T500, F_UI),
             ("the Quests layer to discover them on this map.", T500, F_UI)])
    p.button("Show quests on this map", primary=True)
    p.finish()

    # (c) API key problem
    x = 500
    cv.text(x, 26, "C · KEY / NETWORK PROBLEM", cap, T300, tracking=0.8)
    p = Panel(cv, x, 44)
    p.header(status=WARN)
    s = 1.0

    def warn_op(ox, oy, y=p.y):
        l, t, r, b = ox + PAD, oy + y + 10, ox + PW - PAD, oy + y + 62
        cv.rrect(l, t, r, b, 6, WARN, 0.10, stroke=WARN, sa=0.38, sw=1)
        cv.text(l + 12, t + 17, ICON["warn"], font(F_ICON, 13), WARN, anchor="lm")
        cv.text(l + 34, t + 17, "No API key set.", font(F_UI_SB, 13.5), "#F3C38A")
        cv.text(l + 34, t + 37, "Paste the key from hunger-companion.com.", font(F_UI, 13), "#E3B98A")
    p.add(warn_op)
    p.y += 62
    p.button("Set up…", primary=True)
    p.finish()

    # (d) overflow
    x = 980
    cv.text(x, 26, "D · OVERFLOW (content > max height)", cap, T300, tracking=0.8)
    p = Panel(cv, x, 44, w=PW)
    p.header()
    p.section("QUESTS", 7)
    p.owner_row("1", "#e9745e", "Making Dough", right=f"{N_SPOTS.get('1', 40)} spots")
    p.objective("Find 20x Flour.", "here", "#e9745e")
    p.owner_row("3", "#7ed957", "Sweet Lubrication", right=f"{N_SPOTS.get('3', 21)} spots")
    p.objective("Find 3x Tavern Wine.", "here", "#7ed957")
    p.objective("+1 more objective", "elsewhere")
    p.owner_row("4", "#f2a65a", "Mapping the Front", right="2 spots")
    p.owner_row("5", "#4fb3a9", "Clean Water", right="31 spots")
    p.overflow_footer("4 more sections · scroll")
    p.finish()
    out = composite(bg, cv)
    out.save(os.path.join(HERE, "panel_collapsed.png"))


def scene_sheet():
    """Marker catalogue at 2x on real parchment: normal + hover."""
    base = clean_zoomed_base()
    par = base.crop((420, 640, 1020, 1380))  # a calm parchment area (clear of the game's legend box)
    par = par.resize((par.width * 2, par.height * 2), Image.LANCZOS)
    par = Image.blend(par, Image.new("RGB", par.size, (232, 226, 210)), 0.25)
    W, H = par.size
    cv = Canvas(W, H)
    s = 2.0
    rows = [
        ("Tracked objective (quest 1)", lambda x, y, h: m_tracked_disc(cv, x, y, "#e9745e", "1", s, h)),
        ("Tracked, optional objective", lambda x, y, h: m_tracked_disc(cv, x, y, "#7ed957", "3", s, h, optional=True)),
        ("Cluster (5 containers, quest 1)", lambda x, y, h: (m_cluster(cv, x, y, "#e9745e", "1", 5, s))),
        ("Container / crafting ingredient (A)", lambda x, y, h: m_tracked_disc(cv, x, y, "#c77dff", "A", s, h)),
        ("Searched item, harvest node (B)", lambda x, y, h: m_tracked_square(cv, x, y, "#F5D547", "B", s, h)),
        ("Searched item, node above level", lambda x, y, h: m_tracked_square(cv, x, y, "#F5D547", "B", s, h, locked=True)),
        ("Tracked item, placed spot", lambda x, y, h: m_tracked_diamond(cv, x, y, "#5ec5e9", "2", s, h)),
        ("Done objective (Show done on)", lambda x, y, h: m_done(cv, x, y, s, h)),
        ("Quest available (Quests layer)", lambda x, y, h: m_quest_avail(cv, x, y, s, h)),
        ("Place / POI", lambda x, y, h: m_place(cv, x, y, s, h)),
        ("Extraction", lambda x, y, h: m_extract(cv, x, y, s, h)),
        ("Player spawn", lambda x, y, h: m_spawn(cv, x, y, s, h)),
        ("Miniboss", lambda x, y, h: m_boss(cv, x, y, s, h)),
        ("Trap", lambda x, y, h: m_trap(cv, x, y, s, h)),
        ("Sound trap (close / dust)", lambda x, y, h: (m_sound(cv, x, y, s, h) if h is not None else None)),
        ("Key spot", lambda x, y, h: m_key(cv, x, y, s, h)),
        ("Locked door / container", lambda x, y, h: m_lock(cv, x, y, s, h)),
        ("Placed loot (layer)", lambda x, y, h: m_loot(cv, x, y, s, h)),
        ("Harvest node (layer) / above level", lambda x, y, h: m_harvest(cv, x, y, "Naturalist", s, h)),
    ]
    rh = (H - 140) / len(rows)
    cv.rrect(20, 20, W - 20, 84, 10, INK_950, 0.9)
    cv.text(40, 52, "MARKERS", font(F_DISPLAY, 26), GOLD, tracking=3)
    cv.text(110, 102, "normal", font(F_UI_SB, 22), "#2A2118", anchor="mm")
    cv.text(230, 102, "hover", font(F_UI_SB, 22), "#2A2118", anchor="mm")
    for i, (name, fn) in enumerate(rows):
        y = 130 + i * rh + rh / 2
        fn(110, y, False)
        if i == 14:
            m_sound(cv, 230, y, s, True)
            m_sound(cv, 330, y, s, dust=True)
        elif i == 18:
            m_harvest(cv, 230, y, "Naturalist", s, True)
            m_harvest(cv, 330, y, "Conservator", s, locked=True)
        elif i == 2:
            pass
        else:
            fn(230, y, True)
        lw = cv.measure(name, font(F_UI_SB, 26)) + 24
        cv.rrect(400, y - 22, 400 + lw, y + 22, 8, "#15110C", 0.85)
        cv.text(412, y + 1, name, font(F_UI_SB, 26), T100)
    out = composite(par, cv)
    out.save(os.path.join(HERE, "markers_sheet.png"))


if __name__ == "__main__":
    pack = load_pack()
    data = gather(pack)
    for t in data["tracked"]:
        N_SPOTS[t["badge"]] = N_SPOTS.get(t["badge"], 0) + 1
    scene_default(pack, data)
    scene_zoomed(pack, data)
    scene_states()
    scene_sheet()
    print("written:", ", ".join(n for n in os.listdir(HERE) if n.endswith(".png") and not n.startswith("base_")))
