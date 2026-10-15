"""Hunger Overlay - quest journal + allies mockups (see ../DESIGN_QUESTS.md).

Renders, with the same Direct2D-like primitives and tokens as mockup.py (imported):

    journal.png        the Quest journal window over the game: subject tabs (you / 2 allies / + Ally), UP NEXT
                       (first available quest of each character), character cards with the progress stepper,
                       an open "Track here" confirm strip with its cascade preview, the history footer; plus an
                       inset of the Search window's quest results with their states
    allies_map.png     default map view with the player's 3 quests (filled) and 2 allies (hollow + initial):
                       shared-spot pips, petals, a tooltip on an ally spot; panel with UP NEXT and ALLIES
    panel_allies.png   panel states: "Done" toast with the next quest, ally section variants, the empty
                       ally state, the collapsed summary
    ally_markers.png   marker grammar at 2x: mine vs ally for each shape, clusters, pips, petals, spotlight

Data is REAL: quest titles / givers / kinds from HungerDB/export/quests/quests.json, the chain order from
quest_chains.json, prerequisites from quest_preconditions.json, marker positions from the overlay's cached
pack (%APPDATA%\\HungerOverlay\\pack.json). The player / ally states are invented but CONSISTENT with the
model of DESIGN_QUESTS.md (computed by the small quest graph below, not hand-written).

Run:  python docs/design/mockup_quests.py
"""

import json
import math
import os
import sys

from PIL import Image, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import mockup as M  # noqa: E402  (tokens, Canvas, markers, Panel, tooltip, composite, projection)
from mockup import (ACCENT, F_DISPLAY, F_ICON, F_UI, F_UI_B, F_UI_SB, GOLD, GOLD_HI, HALO, ICON, INK_700,  # noqa: E402
                    INK_800, INK_950, LINE, OK, PAD, PW, T100, T200, T300, T500, WARN, Canvas, Panel, badge_text,
                    composite, font)

HUNGERDB = r"D:\Users\Camille\Downloads\HungerDB\export\quests"
ICON.update({"journal": "\uE82D", "people": "\uE716", "addfriend": "\uE8FA", "history": "\uE81C", "lock": "\uE72E",
             "repeat": "\uE8EE", "reopen": "\uE72C", "cloud": "\uE753", "pc": "\uE7F8", "filter": "\uE71C",
             "min": "\uE921", "max": "\uE922", "xclose": "\uE8BB", "done": "\uE930"})

# Ally colours (fixed, never the website's): A1 mint, A2 orchid. Always drawn HOLLOW (dark disc + coloured ring).
ALLY1 = "#5FE0B5"
ALLY2 = "#E58CF2"
TOKEN = "#16120D"
IVORY = "#F3ECDD"


# ----------------------------------------------------------------------------------------------
# Quest graph (the model of DESIGN_QUESTS.md §2)
# ----------------------------------------------------------------------------------------------
class Graph:
    def __init__(self):
        rd = lambda n: json.load(open(os.path.join(HUNGERDB, n), encoding="utf-8"))
        self.q = {x["quest_id"]: x for x in rd("quests.json")}
        pre = rd("quest_preconditions.json")
        self.req = {k: list(v.get("requires") or []) for k, v in pre.items()}
        self.lines = rd("quest_chains.json")
        # giver display names (pack order); Hector is not in the public pack
        self.giver_name = {"Eloise": "Eloise", "Louis": "Louis", "MrSweet": "Mr Sweet", "Piro": "Piro", "Reynauld": "Reynauld"}
        self.givers = list(self.giver_name)

    def title(self, qid):
        return self.q[qid]["title"]

    def kind(self, qid):
        return self.q[qid]["kind"]  # standard / daily / weekly / monthly

    def npc(self, qid):
        return self.q[qid]["npc"]

    def ancestors(self, qid):
        out, todo = [], list(self.req.get(qid, []))
        while todo:
            a = todo.pop()
            if a not in out:
                out.append(a)
                todo += self.req.get(a, [])
        return out

    def descendants(self, qid):
        return [d for d in self.q if qid in self.ancestors(d)]

    def story(self, npc):
        """Story quests of a character in play order: its lines, then standalone story quests (bridges)."""
        out = []
        for ln in self.lines:
            if ln["questline_id"].split(".")[1] == npc:
                out += [x for x in ln["order"] if x in self.q]
        out += [k for k, v in self.q.items() if v["npc"] == npc and v["kind"] == "standard" and k not in out]
        return out

    def line_of(self, qid):
        for ln in self.lines:
            if qid in ln["order"]:
                return ln
        return None

    def state(self, qid, done, tracked):
        if qid in done:
            return "done"
        if qid in tracked:
            return "tracked"
        if all(r in done for r in self.req.get(qid, [])):
            return "available"
        return "locked"

    def up_next(self, npc, done, tracked):
        """First not-done story quest of the character: (quest id, state)."""
        for x in self.story(npc):
            if x not in done:
                return x, self.state(x, done, tracked)
        return None, "complete"

    def missing(self, qid, done):
        """Ancestors not done yet, in play order."""
        anc = [a for a in self.ancestors(qid) if a not in done]
        order = {x: i for i, x in enumerate(self.flat())}
        return sorted(anc, key=lambda a: order.get(a, 999))

    def flat(self):
        out = []
        for n in self.givers:
            out += self.story(n)
        return out


G = Graph()

# The player ("Kamille") - consistent state: every ancestor of a done / tracked quest is done.
ME_DONE = {"Quest.Eloise.Q01", "Quest.Eloise.Q04", "Quest.Eloise.Q05", "Quest.Louis.Q01", "Quest.MrSweet.Q01", "Quest.Reynauld.Q01"}
ME_TRACKED = [("Quest.Eloise.Q02", "1", "#e9745e"), ("Quest.Reynauld.Q02", "2", "#5ec5e9"), ("Quest.MrSweet.Q02", "3", "#7ed957")]
ME_T = {q for q, _, _ in ME_TRACKED}
BADGE = {q: (b, c) for q, b, c in ME_TRACKED}
for q in list(ME_DONE) + list(ME_T):
    assert all(a in ME_DONE for a in G.ancestors(q)), q

# Allies: Marius = website friend, Lou = manual (no app, no account).
ALLIES = [
    dict(name="Marius", letter="M", color=ALLY1, source="site", quests=["Quest.Eloise.Q02", "Quest.Louis.Q02"]),
    dict(name="Lou", letter="L", color=ALLY2, source="local", quests=["Quest.Eloise.DQ03", "Quest.Eloise.Q06"]),
]


def clean_label(s):
    """'Find 3x Thin Mead. (Hydromel léger)' -> 'Find 3x Thin Mead.' (the cached pack is French)."""
    i = s.find(" (")
    return s[:i] if i > 0 and s.endswith(")") else s


# ----------------------------------------------------------------------------------------------
# Ally markers (DESIGN_QUESTS.md §5): hollow = someone else's; white hairline = tracked by someone in the raid
# ----------------------------------------------------------------------------------------------
def m_ally(cv, x, y, color, letter, s=1.0, h=False, shape="disc"):
    k = s * (1.25 if h else 1)
    if shape == "square":
        r = 8 * k
        cv.rrect(x - r - 2.5 * s, y - r - 2.5 * s, x + r + 2.5 * s, y + r + 2.5 * s, 4 * s, HALO, 0.6)
        cv.rrect(x - r, y - r, x + r, y + r, 3 * k, TOKEN, 0.94)
        cv.rrect(x - r + 1.3 * k, y - r + 1.3 * k, x + r - 1.3 * k, y + r - 1.3 * k, 2 * k, None, stroke=color, sa=1, sw=2.6 * k)
        cv.rrect(x - r - 0.4 * k, y - r - 0.4 * k, x + r + 0.4 * k, y + r + 0.4 * k, 3.4 * k, None, stroke="#FFFFFF", sa=0.85, sw=1.1 * k)
    elif shape == "diamond":
        r = 10.5 * k
        cv.diamond(x, y, r + 3.5 * s, HALO, 0.6)
        cv.diamond(x, y, r, TOKEN, 0.94)
        cv.diamond_stroke(x, y, r - 2.0 * k, 2.6 * k, color)
        cv.diamond_stroke(x, y, r + 0.3 * k, 1.1 * k, "#FFFFFF", 0.85)
    else:
        r = 9 * k
        cv.circle(x, y, r + 2.6 * s, HALO, 0.6)
        cv.circle(x, y, r, TOKEN, 0.94)
        cv.ring(x, y, r - 1.1 * k, 2.2 * k, color)
        cv.ring(x, y, r + 0.3 * k, 1.1 * k, "#FFFFFF", 0.85)
    cv.text(x, y + 0.5 * s, letter, font(F_UI_B, 10.5 * k), color, anchor="mm")
    if h:
        M.focus_ring(cv, x, y, 9 * k + 1 * s)


def ally_pip(cv, x, y, r_owner, color, letter, s=1.0, slot=0):
    """Ally on the SAME spot and the SAME objective as one of my markers: a small satellite, bottom-right
    (2nd ally bottom-left), instead of a second marker."""
    a = math.radians(45 if slot == 0 else 135)
    d = r_owner + 1.5 * s
    px, py = x + d * math.cos(a), y + d * math.sin(a)
    cv.circle(px, py, 7.6 * s, HALO, 0.7)
    cv.circle(px, py, 6.2 * s, TOKEN, 0.96)
    cv.ring(px, py, 5.4 * s, 1.6 * s, color)
    cv.text(px, py + 0.4 * s, letter, font(F_UI_B, 7.5 * s), color, anchor="mm")


def ally_cluster(cv, x, y, color, letter, n, s=1.0):
    m_ally(cv, x, y, color, letter, s * 1.15)
    M.count_pill(cv, x, y, n, color, s)


# ----------------------------------------------------------------------------------------------
# Map data
# ----------------------------------------------------------------------------------------------
def owner_points(pack, qid, min_chance=0.10, mp="Map01"):
    q = pack["quests"][qid]
    out, seen = [], set()
    for o in q["objectives"]:
        for m in o["markers"]:
            if m["map"] != mp or (m.get("chance") is not None and m["chance"] < min_chance):
                continue
            key = (round(m["px"]), round(m["py"]))
            if key in seen:
                continue
            seen.add(key)
            out.append(dict(px=m["px"], py=m["py"], obj=clean_label(o["label"]), container=m.get("container"),
                            chance=m.get("chance"), qid=qid, source=m.get("source")))
    return out


# ----------------------------------------------------------------------------------------------
# Panel additions
# ----------------------------------------------------------------------------------------------
class Panel2(Panel):
    def section2(self, title, count=None, link=None, icon=None, right=None):
        """Section header with a gold link on the right ('Journal', 'Manage')."""
        s, w = self.s, self.w
        y = self.y + 12 * s

        def op(ox, oy):
            cv = self.cv
            cv.line(ox + PAD * s, oy + y, ox + w - PAD * s, oy + y, 1 * s, LINE, 0.16)
            cy = oy + y + 19 * s
            cv.text(ox + PAD * s, cy + 0.5 * s, ICON["down"], font(F_ICON, 9 * s), T500, anchor="lm")
            tx = ox + PAD * s + 16 * s
            tw = cv.text(tx, cy + 1 * s, title, font(F_DISPLAY, 12.5 * s), GOLD, tracking=1.8 * s)
            if count is not None:
                t = str(count)
                f = font(F_UI_SB, 11 * s)
                cw = cv.measure(t, f) + 12 * s
                px = tx + tw + 9 * s
                cv.rrect(px, cy - 8 * s, px + cw, cy + 8 * s, 8 * s, INK_700, 1.0)
                cv.text(px + cw / 2, cy + 0.5 * s, t, f, T200, anchor="mm")
            rx = ox + w - PAD * s
            if link:
                f = font(F_UI_SB, 12.5 * s)
                lw = cv.measure(link, f)
                cv.text(rx, cy, link, f, GOLD, anchor="rm")
                if icon:
                    cv.text(rx - lw - 6 * s, cy + 0.5 * s, ICON[icon], font(F_ICON, 12 * s), GOLD, anchor="rm")
            elif right:
                cv.text(rx, cy, right, font(F_UI, 12.5 * s), T500, anchor="rm")
        self.add(op)
        self.y = y + 34 * s

    def owner_done(self, badge, color, title, right, hover=False, check_hover=False, shared=None):
        """Tracked quest row with the new ✓ 'Mark done' button (22×22, ghost) at the far right."""
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            t, b = oy + y, oy + y + 28 * s
            cy = (t + b) / 2
            rx = ox + w - PAD * s
            if hover:
                cv.rrect(ox + PAD * s - 6 * s, t, rx - 26 * s, b, 5 * s, "#FFFFFF", 0.07)
            bx = ox + PAD * s + 10 * s
            cv.circle(bx, cy, 10 * s, HALO, 0.5)
            cv.circle(bx, cy, 9.5 * s, color)
            cv.ring(bx, cy, 9 * s, 1.4 * s, "#FFFFFF", 0.9)
            badge_text(cv, bx, cy, badge, color, 11.5 * s)
            if shared:
                for i, (c, l) in enumerate(shared):
                    ally_pip(cv, bx, cy, 9.5 * s, c, l, s * 0.85, slot=i)
            tx = ox + PAD * s + 28 * s
            fn = font(F_UI_SB, 15 * s)
            cv.text(tx, cy, cv.ellipsize(title, fn, w - 2 * PAD * s - 150 * s), fn, T100)
            # ✓ button
            cbx = rx - 22 * s
            cv.rrect(cbx, cy - 11 * s, rx, cy + 11 * s, 5 * s, INK_700 if check_hover else None, 1.0,
                     stroke=GOLD if check_hover else "#FFFFFF", sa=0.6 if check_hover else 0.10, sw=1 * s)
            cv.text(cbx + 11 * s, cy + 0.5 * s, ICON["check"], font(F_ICON, 11 * s), GOLD_HI if check_hover else T300, anchor="mm")
            cv.text(cbx - 8 * s, cy, right, font(F_UI, 12.5 * s), T500, anchor="rm")
        self.add(op)
        self.y += 30 * s

    def next_row(self, title, giver, here=0, hover=False, track_hover=False, step=None):
        """UP NEXT row: '!' token when the quest has spots on this map, a hollow ring otherwise."""
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            t, b = oy + y, oy + y + 28 * s
            cy = (t + b) / 2
            r = ox + w - PAD * s
            f2 = font(F_UI_SB, 12 * s)
            lab = "Track"
            bw = cv.measure(lab, f2) + 30 * s
            if hover:
                cv.rrect(ox + PAD * s - 6 * s, t, r - bw - 4 * s, b, 5 * s, "#FFFFFF", 0.07)
            gx = ox + PAD * s + 10 * s
            if here:
                M.m_quest_avail(cv, gx, cy, s * 0.82)
            else:
                cv.ring(gx, cy, 5 * s, 1.4 * s, M.C_QUEST_AVAIL, 0.8)
            tx = ox + PAD * s + 28 * s
            f = font(F_UI, 14 * s)
            tw = cv.text(tx, cy, title, f, T200)
            gtxt = giver + (f" · {step}" if step else "")
            cv.text(tx + tw + 8 * s, cy, gtxt, font(F_UI, 12.5 * s), T500)
            cv.rrect(r - bw, cy - 11 * s, r, cy + 11 * s, 5 * s, GOLD if track_hover else INK_800, 0.22 if track_hover else 0.9, stroke=GOLD, sa=0.55, sw=1 * s)
            cv.text(r - bw + 10 * s, cy + 0.5 * s, ICON["add"], font(F_ICON, 9 * s), GOLD_HI, anchor="lm")
            cv.text(r - bw + 23 * s, cy, lab, f2, GOLD_HI)
        self.add(op)
        self.y += 30 * s

    def ally_row(self, a, right, hover=False, hidden=False):
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            t, b = oy + y, oy + y + 28 * s
            cy = (t + b) / 2
            if hover:
                cv.rrect(ox + PAD * s - 6 * s, t, ox + w - PAD * s + 6 * s, b, 5 * s, "#FFFFFF", 0.07)
            cv.fade = 0.4 if hidden else 1.0
            m_ally(cv, ox + PAD * s + 10 * s, cy, a["color"], a["letter"], s * 1.02)
            cv.fade = 1.0
            tx = ox + PAD * s + 28 * s
            tw = cv.text(tx, cy, a["name"], font(F_UI_SB, 15 * s), T500 if hidden else T100)
            ic = ICON["cloud"] if a["source"] == "site" else ICON["pc"]
            tag = "SITE" if a["source"] == "site" else "LOCAL"
            ft = font(F_UI_SB, 10 * s)
            x = tx + tw + 9 * s
            cv.text(x, cy + 0.5 * s, ic, font(F_ICON, 11 * s), T500, anchor="lm")
            cv.text(x + 16 * s, cy + 0.5 * s, tag, ft, T500, tracking=1.0 * s)
            rx = ox + w - PAD * s
            if hidden:
                cv.text(rx, cy + 0.5 * s, ICON["hide"], font(F_ICON, 13 * s), T500, anchor="rm")
            else:
                cv.text(rx, cy, right, font(F_UI, 12.5 * s), T500, anchor="rm")
        self.add(op)
        self.y += 30 * s

    def ally_quest(self, a, title, right, mine=None, removable=False, hover=False):
        s, w = self.s, self.w
        y = self.y

        def op(ox, oy):
            cv = self.cv
            cy = oy + y + 12 * s
            if hover:
                cv.rrect(ox + PAD * s + 20 * s, cy - 12 * s, ox + w - PAD * s + 6 * s, cy + 12 * s, 5 * s, "#FFFFFF", 0.07)
            gx = ox + PAD * s + 34 * s
            cv.circle(gx, cy, 5.2 * s, HALO, 0.5)
            cv.ring(gx, cy, 4.2 * s, 1.8 * s, a["color"])
            tx = gx + 12 * s
            tw = cv.text(tx, cy, title, font(F_UI, 14 * s), T200)
            if mine:
                b, c = mine
                x = tx + tw + 8 * s
                x += cv.text(x, cy, "also", font(F_UI, 12 * s), T500) + 5 * s
                cv.circle(x + 7 * s, cy, 7 * s, c, 0.75)
                badge_text(cv, x + 7 * s, cy, b, c, 9.5 * s)
            rx = ox + w - PAD * s
            if removable:
                cv.rrect(rx - 22 * s, cy - 10 * s, rx, cy + 10 * s, 5 * s, None, stroke="#FFFFFF", sa=0.08, sw=1 * s)
                cv.text(rx - 11 * s, cy + 0.5 * s, ICON["close"], font(F_ICON, 8.5 * s), T300, anchor="mm")
                rx -= 30 * s
            cv.text(rx, cy, right, font(F_UI, 12.5 * s), T500, anchor="rm")
        self.add(op)
        self.y += 24 * s

    def toast_next(self, done_title, next_title, extra=None):
        """Two-line toast after ✓ Done: what changed + the next quest of that character, with Track and Undo."""
        s, w = self.s, self.w
        y = self.y + 10 * s

        def op(ox, oy):
            cv = self.cv
            l, t, r, b = ox + PAD * s, oy + y, ox + w - PAD * s, oy + y + 56 * s
            cv.rrect(l, t, r, b, 6 * s, OK, 0.13, stroke=OK, sa=0.45, sw=1 * s)
            cy1, cy2 = t + 17 * s, t + 40 * s
            cv.text(l + 11 * s, cy1 + 0.5 * s, ICON["check"], font(F_ICON, 12 * s), OK, anchor="lm")
            f = font(F_UI, 13.5 * s)
            x = l + 31 * s
            x += cv.text(x, cy1, done_title, font(F_UI_SB, 13.5 * s), T100)
            cv.text(x + 5 * s, cy1, "done" + (f" {extra}" if extra else ""), f, T100)
            aw = cv.measure("Undo", font(F_UI_SB, 13 * s))
            cv.text(r - 12 * s - aw, cy1, "Undo", font(F_UI_SB, 13 * s), GOLD_HI)
            cv.line(r - 12 * s - aw, cy1 + 9 * s, r - 12 * s, cy1 + 9 * s, 1 * s, GOLD_HI, 0.6)
            x = l + 31 * s
            x += cv.text(x, cy2, "Next:", f, T300) + 6 * s
            cv.text(x, cy2, next_title, f, T100)
            f2 = font(F_UI_SB, 12 * s)
            bw = cv.measure("Track", f2) + 30 * s
            cv.rrect(r - 8 * s - bw, cy2 - 10 * s, r - 8 * s, cy2 + 10 * s, 5 * s, INK_800, 0.9, stroke=GOLD, sa=0.55, sw=1 * s)
            cv.text(r - 8 * s - bw + 10 * s, cy2 + 0.5 * s, ICON["add"], font(F_ICON, 9 * s), GOLD_HI, anchor="lm")
            cv.text(r - 8 * s - bw + 23 * s, cy2, "Track", f2, GOLD_HI)
        self.add(op)
        self.y = y + 56 * s

    def dim_line(self, parts, indent=4):
        self.note(parts, indent=indent)


# ----------------------------------------------------------------------------------------------
# Scene 1: allies on the map + panel
# ----------------------------------------------------------------------------------------------
def map_owners(pack):
    """Every owner group on Map01: (key, kind, color, badge/letter, points)."""
    groups = []
    for qid, b, c in ME_TRACKED:
        groups.append(dict(key=("me", qid), ally=None, color=c, badge=b, qid=qid, pts=owner_points(pack, qid)))
    for a in ALLIES:
        for qid in a["quests"]:
            groups.append(dict(key=(a["name"], qid), ally=a, color=a["color"], badge=a["letter"], qid=qid, pts=owner_points(pack, qid)))
    return groups


def scene_map(pack):
    base = Image.open(os.path.join(HERE, "base_default.png")).convert("RGB")
    W, H = base.size
    cv = Canvas(W, H)
    proj, clip = M.PROJ_DEFAULT, M.MAP_DEFAULT
    # extractions layer (ON by default)
    for l in pack["layers"]:
        if l["id"] == "extractions":
            for m in l["markers"]:
                if m["map"] == "Map01":
                    x, y = proj(m["px"], m["py"])
                    if M.inside(x, y, clip, -6):
                        M.m_extract(cv, x, y)
    groups = map_owners(pack)
    my_spots = {}
    for g in groups:
        if g["ally"] is None:
            for p in g["pts"]:
                my_spots[(round(p["px"]), round(p["py"]), g["qid"])] = (g["badge"], g["color"])
    # 1) pips: an ally point on the same spot AND the same quest as mine is not drawn as a marker
    pips = {}
    clusters = []
    for g in groups:
        pts = []
        for p in g["pts"]:
            x, y = proj(p["px"], p["py"])
            if not M.inside(x, y, clip):
                continue
            k = (round(p["px"]), round(p["py"]), g["qid"])
            if g["ally"] is not None and k in my_spots:
                pips.setdefault(k, []).append(g["ally"])
                continue
            pts.append((x, y, p))
        rad = 24
        for x, y, items in M.cluster(pts, rad):
            clusters.append((x, y, g, items))
    # merge per ally (one owner per ally on the map, not per ally quest), then petals between different owners
    by_owner = {}
    for x, y, g, items in clusters:
        key = g["ally"]["name"] if g["ally"] else g["badge"]
        by_owner.setdefault(key, []).append((x, y, g, items))
    final = []
    for key, lst in by_owner.items():
        if lst[0][2]["ally"] is not None:
            pts = [(x, y, (g, it)) for x, y, g, items in lst for it in items]
            for x, y, its in M.cluster(pts, 24):
                final.append((x, y, its[0][0], [i for _, i in its]))
        else:
            final += lst
    groups_sp = [(x, y, (0 if g["ally"] is None else 1, g["badge"]), g, items) for x, y, g, items in final]
    placed = spread(cv, groups_sp, 22)
    # draw: allies first (below), then mine
    lou_target = None
    for x, y, _, g, items in sorted(placed, key=lambda t: 0 if t[3]["ally"] else 1):
        if g["ally"]:
            a = g["ally"]
            if a["name"] == "Lou" and any(i["obj"].startswith("Search Sarlat") for i in items):
                lou_target = (x, y, items)
                continue
            if len(items) > 1:
                ally_cluster(cv, x, y, a["color"], a["letter"], len(items))
            else:
                m_ally(cv, x, y, a["color"], a["letter"])
        else:
            if len(items) > 1:
                M.m_cluster(cv, x, y, g["color"], g["badge"], len(items))
            else:
                M.m_tracked_disc(cv, x, y, g["color"], g["badge"])
            # pips for allies sharing this spot + objective
            n_shared = []
            for it in items:
                k = (round(it["px"]), round(it["py"]), g["qid"])
                for a in pips.get(k, []):
                    if a["name"] not in [b["name"] for b in n_shared]:
                        n_shared.append(a)
            for i, a in enumerate(n_shared[:2]):
                ally_pip(cv, x, y, 10 * (1.18 if len(items) > 1 else 1), a["color"], a["letter"], 1.0, slot=i)
    # hovered ally marker (Lou - Signs Of Life - Search Sarlat) + tooltip
    hx, hy, items = lou_target
    m_ally(cv, hx, hy, ALLY2, "L", h=True)

    # Panel
    p = Panel2(cv, 1987, 60)
    p.header()
    p.search_field()
    p.quick_row(show_done=False, chance=10, lang="EN")
    n = {g["key"]: len([q for q in g["pts"] if M.inside(*proj(q["px"], q["py"]), clip)]) for g in groups}
    p.section2("QUESTS", 3, link="Journal", icon="journal")
    shared_last_embers = [(ALLY1, "M")]
    p.owner_done("1", "#e9745e", G.title("Quest.Eloise.Q02"), f"{n[('me', 'Quest.Eloise.Q02')]} spots", shared=shared_last_embers)
    p.objective(clean_label(pack["quests"]["Quest.Eloise.Q02"]["objectives"][0]["label"]), "here", "#e9745e")
    p.owner_done("2", "#5ec5e9", G.title("Quest.Reynauld.Q02"), f"{n[('me', 'Quest.Reynauld.Q02')]} spots")
    p.warning("Only below your 10 % filter here", "Show ≥ 5 %")  # best Ammo Box chance on Map01 = 6.7 %
    p.objective(clean_label(pack["quests"]["Quest.Reynauld.Q02"]["objectives"][0]["label"]), "elsewhere")
    p.owner_done("3", "#7ed957", G.title("Quest.MrSweet.Q02"), f"{n[('me', 'Quest.MrSweet.Q02')]} spots", hover=True, check_hover=True)
    p.objective(clean_label(pack["quests"]["Quest.MrSweet.Q02"]["objectives"][0]["label"]), "here", "#7ed957")
    p.subheader("Up next", 2, right="first per character")
    p.next_row(G.title("Quest.Louis.Q02"), "Louis", here=0, step="2/10")
    p.next_row(G.title("Quest.Piro.Q01"), "Piro", here=0, step="1/9")
    p.link_row("13 repeatable · Journal", indent=28)
    # ALLIES
    p.section2("ALLIES", 2, link="Manage", icon="people")
    m_tot = n[("Marius", "Quest.Eloise.Q02")] + n[("Marius", "Quest.Louis.Q02")]
    p.ally_row(ALLIES[0], f"{m_tot} spots")
    p.ally_quest(ALLIES[0], G.title("Quest.Eloise.Q02"), f"{n[('Marius', 'Quest.Eloise.Q02')]} spots", mine=("1", "#e9745e"))
    p.ally_quest(ALLIES[0], G.title("Quest.Louis.Q02"), f"{n[('Marius', 'Quest.Louis.Q02')]} spots")
    l_tot = n[("Lou", "Quest.Eloise.DQ03")] + n[("Lou", "Quest.Eloise.Q06")]
    p.ally_row(ALLIES[1], f"{l_tot} spots", hover=True)
    p.ally_quest(ALLIES[1], G.title("Quest.Eloise.DQ03"), f"{n[('Lou', 'Quest.Eloise.DQ03')]} spot", removable=True, hover=True)
    p.ally_quest(ALLIES[1], G.title("Quest.Eloise.Q06"), f"{n[('Lou', 'Quest.Eloise.Q06')]} spots", removable=True)
    p.y += 4
    p.section("MAP LAYERS", None, right="1 on")
    M.layers_block(p, {"extractions"})
    prect = p.finish()

    M.tooltip(cv, hx, hy, 11, ALLY2, "L · Lou · ally · objective", "Search Sarlat.",
              [("Quest", G.title("Quest.Eloise.DQ03")), ("Giver", "Eloise"), ("Added by you", "local ally")], avoid=prect)
    out = composite(base, cv)
    out.save(os.path.join(HERE, "allies_map.png"))


def spread(cv, groups, mind):
    """mockup.spread with a stable order: my owners (by badge) first, then allies."""
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
        cv.circle(cx, cy, 2.0, IVORY)
        for i, g in enumerate(st):
            if len(st) == 2:
                a = math.pi if i == 0 else 0.0
            else:
                a = -math.pi / 2 + i * 2 * math.pi / len(st)
            nx, ny = cx + R * math.cos(a), cy + R * math.sin(a)
            cv.line(cx, cy, nx, ny, 2.6, HALO, 0.55)
            cv.line(cx, cy, nx, ny, 1.2, IVORY, 0.9)
            out.append((nx, ny) + tuple(g[2:]))
    return out


# ----------------------------------------------------------------------------------------------
# Scene 2: the Quest journal window
# ----------------------------------------------------------------------------------------------
JW = 800  # journal client width (px at 1440p)


class J:
    """Tiny immediate-mode layout helper for the journal (absolute coordinates)."""

    def __init__(self, cv, x, y, w):
        self.cv, self.x, self.y, self.w = cv, x, y, w
        self.pad = 20

    @property
    def l(self):
        return self.x + self.pad

    @property
    def r(self):
        return self.x + self.w - self.pad

    def hair(self, y, a=0.18):
        self.cv.line(self.l, y, self.r, y, 1, LINE, a)

    def caps(self, x, y, t, c=T500, size=11, tracking=1.2):
        return self.cv.text(x, y, t.upper(), font(F_UI_SB, size), c, tracking=tracking)

    def chip(self, x, cy, text, icon=None, style="normal", h=26, hover=False, size=13):
        cv = self.cv
        f = font(F_UI_SB if style != "normal" else F_UI, size)
        tw = cv.measure(text, f)
        iw = 18 if icon else 0
        w = tw + iw + 22
        if style == "primary":
            cv.rrect(x, cy - h / 2, x + w, cy + h / 2, 5, GOLD_HI if hover else ACCENT, 1.0)
            ink = "#17120C"
        elif style == "ghost":
            cv.rrect(x, cy - h / 2, x + w, cy + h / 2, 5, GOLD if hover else INK_800, 0.22 if hover else 0.9, stroke=GOLD, sa=0.55, sw=1)
            ink = GOLD_HI
        elif style == "warn":
            cv.rrect(x, cy - h / 2, x + w, cy + h / 2, 5, WARN, 0.16, stroke=WARN, sa=0.6, sw=1)
            ink = "#FFD7A8"
        else:
            cv.rrect(x, cy - h / 2, x + w, cy + h / 2, 5, INK_700 if hover else INK_800, 0.95, stroke=GOLD if hover else "#FFFFFF", sa=0.5 if hover else 0.08, sw=1)
            ink = T200
        if icon:
            cv.text(x + 11, cy + 0.5, ICON[icon], font(F_ICON, 10.5), ink, anchor="lm")
        cv.text(x + 11 + iw, cy, text, f, ink)
        return w


def quest_glyph(cv, x, y, qid, done, tracked, s=1.0):
    st = G.state(qid, done, tracked)
    if st == "done":
        cv.circle(x, y, 7 * s, GOLD, 0.9)
        cv.polyline([(x - 3 * s, y + 0.2 * s), (x - 0.9 * s, y + 2.4 * s), (x + 3.2 * s, y - 2.2 * s)], 1.7 * s, "#17120C")
    elif st == "tracked":
        b, c = BADGE[qid]
        cv.circle(x, y, 8.5 * s, c)
        cv.ring(x, y, 8 * s, 1.3 * s, "#FFFFFF", 0.9)
        badge_text(cv, x, y, b, c, 10.5 * s)
    elif st == "available":
        cv.ring(x, y, 6.2 * s, 1.7 * s, M.C_QUEST_AVAIL)
    else:
        cv.circle(x, y, 2.6 * s, T500, 0.7)
    return st


def stepper(cv, x, cy, quests, done, tracked, sel=None, gap=26):
    """Progress track of a character's story: done = gold disc + ✓, tracked = badge, available = gold ring,
    locked = small dim dot; the selected dot (open confirm strip) gets the focus ring."""
    xs = [x + i * gap for i in range(len(quests))]
    for i in range(len(quests) - 1):
        a = quests[i] in done
        cv.line(xs[i] + 7, cy, xs[i + 1] - 7, cy, 1.4 if a else 1.0, GOLD if a else T500, 0.6 if a else 0.35)
    for qx, q in zip(xs, quests):
        quest_glyph(cv, qx, cy, q, done, tracked)
        if q == sel:
            M.focus_ring(cv, qx, cy, 6)
    return xs


def journal_window(cv, wx, wy):
    """Draws the whole Quest journal window at (wx, wy); returns its rect."""
    cv_ = cv
    W = JW
    title_h = 40
    H = 1150
    # shadow + native dark title bar (DWM dark mode, like the Search window)
    cv_.rrect(wx + 6, wy + 10, wx + W + 6, wy + H + 10, 9, "#000000", 0.45)
    cv_.rrect(wx, wy, wx + W, wy + H, 8, INK_950, 1.0, stroke="#000000", sa=0.8, sw=1)
    cv_.rrect(wx, wy, wx + W, wy + title_h, 8, "#1C1915", 1.0)
    cv_.rrect(wx, wy + title_h - 8, wx + W, wy + title_h, 0, "#1C1915", 1.0)
    cv_.circle(wx + 22, wy + title_h / 2, 7, ACCENT)
    cv_.text(wx + 22, wy + title_h / 2 + 0.5, "H", font(F_UI_B, 9), "#17120C", anchor="mm")
    cv_.text(wx + 38, wy + title_h / 2, "Quest journal — Hunger Companion", font(F_UI, 13.5), "#E8E2D6")
    for i, ic in enumerate(["xclose", "max", "min"]):
        cv_.text(wx + W - 24 - i * 46, wy + title_h / 2, ICON[ic], font(F_ICON, 10), "#CFC8BA", anchor="mm")
    # inner gold frame (game's double line)
    cv_.rrect(wx + 3, wy + title_h + 3, wx + W - 3, wy + H - 3, 5, None, stroke=LINE, sa=0.32, sw=1)
    j = J(cv_, wx, wy + title_h, W)
    y = wy + title_h + 18

    # --- subject tabs ------------------------------------------------------------------------
    cy = y + 18
    x = j.l
    # "You" tab (selected)
    f = font(F_UI_SB, 14)
    tw = cv_.measure("Kamille", f)
    w = tw + 70
    cv_.rrect(x, cy - 18, x + w, cy + 18, 6, INK_700, 1.0, stroke=GOLD, sa=0.7, sw=1)
    cv_.circle(x + 18, cy, 9, GOLD)
    cv_.text(x + 18, cy + 0.5, "K", font(F_UI_B, 10.5), "#17120C", anchor="mm")
    cv_.text(x + 34, cy, "Kamille", f, T100)
    cv_.text(x + 38 + tw, cy, "you", font(F_UI, 12), T300)
    x += w + 8
    for a in ALLIES:
        tw = cv_.measure(a["name"], f)
        w = tw + 64
        cv_.rrect(x, cy - 18, x + w, cy + 18, 6, INK_800, 0.95, stroke="#FFFFFF", sa=0.08, sw=1)
        m_ally(cv_, x + 18, cy, a["color"], a["letter"], 0.95)
        cv_.text(x + 34, cy, a["name"], f, T200)
        cv_.text(x + 40 + tw, cy + 0.5, ICON["cloud"] if a["source"] == "site" else ICON["pc"], font(F_ICON, 11), T500, anchor="lm")
        x += w + 8
    j.chip(x, cy, "Ally", icon="addfriend", style="ghost", h=36, size=13.5)
    # sync status, right
    cv_.circle(j.r - 66, cy, 3.5, OK)
    cv_.text(j.r, cy, "synced", font(F_UI, 12.5), T500, anchor="rm")
    y = cy + 30

    # --- filter field + repeatables toggle ----------------------------------------------------
    cy = y + 17
    fr = j.r - 170
    cv_.rrect(j.l, cy - 17, fr, cy + 17, 6, "#0B0906", 0.75, stroke="#FFFFFF", sa=0.10, sw=1)
    cv_.text(j.l + 13, cy + 0.5, ICON["search"], font(F_ICON, 13), T500, anchor="lm")
    cv_.text(j.l + 34, cy, "Filter quests…", font(F_UI, 14), T500)
    x = fr + 10
    cv_.rrect(x, cy - 15, j.r, cy + 15, 6, INK_800, 0.95, stroke="#FFFFFF", sa=0.08, sw=1)
    cv_.rrect(x + 10, cy - 6.5, x + 23, cy + 6.5, 3, None, stroke=T300, sa=0.9, sw=1.3)
    cv_.text(x + 31, cy, "Repeatables", font(F_UI, 13.5), T200)
    y = cy + 30

    # --- UP NEXT --------------------------------------------------------------------------------
    j.hair(y)
    cy = y + 20
    cv_.text(j.l, cy + 1, "UP NEXT", font(F_DISPLAY, 12.5), GOLD, tracking=1.8)
    cv_.text(j.l + 92, cy, "first quest to do for each character", font(F_UI, 12.5), T500)
    y = cy + 18
    rows = []
    for npc in G.givers:
        q, st = G.up_next(npc, ME_DONE, ME_T)
        rows.append((npc, q, st))
    for i, (npc, q, st) in enumerate(rows):
        rh = 36
        cy = y + rh / 2
        hover = npc == "Piro"
        if hover:
            cv_.rrect(j.l - 8, y + 1, j.r + 8, y + rh - 1, 5, "#FFFFFF", 0.06)
        cv_.text(j.l, cy, G.giver_name[npc], font(F_UI, 13), T300)
        gx = j.l + 104
        quest_glyph(cv_, gx, cy, q, ME_DONE, ME_T)
        story = G.story(npc)
        line = G.line_of(q)
        pos = f"{line['order'].index(q) + 1}/{len(line['order'])}" if line else ""
        tw = cv_.text(gx + 18, cy, G.title(q), font(F_UI_SB if st == "tracked" else F_UI, 14.5), T100 if st != "locked" else T500)
        cv_.text(gx + 26 + tw, cy, pos, font(F_UI, 12.5), T500)
        rx = j.r
        if st == "tracked":
            w_ = j.chip(rx - 84, cy, "Done", icon="check", style="normal", hover=False)
            cv_.text(rx - 96, cy, "in progress", font(F_UI, 12.5), T300, anchor="rm")
        elif st == "available":
            wd = j.chip(rx - 150, cy, "Already done", icon="check", style="normal", hover=hover)
            j.chip(rx - 150 - 86, cy, "Track", icon="add", style="ghost")
        y += rh
    # repeatables summary + waiting
    rep = [k for k, v in G.q.items() if v["kind"] != "standard" and v["npc"] in G.giver_name and G.state(k, ME_DONE, ME_T) == "available"]
    cy = y + 14
    cv_.text(j.l + 104, cy, ICON["repeat"], font(F_ICON, 12), T500, anchor="mm")
    tw = cv_.text(j.l + 122, cy, f"{len(rep)} repeatable quests available (daily · weekly · monthly)", font(F_UI, 13), T300)
    cv_.text(j.l + 130 + tw, cy, "Show", font(F_UI_SB, 13), GOLD)
    y = cy + 22

    # --- CHARACTERS ----------------------------------------------------------------------------
    j.hair(y)
    cy = y + 20
    cv_.text(j.l, cy + 1, "CHARACTERS", font(F_DISPLAY, 12.5), GOLD, tracking=1.8)
    cv_.text(j.r, cy, "click a step: “I am here”", font(F_UI, 12.5), T500, anchor="rm")
    y = cy + 20

    def card_head(npc, y, expanded=False, sel=None):
        cy = y + 20
        cv_.text(j.l, cy + 0.5, ICON["down" if expanded else "right"], font(F_ICON, 9), T500, anchor="lm")
        cv_.text(j.l + 16, cy + 1, G.giver_name[npc].upper(), font(F_DISPLAY, 12.5), T100, tracking=1.5)
        line1 = next(ln for ln in G.lines if ln["questline_id"].split(".")[1] == npc)["order"]
        xs = stepper(cv_, j.l + 150, cy, line1, ME_DONE, ME_T, sel=sel)
        nd = sum(1 for q in line1 if q in ME_DONE)
        cv_.text(j.r, cy, f"{nd} / {len(line1)} done", font(F_UI, 12.5), T300, anchor="rm")
        return y + 40, xs, line1

    # ELOISE - expanded
    y, xs, line1 = card_head("Eloise", y, expanded=True)
    def qrow(qid, y, hover=False, actions=None, note=None, indent=40):
        rh = 28
        cy = y + rh / 2
        if hover:
            cv_.rrect(j.l + indent - 12, y + 1, j.r + 8, y + rh - 1, 5, "#FFFFFF", 0.06)
        st = quest_glyph(cv_, j.l + indent, cy, qid, ME_DONE, ME_T, 0.9)
        col = {"done": T300, "tracked": T100, "available": T100, "locked": T500}[st]
        tw = cv_.text(j.l + indent + 16, cy, G.title(qid), font(F_UI_SB if st == "tracked" else F_UI, 14), col)
        if note:
            cv_.text(j.l + indent + 26 + tw, cy, note, font(F_UI, 12.5), T500)
        rx = j.r
        for lab, ic, style in (actions or []):
            fsz = 12.5
            wch = cv_.measure(lab, font(F_UI_SB if style != "normal" else F_UI, fsz)) + (18 if ic else 0) + 22
            j.chip(rx - wch, cy, lab, icon=ic, style=style, h=24, size=fsz)
            rx -= wch + 6
        return y + rh

    y = qrow("Quest.Eloise.Q01", y)
    y = qrow("Quest.Eloise.Q04", y)
    y = qrow("Quest.Eloise.Q05", y, hover=True, actions=[("Reopen", "reopen", "normal")], note="done 3 min ago")
    y = qrow("Quest.Eloise.Q02", y, actions=[("Done", "check", "normal"), ("Untrack", None, "normal")], note="tracked")
    y = qrow("Quest.Eloise.Q06", y, note="after Last Embers")
    y = qrow("Quest.Eloise.Q07", y)
    cv_.text(j.l + 56, y + 9, "+ 2 more", font(F_UI, 12.5), T500)
    y += 22
    # line 2 gated by another character
    cy = y + 12
    cv_.text(j.l + 40, cy, ICON["lock"], font(F_ICON, 11), T500, anchor="mm")
    x = j.l + 56
    x += cv_.text(x, cy, "Line 2 starts after ", font(F_UI, 13), T300)
    x += cv_.text(x, cy, "Mr Sweet · A Touch Of Bribery", font(F_UI_SB, 13), T200)
    cv_.text(x + 6, cy, "(needs Eloise · Pushing Back The Shadows)", font(F_UI, 12.5), T500)
    y += 26
    y = qrow("Quest.Eloise.Q10", y)
    y = qrow("Quest.Eloise.Q11", y)
    cy = y + 12
    cv_.text(j.l + 40, cy, ICON["repeat"], font(F_ICON, 11), T500, anchor="mm")
    tw = cv_.text(j.l + 56, cy, "Repeatable: 7 daily · 4 weekly · 1 monthly — available", font(F_UI, 13), T300)
    cv_.text(j.l + 64 + tw, cy, "Show", font(F_UI_SB, 13), GOLD)
    y += 32

    # LOUIS - collapsed card with the confirm strip open on "A Gift For The Giver"
    target = "Quest.Louis.Q04"
    y0 = y
    y, xs, line1 = card_head("Louis", y, sel=target)
    tx = xs[line1.index(target)]
    # strip (callout pointing at the dot)
    miss = G.missing(target, ME_DONE)
    st_t, st_b = y - 2, y + 92
    cv_.rrect(j.l + 10, st_t, j.r, st_b, 6, GOLD, 0.07, stroke=GOLD, sa=0.45, sw=1)
    cv_.poly([(tx - 7, st_t + 0.5), (tx + 7, st_t + 0.5), (tx, st_t - 7)], "#1D1810")
    cv_.polyline([(tx - 7, st_t), (tx, st_t - 7), (tx + 7, st_t)], 1, GOLD, 0.45)
    cy = st_t + 18
    x = j.l + 24
    x += cv_.text(x, cy, G.title(target), font(F_UI_SB, 14.5), T100)
    cv_.text(x + 8, cy, "Louis · step 4 of 10", font(F_UI, 12.5), T500)
    cy += 24
    names = ", ".join(G.title(m) for m in miss)
    x = j.l + 24
    x += cv_.text(x, cy, f"“I am here” also marks {len(miss)} earlier quests done: ", font(F_UI, 13.5), T200)
    cv_.text(x, cy, names + ".", font(F_UI_SB, 13.5), T100)
    cy += 30
    x = j.l + 24
    x += j.chip(x, cy, "Track here", icon="add", style="primary", h=28, size=13) + 8
    x += j.chip(x, cy, "Done up to here", icon="check", style="normal", h=28, size=13) + 8
    j.chip(x, cy, "Cancel", style="normal", h=28, size=13)
    cv_.text(j.r - 14, cy, "Undo stays available", font(F_UI, 12), T500, anchor="rm")
    y = st_b + 8

    for npc in ("MrSweet", "Piro", "Reynauld"):
        y, xs, line1 = card_head(npc, y)
        if npc == "MrSweet":
            # bridge quest (standalone story quest)
            cy = y + 6
            cv_.text(j.l + 40, cy, ICON["lock"], font(F_ICON, 11), T500, anchor="mm")
            x = j.l + 56
            x += cv_.text(x, cy, "+ A Touch Of Bribery", font(F_UI, 13), T300)
            cv_.text(x + 8, cy, "side step · needs Eloise · Pushing Back The Shadows", font(F_UI, 12.5), T500)
            y += 22

    # --- footer: last change + Undo + History --------------------------------------------------
    fy = wy + H - 52
    cv_.rrect(wx + 4, fy, wx + W - 4, wy + H - 4, 0, "#0D0B08", 1.0)
    j.hair(fy, 0.25)
    cy = fy + 25
    cv_.text(j.l, cy + 0.5, ICON["history"], font(F_ICON, 13), T300, anchor="lm")
    x = j.l + 22
    x += cv_.text(x, cy, "Last change:", font(F_UI, 13), T500) + 6
    x += cv_.text(x, cy, "The Gavroche Family", font(F_UI_SB, 13), T200)
    cv_.text(x + 6, cy, "done · 3 min ago", font(F_UI, 13), T500)
    rx = j.r
    w2 = j.chip(rx - 96, cy, "History", icon="down", style="normal", h=28)
    j.chip(rx - 96 - 82, cy, "Undo", icon="undo", style="ghost", h=28)
    return (wx, wy, wx + W, wy + H)


def search_inset(cv, x, y):
    """The existing Search window (settings dark theme) with quest results that now carry their state."""
    w, h = 480, 290
    th = 36
    bg, card, sub, text, acc = "#202020", "#2B2B2B", "#A6A6A6", "#F2F2F2", "#D4A354"
    cv.rrect(x + 5, y + 8, x + w + 5, y + h + 8, 8, "#000000", 0.45)
    cv.rrect(x, y, x + w, y + h, 8, bg, 1.0, stroke="#000000", sa=0.8, sw=1)
    cv.rrect(x, y, x + w, y + th, 8, "#1B1B1B", 1.0)
    cv.rrect(x, y + th - 8, x + w, y + th, 0, "#1B1B1B", 1.0)
    cv.text(x + 14, y + th / 2, "Find an item or quest", font(F_UI, 13), "#E6E6E6")
    cv.text(x + w - 22, y + th / 2, ICON["xclose"], font(F_ICON, 9.5), "#CFCFCF", anchor="mm")
    yy = y + th + 12
    cv.text(x + 16, yy + 6, "Type a name · ↑ ↓ Enter", font(F_UI, 12), sub)
    yy += 20
    cv.rrect(x + 16, yy, x + w - 16, yy + 36, 5, "#383838", 1.0, stroke="#454545", sa=1, sw=1)
    cv.line(x + 20, yy + 35, x + w - 20, yy + 35, 2, acc)
    cv.text(x + 28, yy + 18, "ga", font(F_UI, 14.5), text)
    yy += 48
    rows = [
        ("Quest.Louis.Q02", "Available · 40 spots here", "Track"),
        ("Quest.Louis.DQ02", "Locked · repeatable, after A Gift For The Giver", "Open in journal"),
        ("Quest.Eloise.Q05", "Done", "Open in journal"),
    ]
    for i, (qid, state, act) in enumerate(rows):
        rh = 40
        if i == 1:
            cv.rrect(x + 10, yy, x + w - 10, yy + rh, 5, "#3A3A3A", 1.0)
        cy = yy + rh / 2
        quest_glyph(cv, x + 26, cy, qid, ME_DONE, ME_T, 0.85)
        cv.text(x + 40, cy - 7, G.title(qid), font(F_UI, 14), text)
        cv.text(x + 40, cy + 10, f"{G.giver_name[G.npc(qid)]} · {state}", font(F_UI, 12), sub)
        hi = i == 1 or act == "Track"
        cv.text(x + w - 22, cy, act, font(F_UI_SB if hi else F_UI, 12.5), acc if hi else sub, anchor="rm")
        yy += rh + 2
    return (x, y, x + w, y + h)


def scene_journal():
    base = Image.open(os.path.join(HERE, "base_default.png")).convert("RGB")
    crop = base.crop((330, 60, 1890, 1400)).filter(ImageFilter.GaussianBlur(1.2))
    crop = Image.blend(crop, Image.new("RGB", crop.size, (12, 10, 8)), 0.45)
    W, H = crop.size
    cv = Canvas(W, H)
    journal_window(cv, 40, 40)
    sx, sy = 40 + JW + 50, 40
    cv.text(sx, sy + 10, "SEARCH WINDOW · QUEST RESULTS", font(F_UI_SB, 12), T300, tracking=1.0)
    search_inset(cv, sx, sy + 30)
    # legend of the stepper glyphs
    lx, ly = sx, sy + 440
    cv.rrect(lx, ly, lx + 480, ly + 210, 8, INK_950, 0.92, stroke=LINE, sa=0.32, sw=1)
    cv.text(lx + 16, ly + 22, "STEP STATES", font(F_DISPLAY, 12), GOLD, tracking=1.6)
    items = [("Quest.Eloise.Q01", "Done"), ("Quest.Eloise.Q02", "In progress (tracked, its badge)"),
             ("Quest.Louis.Q02", "Available (all prerequisites done)"), ("Quest.Louis.Q05", "Locked (a prerequisite is not done)")]
    for i, (q, lab) in enumerate(items):
        cy = ly + 56 + i * 36
        quest_glyph(cv, lx + 30, cy, q, ME_DONE, ME_T)
        cv.text(lx + 52, cy, lab, font(F_UI, 13.5), T200)
    out = composite(crop, cv)
    out.save(os.path.join(HERE, "journal.png"))


# ----------------------------------------------------------------------------------------------
# Scene 3: panel states
# ----------------------------------------------------------------------------------------------
def scene_panels():
    base = Image.open(os.path.join(HERE, "base_default.png")).convert("RGB")
    bg = base.crop((0, 0, 1500, 900))
    bg = Image.blend(bg, Image.new("RGB", bg.size, (14, 12, 10)), 0.45)
    W, H = bg.size
    cv = Canvas(W, H)
    cap = font(F_UI_SB, 13)

    # A: toast after ✓ Done on Last Embers, UP NEXT now shows Mapping The Front first
    x = 30
    cv.text(x, 26, "A · AFTER [DONE] ON “LAST EMBERS”", cap, T300, tracking=0.8)
    p = Panel2(cv, x, 44)
    p.header()
    p.toast_next(G.title("Quest.Eloise.Q02"), G.title("Quest.Eloise.Q06"))
    p.search_field()
    p.section2("QUESTS", 2, link="Journal", icon="journal")
    p.owner_done("2", "#5ec5e9", G.title("Quest.Reynauld.Q02"), "0 spots")
    p.warning("Only below your 10 % filter here", "Show ≥ 5 %")
    p.objective("Find 10x Heavy Ammunition.", "elsewhere")
    p.owner_done("3", "#7ed957", G.title("Quest.MrSweet.Q02"), "8 spots")
    p.objective("Obtain a Longboy for Mr Sweet.", "here", "#7ed957")
    p.subheader("Up next", 3, right="first per character")
    p.next_row(G.title("Quest.Eloise.Q06"), "Eloise", here=2, step="5/8", hover=True)
    p.next_row(G.title("Quest.Louis.Q02"), "Louis", here=0, step="2/10")
    p.next_row(G.title("Quest.Piro.Q01"), "Piro", here=0, step="1/9")
    p.link_row("13 repeatable · Journal", indent=28)
    p.finish()

    # B: allies - one hidden, compact (overflow level 3: one line per ally)
    x = 520
    cv.text(x, 26, "B · ALLIES: HIDDEN ALLY, FOLDED QUESTS", cap, T300, tracking=0.8)
    p = Panel2(cv, x, 44)
    p.header()
    p.section2("ALLIES", 2, link="Manage", icon="people")
    p.ally_row(ALLIES[0], "", hidden=True)
    p.ally_row(ALLIES[1], "3 spots")
    p.note([("2 quests: Signs Of Life, Mapping The Front", T500)], indent=28)
    p.finish()

    cv.text(x, 290, "C · NO ALLY YET", cap, T300, tracking=0.8)
    p = Panel2(cv, x, 308)
    p.header()
    p.section2("ALLIES", None, link="Add", icon="addfriend")

    def empty_op(ox, oy, y=p.y):
        m_ally(cv, ox + PAD + 10, oy + y + 4, T500, "+", 0.9)
        cv.text(ox + PAD + 28, oy + y + 4, "Nobody yet. Pick a raid mate from your hub,", font(F_UI, 13), T500)
        cv.text(ox + PAD + 28, oy + y + 24, "or add someone who has no app.", font(F_UI, 13), T500)
    p.add(empty_op)
    p.y += 40
    p.finish()
    cv.text(x, 500, "D · COLLAPSED", cap, T300, tracking=0.8)
    p = Panel2(cv, x, 518, w=400)
    p.header(collapsed=True, title="COMPANION", summary="3 quests · 2 allies · 1 layer")
    p.finish()

    # D: ally picker (journal "+ Ally" tab)
    x = 1000
    cv.text(x, 26, "E · JOURNAL › + ALLY (max 2)", cap, T300, tracking=0.8)
    w, h = 470, 460
    y = 44
    cv.rrect(x + 5, y + 8, x + w + 5, y + h + 8, 8, "#000000", 0.45)
    cv.rrect(x, y, x + w, y + h, 8, INK_950, 1.0, stroke=LINE, sa=0.32, sw=1)
    j = J(cv, x, y, w)
    cy = y + 30
    cv.text(j.l, cy, "RAID ALLIES", font(F_DISPLAY, 13), GOLD, tracking=1.8)
    cv.text(j.r, cy, "1 of 2", font(F_UI, 12.5), T500, anchor="rm")
    cy += 26
    cv.text(j.l, cy, "Their quests show hollow on your map, with their initial.", font(F_UI, 13), T300)
    cy += 30
    j.hair(cy - 12)
    j.caps(j.l, cy + 4, "From your hub on hunger-companion.com")
    cy += 32
    hub = [("Marius", "Friend", "2 quests", True), ("Aurélie", "Friend", "4 quests", False), ("Gaspard", "Follows you", "1 quest", False),
           ("Sixtine", "You follow", "3 quests", False)]
    for name, rel, nq, added in hub:
        rh = 40
        c = cy + rh / 2 - 12
        if added:
            m_ally(cv, j.l + 12, c, ALLY1, name[0], 1.0)
        else:
            cv.circle(j.l + 12, c, 9, INK_700)
            cv.text(j.l + 12, c + 0.5, name[0], font(F_UI_B, 10.5), T300, anchor="mm")
        tw = cv.text(j.l + 32, c, name, font(F_UI_SB, 14.5), T100)
        cv.text(j.l + 40 + tw, c, f"{rel} · {nq}", font(F_UI, 12.5), T500)
        if added:
            j.chip(j.r - 92, c, "Added", icon="check", style="normal", h=26)
        else:
            j.chip(j.r - 72, c, "Add", icon="add", style="ghost", h=26)
        cy += rh
    cy += 4
    j.hair(cy)
    cy += 22
    j.caps(j.l, cy, "Someone without the app or an account")
    cy += 30
    cv.rrect(j.l, cy - 17, j.r - 92, cy + 17, 6, "#0B0906", 0.75, stroke=GOLD, sa=0.6, sw=1)
    cv.text(j.l + 14, cy, "Lou", font(F_UI, 14), T100)
    cv.line(j.l + 41, cy - 9, j.l + 41, cy + 9, 1.2, T100)
    j.chip(j.r - 84, cy, "Add", icon="add", style="primary", h=32)
    cy += 30
    cv.text(j.l, cy, "Kept on this PC only. Pick their quests in their tab", font(F_UI, 12.5), T500)
    cv.text(j.l, cy + 18, "(they tell you; same suggestions as yours).", font(F_UI, 12.5), T500)
    out = composite(bg, cv)
    out.save(os.path.join(HERE, "panel_allies.png"))


# ----------------------------------------------------------------------------------------------
# Scene 4: marker grammar sheet
# ----------------------------------------------------------------------------------------------
def scene_sheet():
    base = M.clean_zoomed_base()
    par = base.crop((420, 640, 1020, 1240))
    par = par.resize((par.width * 2, par.height * 2), Image.LANCZOS)
    par = Image.blend(par, Image.new("RGB", par.size, (232, 226, 210)), 0.25)
    W, H = par.size
    cv = Canvas(W, H)
    s = 2.0
    C1, C2 = "#e9745e", "#7ed957"
    cv.rrect(20, 20, W - 20, 84, 10, INK_950, 0.9)
    cv.text(40, 52, "ALLY MARKERS", font(F_DISPLAY, 26), GOLD, tracking=3)
    for x, t in ((130, "mine"), (260, "ally")):
        cv.text(x, 112, t, font(F_UI_SB, 22), "#2A2118", anchor="mm")

    def lab(y, text):
        lw = cv.measure(text, font(F_UI_SB, 24)) + 24
        cv.rrect(430, y - 21, 430 + lw, y + 21, 8, "#15110C", 0.85)
        cv.text(442, y + 1, text, font(F_UI_SB, 24), T100)

    rows = 9
    rh = (H - 150) / rows
    ys = [150 + i * rh + rh / 2 for i in range(rows)]
    # 1 container / spot
    M.m_tracked_disc(cv, 130, ys[0], C1, "1", s)
    m_ally(cv, 260, ys[0], ALLY1, "M", s)
    lab(ys[0], "Container / spot: filled vs hollow")
    # 2 harvest node
    M.m_tracked_square(cv, 130, ys[1], C1, "1", s)
    m_ally(cv, 260, ys[1], ALLY1, "M", s, shape="square")
    lab(ys[1], "Harvest node")
    # 3 placed object
    M.m_tracked_diamond(cv, 130, ys[2], C1, "1", s)
    m_ally(cv, 260, ys[2], ALLY2, "L", s, shape="diamond")
    lab(ys[2], "Placed object")
    # 4 cluster
    M.m_cluster(cv, 130, ys[3], C1, "1", 5, s)
    ally_cluster(cv, 260, ys[3], ALLY1, "M", 4, s)
    lab(ys[3], "Cluster (one per ally, all quests)")
    # 5 hover
    M.m_tracked_disc(cv, 130, ys[4], C1, "1", s, h=True)
    m_ally(cv, 260, ys[4], ALLY2, "L", s, h=True)
    lab(ys[4], "Hover (×1.25 + focus ring)")
    # 6 pip: same spot, same objective
    M.m_tracked_disc(cv, 130, ys[5], C1, "1", s)
    ally_pip(cv, 130, ys[5], 10 * s, ALLY1, "M", s, 0)
    M.m_tracked_disc(cv, 260, ys[5], C1, "1", s)
    ally_pip(cv, 260, ys[5], 10 * s, ALLY1, "M", s, 0)
    ally_pip(cv, 260, ys[5], 10 * s, ALLY2, "L", s, 1)
    lab(ys[5], "Same spot + same objective: pip")
    # 7 petals mine + ally, different objectives
    cx, cy = 195, ys[6]
    cv.circle(cx, cy, 3.2 * s, HALO, 0.7)
    cv.circle(cx, cy, 2.0 * s, IVORY)
    for nx, fn in ((cx - 28 * s / 2 * 2, "me"), (cx + 28 * s / 2 * 2, "ally")):
        cv.line(cx, cy, nx, cy, 2.6 * s, HALO, 0.55)
        cv.line(cx, cy, nx, cy, 1.2 * s, IVORY, 0.9)
    M.m_tracked_disc(cv, cx - 28 * s, cy, C2, "3", s)
    m_ally(cv, cx + 28 * s, cy, ALLY2, "L", s)
    lab(ys[6], "Different objectives: petals, mine first")
    # 8 vs Quests layer token (distinction)
    M.m_quest_avail(cv, 130, ys[7], s)
    m_ally(cv, 260, ys[7], ALLY2, "L", s)
    lab(ys[7], "Not a token: white hairline = tracked")
    # 9 spotlight: ally hovered in panel -> everything else at 35 %
    cv.fade = 0.35
    M.m_tracked_disc(cv, 130, ys[8], C1, "1", s)
    cv.fade = 1.0
    m_ally(cv, 260, ys[8], ALLY1, "M", s)
    M.pulse_ring = None
    cv.ring(260, ys[8], 9 * s + 7 * s, 2 * s, ALLY1, 0.5)
    lab(ys[8], "Spotlight on Marius (panel hover)")
    out = composite(par, cv)
    out.save(os.path.join(HERE, "ally_markers.png"))


if __name__ == "__main__":
    pack = M.load_pack()
    scene_map(pack)
    scene_journal()
    scene_panels()
    scene_sheet()
    print("written: allies_map.png, journal.png, panel_allies.png, ally_markers.png")
