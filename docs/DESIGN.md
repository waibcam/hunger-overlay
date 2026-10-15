# Hunger Overlay: map overlay design spec

Status: proposal (2026-10-14). Scope: what is drawn over the in-game map, meaning the markers, the side panel and the tooltips. The Search and Settings windows are out of scope; they are already fine.

Mockups (rendered by `docs/design/mockup.py` over real screenshots, with real marker positions from the cached pack; the
screenshots `base_default.png` / `base_zoomed.png` are not in the repository):

| File | Shows |
|---|---|
| `docs/design/mockup_default.jpg` | Default map view. 3 tracked quests, a crafting recipe and an item search; Extractions and the new Quests layer are ON; tooltip on an untracked quest spot. |
| `docs/design/mockup_zoomed.jpg` | Zoom ≈ 2.5×. All layers ON, place labels, a tracked-container tooltip with a chance bar, and the "Now tracking" toast after a Ctrl+click. |
| `docs/design/panel_collapsed.jpg` | Panel states: collapsed, nothing tracked, key/network problem, overflow. |
| `docs/design/markers_sheet.jpg` | Every marker type at 2×, normal and hover. |

All sizes below are **px at 1440p**. Multiply by `sc = client_height / 1440`, as the code already does. Marker sizes are also multiplied by `cfg.marker_scale`.

---

## 1. Design principles

1. **Readability on parchment first.** The map is beige with dense black ink hatching. Every overlay element sits on a dark element: a dark halo, a dark token disc or a dark plaque. No overlay element relies on thin coloured lines drawn straight on the paper.
2. **Shape tells "what is it", colour tells "whose is it".** Each shape family has one meaning:
   - circle = container or objective spot
   - square = harvest node
   - diamond = place or placed object
   - dark token with a glyph = information layer

   Colour is the owner's colour (a quest, recipe or search) or the layer's colour. Never use colour alone.
3. **Colour-blind safe by construction.** Every tracked owner gets a **badge character**, drawn both in the panel and inside its map markers. Quests get `1…9` and recipes/searches get `A…Z`. Two quests that look identical to a deuteranope (salmon `#e9745e` vs green `#7ed957`) still read `1` and `3`. Danger layers use different glyphs (✕, ☠, dashed ring), not only different reds.
4. **Hierarchy by commitment.** What the player is actively doing (tracked) is bright, filled, ringed in white and on top. Discoverable things (the Quests layer) are mid-weight. Reference layers (traps, locks, loot) are dark tokens or small chips. Dense layers (sound traps, 289 points) fade to "dust" at overview zoom.
5. **Match the game, don't copy it.** The game UI is near-black with thin light double borders and serif caps with wide tracking ("SARLAT FARM", "CLOSE"). The panel reuses this: a double hairline frame, Palatino caps titles and gold hairlines. The body text stays Segoe UI for legibility at small sizes.
6. **Minimal clutter by default.** First run: only Extractions is ON. Everything else is one click away and remembered.
7. **Never block the game.** The marker layer stays click-through. Every action lives in the panel, which is `WS_EX_NOACTIVATE`, or in a passive Ctrl+click (§7).

---

## 2. Rendering notes (what the primitives must do)

Everything below can be drawn with what `render.rs` already has, plus three small additions:

| Need | How |
|---|---|
| Filled diamond | Either `diamond(x, y, r/2, width = r)` (the current thick-stroke trick), or better, a new `fill_diamond()` built on a 4-point `ID2D1PathGeometry` (≈15 lines). The mockups use a true polygon. |
| Dashed circle | `stroke_circle(.., dashed = true)` (exists). Dash style: `D2D1_DASH_STYLE_DASH`, round caps. |
| Letter-spaced caps (titles) | `IDWriteTextLayout1::SetCharacterSpacing(0, tracking, 0, range)` on a layout built for the string. Fallback: insert U+200A HAIR SPACE between letters. |
| Extra font roles | Extend `Font` with `Display` (Palatino Linotype Bold), `Caps` (Segoe UI Semibold, small) and `Icon` (Segoe Fluent Icons; Segoe MDL2 Assets on Windows 10). All three ship with Windows. |
| Icons | Draw them as text in the Icon font: settings `E713`, search `E721`, chevron down `E70D`, chevron right `E76C`, close `E711`, add `E710`, remove `E738`, globe `E774`, warning `E7BA`, check `E73E`, hidden `ED1A`. Skull `☠` U+2620 comes from Segoe UI Symbol. Do not use emoji (🔍 🌐 🛠 ⚙ ✥): they render inconsistently in grayscale AA. |
| Text AA | Keep `D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE`. |

---

## 3. Design tokens

### 3.1 Colours (panel and tooltip)

| Token | Hex | Alpha | Use |
|---|---|---|---|
| `ink-950` | `#110E0A` | 0.94 × user opacity | Panel and tooltip background |
| `ink-900` | `#0B0906` | 0.75 | Inset field (search box) |
| `ink-800` | `#241D16` | 0.95 | Chips and icon buttons |
| `ink-700` | `#30271D` | 1.0 | Hover fill, "on" layer pill, count pills |
| `frame-outer` | `#000000` | 0.70 | Panel outer 1 px edge |
| `line-gold` | `#C8A765` | 0.32 (inner frame) / 0.18 (separators) / 0.38 (tooltip edge) | Hairlines |
| `gold` | `#D4AF6A` | 1 | Section titles, header title, links |
| `gold-hi` | `#EBCB8B` | 1 | Hover, focus ring, action text |
| `accent` | `#D9A54A` | 1 | Primary button and "on" toggle (same as the Settings window) |
| `text-100` | `#F1EADB` | 1 | Titles and values |
| `text-200` | `#D9D1C2` | 1 | Body (objectives) |
| `text-300` | `#B3A995` | 1 | Secondary and labels |
| `text-500` | `#857C6D` | 1 | Disabled, "off", hints, counts |
| `warn` | `#F0A24B` | fill 0.10 / edge 0.38; text `#F3C38A` | Filter and key warnings |
| `ok` | `#86C27E` | fill 0.13 / edge 0.45 | Sync dot and "Now tracking" toast |
| `hover-row` | `#FFFFFF` | 0.07 | Row hover background |
| `chip-edge` | `#FFFFFF` | 0.08 (0.50 gold on hover) | Chip borders |
| `shadow` | `#000000` | 0.35, offset (3, 5) | Panel and tooltip drop shadow (a second rrect) |

### 3.2 Colours (markers)

| Token | Hex | Notes |
|---|---|---|
| `halo` | `#0B0907` | Under every marker: alpha 0.45–0.75 (see table §5) |
| `ring-white` | `#FFFFFF` @ 0.95 | Ring of tracked markers |
| `token-fill` | `#16120D` @ 0.92 | Disc of information tokens |
| Owner colours | From the website (`tracked[].color`, `crafting[].color`) | Searches use a fixed rotating set: `#F5D547`, `#FF9EC7`, `#7FE0D0`, `#C8E36B` |
| `extraction` | `#3E9EDB` | Close to the game's own blue extraction icons |
| `spawn` | `#F3ECDD` | Ivory crosshair |
| `boss` | `#E0453A` ring, glyph `#F8DCD4` | |
| `trap` | `#F07A2E` ring, glyph `#FF9C5C` | |
| `sound` | `#A46BE0` | |
| `key` | `#E3B341` ring, glyph `#F2CF68` | |
| `lock` | `#A9BCC6` ring, glyph `#D5E1E7` | |
| `loot` | `#F2C230` | Placed-loot diamond, highlight `#FFF6D8` |
| `harvest` | Naturalist `#62C26A`, Scavenger `#D1A23A`, Conservator `#D9739F` | Conservator moves from purple to rose so it no longer clashes with sound traps. |
| `quest-avail` | `#F0D58C` | "!" token of the Quests layer |
| `place` | `#D4AF6A` (= gold) | POI diamond |
| `done` | `#6F6A62` fill, `#F3ECDD` @ 0.6 ring | |

**Owner colour guard:** if a website colour has luminance < 70 (it would vanish in the halo), lighten it by 25 %. Badge text is `#17120C` on light fills (luminance > 120) and `#FFFFFF` otherwise.

### 3.3 Typography (1440p)

| Role | Font | Size | Weight | Tracking | Colour |
|---|---|---|---|---|---|
| Panel title | Palatino Linotype | 14.5 | Bold | 2.0 | `gold` |
| Section title (caps) | Palatino Linotype | 12.5 | Bold | 1.8 | `gold` |
| Sub-header / layer group / tooltip eyebrow (caps) | Segoe UI | 11 | Semibold | 1.2 (eyebrow 1.1) | `text-500` (eyebrow `text-300`) |
| Owner title (quest, recipe, search) | Segoe UI | 15 | Semibold | 0 | `text-100` |
| Body / objective | Segoe UI | 14 | Regular | 0 | `text-200` |
| Chip / layer label | Segoe UI | 13.5 | Regular | 0 | `text-200` / `text-100` when on |
| Counts / hints | Segoe UI | 12–12.5 | Regular | 0 | `text-500` |
| Badge digit | Segoe UI | 11.5 | Bold | 0 | `#17120C` / white |
| Map label | Segoe UI | 12.5 | Semibold | 0 | `text-100` |
| Tooltip title | Segoe UI | 15.5 | Semibold | 0 | `text-100` |
| Tooltip line | Segoe UI | 13.5 | Regular | 0 | key `text-300`, value `text-100` |

Line heights: owner row 30, objective row 22, hint +19, sub-header 22 (+6 top), layer pill row 36 (pill 30 + 6 gap).

### 3.4 Spacing, radii, strokes

- **Spacing scale:** 2 · 4 · 6 · 8 · 12 · 14 · 16 · 24. Panel side padding = 14, section gap = 12. Objective indent = 28 (the badge column), objective glyph at x + 34 and text at x + 46.
- **Radii:**

  | Element | Radius |
  |---|---|
  | Panel | 7 (inner frame 5) |
  | Tooltip | 8 |
  | Chips, buttons, pills, fields | 6 |
  | Small buttons (✕, Track) | 5 |
  | Map label plaque | 4 |
  | Count pills | full |
- **Strokes:**

  | Element | Stroke |
  |---|---|
  | Hairlines | 1 |
  | Chip edges | 1 |
  | Tracked marker ring | 2.0 (white) |
  | Token ring | 1.8 (extraction 2.6, boss 2.2) |
  | Focus ring | 2.2 `gold-hi` over 4.4 `halo` @ 0.55 |
  | Place diamond outline | 2.2 |
  | Leader lines | 1.2 ivory over 2.6 halo |
- **Panel frame:** shadow rrect (+3, +5, black 0.35), then the body rrect with a 1 px black edge @ 0.7, then an **inner** 1 px `line-gold` @ 0.32 frame inset by 3 px. This is the game's double-line box.
- **Legibility on parchment:** every marker gets a `halo` underlay 2–3.5 px larger than its shape. Map labels sit on a dark plaque (`#15110C` @ 0.82 with a `line-gold` @ 0.30 edge). Never draw text directly on the paper.

---

## 4. Panel

### 4.1 Geometry

- **Width:** 430 expanded, 360 collapsed.
- **Default position:** 24 px right of the map square, top 60 (unchanged). It can be moved by dragging the header, and double-clicking the header resets it.
- **Max height:** `min(map_square.bottom, client_h − 20) − top`. With the default position this is ≈ 1340 px, which fits everything in the default mockup (≈ 1010 px).

### 4.2 Section order

```
┌ HEADER ─────────────────────────────────────── 46 ┐
│ ⠿ HUNGER COMPANION ●                   [⌄] [⚙]   │  ⠿ grip + title = drag handle · ● sync status
├───────────────────────────────────────────────────┤
│ (toast, transient: ✓ Now tracking … Undo)          │
│ [⌕ Find an item or quest…                     ]   │  search field (opens the Search window)
│ [☐ Show done] [− | Chance ≥ 10 % | +]     [◎ EN]  │  quick settings
│ ⌄ QUESTS ③                                        │
│   ① Making Dough                       40 spots    │  click = hide/show; hover = map spotlight
│       ● Find 20x Flour.                            │
│   ③ Sweet Lubrication                  40 spots    │
│       ● Find 3x Tavern Wine.                       │
│       ● Find 3x Thin Mead.                         │
│   Other map: ② Still Alive                         │  quests with 0 markers here, 1 dim line
│   AVAILABLE ON THIS MAP 17          Quests layer   │  only when the Quests layer is ON
│   ! Mapping the Front  Eloise          [+ Track]   │
│   ! Signs Of Life  Eloise              [+ Track]   │
│   ! A Gift for the Giver  Louis        [+ Track]   │
│   Show 14 more ⌄                                   │
│ ⌄ CRAFTING ①                                      │
│   Ⓐ Grubber's Purse                                │
│       ○ Bolt of Cotton Cloth ×7                    │
│       ○ Delicate Chain ×1                          │
│     ⚠ Only below your 10 % filter here [Show ≥1 %] │
│ ⌄ SEARCH ①                                        │
│   Ⓑ Iron Ore                       11 spots  [✕]   │
│ ⌄ MAP LAYERS                              2 on     │
│   SURVIVAL                                         │
│   [⇧ Extractions 23] [⌖ Player spawns 18]          │
│   [☠ Minibosses  24] [✕ Traps         21]          │
│   [◌ Sound traps 289]                              │
│   LOOT & ACCESS                                    │
│   [◆ Placed loot 68] [■ Harvest · your levels 123] │
│   [⚷ Key spots    5] [🔒 Locked        56]          │
│   WORLD                                            │
│   [! Quests NEW  25] [◇ Places        31]           │
└───────────────────────────────────────────────────┘
```

### 4.3 Section details

**Header (46).** The grip is 2×3 dots (r 1.5, `text-500`). The title is "HUNGER COMPANION" (Display, gold, tracking 2), followed by a sync dot (r 3.5): `ok` when synced, `warn` when offline or the key is bad, and pulsing `text-500` while syncing. Two 28×28 icon buttons sit on the right: collapse (`E70D`, which becomes `E76C` when collapsed) and settings (`E713`). The ✥ button is removed: the whole header except the buttons is the drag handle, and the grip dots signal it. A hairline sits under the header.

**Collapsed (`panel_collapsed.jpg` A).** The header is shown alone. The title shortens to "COMPANION", followed by a summary in `text-300` 13 px: "3 quests · 2 layers". If the summary does not fit, ellipsize it. If there is a key or network problem, the dot turns `warn` and the summary becomes the problem ("No API key").

**Toast (32, transient).** It appears under the header for 6 s after an action that changes server state (track or untrack): "✓ Now tracking Mapping the Front · Undo". Use the `ok` tint, with `Undo` as a `gold-hi` underlined link. Only one toast at a time; a new one replaces the old.

**Search field (34).** It looks like an input (inset `ink-900`, 1 px white @ 0.10, gold on hover), with the `E721` icon and the placeholder "Find an item or quest…". Clicking it opens the Search window. It replaces the old "🔍 Find an item…" chip.

**Quick settings (30).**
- **"Show done"**: a checkbox chip. When on, the box is filled with `accent` and shows a dark ✓.
- **Chance stepper**: one segmented chip, `[−] Chance ≥ 10 % [+]`, with hairline separators. The steps are 0 / 1 / 2 / 5 / 10 / 15 / 20 / 25 / 33 / 50 %; non-linear steps are more useful than ±1.
- **Language**: a chip right-aligned on the same row (`E774` + "EN").

On narrow panels, the language chip wraps first.

**QUESTS section** (collapsible, expanded by default).
- **Owner row (30).** A badge disc (r 9.5, owner colour, 1.4 white ring, badge character), the title in Semibold 15, and on the right "`N spots`" in `text-500` (the markers on this map after filters). Clicking the row toggles hidden. A hidden quest's row turns `text-500` at 40 % with an `ED1A` icon instead of the count, and its objectives are not listed. Hovering a row turns on the **map spotlight** (§7.3).
- **Objective row (22).** The status glyph comes first:
  - ● dot r 4 in the owner colour = has markers on this map
  - ○ ring `text-500` = none here
  - ✓ `text-500` = done (only listed when Show done is on)

  The label follows in `text-200`. A website hint (`Best: Flour Sack (48 %) …`) is shown in `text-500` 12.5 under the label, **only** for an objective that has no marker here. Otherwise the hint goes in the tooltip.
- **Other-map quests.** Tracked quests with 0 markers on this map collapse into **one** dim line: "Other map: ② Still Alive ③ …". This saves 2–3 rows per quest. Clicking a name expands that quest in place.
- **Order:** quests in tracking order (their numbers never change while tracked). Other-map quests go last.
- **"AVAILABLE ON THIS MAP" sub-list** (only when the Quests layer is ON):
  - The sub-header shows a count and a right-aligned "Quests layer" hint.
  - Each row has a mini "!" token, the quest title (`text-200` 14), the giver (`text-500` 12.5) and a `[+ Track]` ghost button (gold edge 0.55, `gold-hi` text; hover fills it with gold @ 0.22).
  - Show 3 rows, then "Show N more ⌄", which expands to at most 10 more and then scrolls.
  - Sort order: quests whose prerequisites are met first (when the pack gives them), then by giver, then by title.

**CRAFTING section** (collapsible, expanded). The owner row uses badge `A…`; append "×N" to the title when quantity > 1. Ingredients are objective rows ("Bolt of Cotton Cloth ×7").

When **all** of a recipe's (or quest's) spots are hidden by the chance filter, show a **warning callout** instead of the orange sentence. It is indented under the owner, uses the `warn` tint, has a `E7BA` icon, and reads "Only below your 10 % filter here", with an action chip `[Show ≥ 1 %]`. The chip lowers the filter to the highest step that reveals at least one point. This replaces "points below the chance threshold (lower "Chance ≥")", which told the user what to do but did not let them do it.

**SEARCH section** (collapsible, expanded; hidden when empty). One owner row per searched item: badge `B…`, item name, "`N spots`" and a 22×22 ✕ button that removes the search. "Clear all" appears right-aligned in the section header when there are 2 or more searches.

**MAP LAYERS section** (collapsible, expanded on first run, state remembered). The header shows "`k on`" on the right. Clicking a **group label** toggles the whole group: if any layer in the group is on, all turn off; otherwise all turn on.

Layers are pills in a two-column grid (pill 30 high, 6 gap). Each pill shows a live swatch (the real marker drawn at 0.8×), the name and a right-aligned count.

| State | Background | Edge | Name | Swatch |
|---|---|---|---|---|
| ON | `ink-700` | `gold` @ 0.55 | `text-100` | Full colour |
| OFF | black @ 0.18 | white @ 0.08 | `text-500` | Drawn at 42 % alpha |

The checkbox glyph is dropped: the pill state is unambiguous (fill, edge and text all change) and it saves 18 px.

| Group | Layer (pack id) | Default | Why |
|---|---|---|---|
| **Survival** | Extractions (`extractions`) | **ON** | The thing you always need. |
| | Player spawns (`player_spawns`) | OFF | Useful at raid start only. |
| | Minibosses (`minibosses`) | OFF | |
| | Traps (`traps`) | OFF | |
| | Sound traps (`sound_traps`) | OFF | 289 points; dust mode (§5.4). |
| **Loot & access** | Placed loot (`placed_loot`) | OFF | |
| | Harvest · your levels (3 harvest layers merged in the UI) | OFF | Only nodes at or below your level; the tooltip names the profession. |
| | Key spots (`keys`) | OFF | |
| | Locked (`locks`) | OFF | |
| **World** | Quests (**new**, `quests`) | OFF | Shows a `NEW` tag (accent pill) until the user turns it on once. |
| | Places (`places`) | OFF | The game map already prints most names. |

The client defaults above override the pack's `default_on` (the pack currently says `player_spawns: true`). Change it there too.

### 4.4 Empty and warning states (`panel_collapsed.jpg` B, C)

- **Nothing tracked:** "No quest tracked yet." (`text-200` Semibold), followed by two dim lines: "Track quests on hunger-companion.com, or turn on the Quests layer to discover them on this map." Below them, a primary button `[Show quests on this map]` turns the Quests layer on.
- **Key missing or invalid:** a warn callout (title + explanation) and a primary `[Set up…]` button that opens Settings. Everything below the header is hidden.
- **Offline or rate-limited:** the warn callout without a button. The rest of the panel works from the cache, and the sync dot turns `warn`.
- **Map position uncertain** (`m.uncertain`): a one-line callout with an info icon: "Re-locating the map… markers hidden".

### 4.5 Overflow rules (`panel_collapsed.jpg` D)

When the content exceeds the max height, apply these steps in order until it fits:

1. Collapse "Available on this map" to its sub-header and "Show N quests".
2. Collapse other-map quests (already one line) and hide done objectives even if "Show done" is on. Show "+N done" at the end of the owner row instead.
3. Cap objectives per owner at 2, with "+N more objectives" (○ style).
4. Collapse CRAFTING and SEARCH to their headers. The header shows a summary, e.g. "Grubber's Purse".
5. Then **scroll** the panel body. The header and quick settings stay fixed. Mouse wheel over the panel scrolls 3 rows; the panel already receives clicks, and wheel is delivered to the window under the cursor on Windows 10+. A 30 px fade to `ink-950` at the bottom edge and a centred "N more sections · scroll ⌄" line show that there is more.

Never truncate inside a row except by **ellipsizing** a single-line title (character trimming, as the Title format does today).

---

## 5. Marker system

### 5.1 Families

| Family | Look | Meaning |
|---|---|---|
| **Tracked** | Bright fill in the owner colour, 2 px white ring, badge character inside | Things you are doing now |
| **Token** | Dark disc `#16120D` @ 0.92, coloured ring, light glyph | Information layers |
| **Chip** | Small filled shape with a dark outline/halo, no ring | Dense resource layers |
| **Place** | Dark diamond with a gold outline and a gold centre, plus a label plaque | Named places |

### 5.2 Catalogue

Sizes are radius r or half-side k at `marker_scale = 1`. "Halo" means a shape of the same kind, larger by the given amount, in `halo` colour.

| # | Type | Shape | Size | Fill | Outline / inner | Colour source | Z | Hover |
|---|---|---|---|---|---|---|---|---|
| 1 | Tracked objective, container point | Circle | r 10 | Owner colour | Halo +2.5 @ 0.6; white ring 2.0 drawn at r−1; badge char 11.5 Bold (no badge = white centre dot r·0.28) | Quest | 13 | ×1.25 + focus ring |
| 2 | Tracked optional objective | As 1 | r 10 | Owner | + dashed ring r+5, 1.5 px, owner colour | Quest | 13 | as 1 |
| 3 | Tracked spot (go to / inspect / talk, `map_coords`) | Circle | r 10 | Owner | As 1 + dashed ring r+5 **only while the objective is the next one**, as a "go here" cue | Quest | 13 | as 1 |
| 4 | Crafting ingredient (container) | Circle | r 10 | Recipe colour | As 1 with badge `A…` | Recipe | 12 | as 1 |
| 5 | Searched item, container | Circle | r 10 | Search colour | As 1 with badge `B…` | Search | 11 | as 1 |
| 6 | Tracked/searched **harvest node** | Rounded square | k 9, radius 3 | Owner | Halo +2.5 (radius 4) @ 0.6; white inner stroke 2.0; badge | Owner | 11–13 | ×1.25 + focus |
| 7 | Same, **above your level** | Rounded square | k 9 | `#77716A` @ 0.9 | White stroke @ 0.5; badge kept | — | 10 | Tooltip shows "Level 151 · you 75" |
| 8 | Tracked/searched **placed object** | Filled diamond | r 12 | Owner | Halo +3.5 @ 0.6; white stroke 2.0 at r−1.4; badge 11 | Owner | 11–13 | as 1 |
| 9 | Done objective (Show done ON) | Circle | r 6.5 | `#6F6A62` @ 0.85 | Ring 1.2 ivory @ 0.6 + ✓ drawn with 2 lines (1.6) | — | 9 | ×1.25 |
| 10 | **Quest available** (Quests layer, untracked) | Token | r 8.5 | `token-fill` | Ring 1.8 `quest-avail`; "!" Georgia Bold 12.5 `quest-avail` | Layer | 8 | ×1.25 + focus; tooltip offers tracking |
| 11 | Place / POI | Diamond | r 8.5 | `#1A1510` @ 0.92 | Halo +2.4 @ 0.65; gold outline 2.2; gold centre diamond r 2.2; label plaque (§5.5) | Layer | 7 | ×1.25 |
| 12 | Player spawn | Crosshair | ring r 7 + 4 ticks (r+1.5 … r+5.5) | none | Ivory 2.0 over halo 4.6 @ 0.55; centre dot r 2 | Layer | 9 | ×1.25 + focus |
| 13 | Extraction | Token | r 11 | `token-fill` | Ring 2.6 `extraction`; up-chevron + stem, 2.3 px, `#EAF4FB` | Layer | 9 | ×1.25 + focus |
| 14 | Key spot | Token | r 8.5 | `token-fill` | Ring `key`; key glyph = ring r 2.1 + shank line + 2 teeth (1.4–1.6 px) | Layer | 6 | ×1.25 + focus |
| 15 | Locked (door/container) | Token | r 8.5 | `token-fill` | Ring `lock`; padlock = shackle ring r 2.5 at y−1.6 + body rrect 7.8×4.9 + keyhole dot | Layer | 6 | as 14 |
| 16 | Miniboss | Token | r 9 | `token-fill` | Ring 2.2 `boss`; ☠ (Segoe UI Symbol 12.5) | Layer | 6 | as 14 |
| 17 | Trap | Token | r 8 | `token-fill` | Ring `trap`; ✕ = 2 lines ±3.3, 2.1 px | Layer | 6 | as 14 |
| 18 | Sound trap: close (Z ≥ 2) | Dot + dashed ring | dot r 3.2, ring r 7.5 | `sound` | Dot halo +1.4 @ 0.5; ring 1.3 dashed @ 0.9 | Layer | 2 | ×1.25 + focus |
| 19 | Sound trap: dust (Z < 2) | Dot | r 3.2 | `sound` @ 0.8 | none | Layer | 1 | Hover still works (nearest point) |
| 20 | Placed loot (layer) | Filled diamond | r 7 | `loot` | Halo diamond +1.6 @ 0.75; centre diamond r·0.35 `#FFF6D8` @ 0.85 | Layer | 4 | ×1.25 + focus |
| 21 | Harvest node (layer) | Rounded square | k 5.5, radius 1.8 | Profession colour | Halo +1.6 @ 0.75 | Layer | 3 | ×1.25 + focus |
| 22 | Cluster (any family) | The family's marker ×1.15 + count pill | — | — | Pill: `ink-950` @ 0.95, 1.2 px edge in the owner/layer colour, Semibold 11 `text-100`, top-right at (x+6, y−20), height 15 | — | Same as members | Tooltip lists members |

The **z-order** is the Z column, bottom to top. **Hovered** marker and **tooltip** are always last.

**Dropped:** the "same circle for every container" ambiguity. Tracked objectives, ingredients and searched items now share the circle shape on purpose (they are all "go loot this container"), and are told apart by colour and badge.

**Hover (all types):** scale ×1.25, raise to the top, and add the focus ring (`halo` 4.4 @ 0.55 under `gold-hi` 2.2, at r + 4.5). Nothing else changes, so the marker stays recognizable. The hit radius is `max(r·1.25, 12) + 4`.

### 5.3 Zoom metric

Use `Z = sq.side / (0.9486 · client_h)`, so the game's default (fully zoomed-out) map is Z = 1. The S1 screenshot is Z ≈ 2.5. Marker sizes do **not** scale with Z (constant screen size, as today). Only density rules use Z:

| Z | Mode | Rules |
|---|---|---|
| < 1.5 | Overview | Cluster radius 24; sound traps in dust mode; no place labels unless the place is a tracked objective; count pills shown |
| 1.5 – 3 | Area | Cluster radius 16; sound traps in close mode from Z ≥ 2; place labels when Places is ON |
| ≥ 3 | Close | Clustering only for exact overlaps (< 8 px); labels for tracked `map_coords` spots ("Search Sarlat") |

### 5.4 Declutter

1. **Per-owner / per-layer clustering** (screen space, recomputed only when the map transform or the set of points changes). Use greedy centroid clustering in z-order: a point joins the first cluster whose centroid is within the cluster radius. A cluster of n ≥ 2 is drawn as its family marker ×1.15 plus the count pill. Same-position duplicates of one owner (wine and mead in the same barrel) are **deduplicated before** clustering: one marker per spot per owner, and the tooltip lists both objectives.
2. **Cross-owner petals.** If tracked markers or clusters of **different owners** end up within 22 px of each other, keep a small ivory dot (r 2, halo r 3.2) at the true centroid. Place the markers on a circle of R = 14 (2 owners, left/right) or 18 (3 or more, evenly spaced from the top), each tied to the dot by a leader line (1.2 ivory over 2.6 halo @ 0.55). Never let a tracked marker hide another owner's marker.
3. **Layer vs tracked.** If a layer marker is within 8 px of a tracked marker, skip it (the tracked one wins, and its tooltip mentions the layer fact, e.g. "Locked · Bronze Key").
4. **Dense layers.** Any layer with more than 120 visible points at Z < 2 switches to dust mode (sound traps today).
5. **Spotlight** (§7.3) is the per-owner declutter tool: it dims everything that is not the hovered owner.

### 5.5 Labels

- **What:** place names (Places layer ON), and tracked `map_coords` spots at Z ≥ 3. Containers and layer points never get labels.
- **Look:** a plaque (`#15110C` @ 0.82, `line-gold` @ 0.30 edge, radius 4, padding 6×2, height 20) with Segoe UI Semibold 12.5 `text-100`.
- **Placement** (greedy, in priority order: tracked spots, then extractions, then places):
  - Try right (x + r + 6), then left, then above, then below (centred).
  - Accept the first slot that does not intersect the panel rect, another label (+4/+3 px margin), any token or tracked marker footprint (r + 3), or the client edge (4 px).
  - Otherwise **drop** the label; the name stays available in the tooltip.
- **Duplicates:** the pack has some places twice ("River Watch"); deduplicate by name per map.

---

## 6. Tooltip

**Layout.** Max width 320.
- **Body:** `ink-950` @ 0.95 rrect, radius 8, 1 px `line-gold` @ 0.38, plus a shadow rrect (+2, +3, black 0.35). Padding is 12 horizontal and 10 vertical, plus a **3 px accent bar** inside the left edge (inset 4 px, radius 1.5) in the owner or layer colour. This replaces the full coloured border, which looked like an error state on red quests.
- **Rows, top to bottom:**
  1. **Eyebrow** (caps 11 Semibold, tracking 1.1, `text-300`): `<badge> · <owner title> · <source kind>`.
  2. **Title** (15.5 Semibold `text-100`): what you are looking at, such as the container, node, place or quest name.
  3. **Lines** (13.5): `key` in `text-300`, then the value in `text-100`. At most 4.
  4. Optional **chance bar**: "Chance", a bar 7 px high (track white @ 0.10, fill in the owner colour, radius 3.5) and the value right-aligned in Semibold.
  5. Optional **footer** under a hairline: a keycap (`ink-700`, 1 px `text-500` edge, "Ctrl" Semibold 11.5) followed by a `gold-hi` hint.

**Placement.** By default the box starts 14 px right of the marker edge, with its top 24 px above the marker centre. Flip to the left if it would cross the client edge **or the panel rect**. Clamp vertically to 8 px from the client edges. Never cover the hovered marker.

**Content per marker type:**

| Marker | Eyebrow | Title | Lines | Bar | Footer |
|---|---|---|---|---|---|
| Tracked container | `1 · MAKING DOUGH · CONTAINER` | Container ("Flour Sack") | Objective label(s); if ≥ 2 owners share the spot, one line each | Chance | — |
| Tracked `map_coords` spot | `1 · QUEST · OBJECTIVE` | Objective ("Locate the Gunline Fort.") | Giver; "Also: …" if a place is here | — | — |
| Harvest node (tracked, search or layer) | `B · IRON ORE · HARVEST` / `HARVEST` | Node ("Iron Ore") | `Profession` Scavenger · `Level` 151 (`you` 75, in `warn` when above) · `Yield` 3–7 | Chance | — |
| Placed object (tracked or layer) | `… · PLACED` / `PLACED LOOT` | Item | "Always at this spot" | Chance (if < 100 %) | — |
| Quest available | `QUEST · NOT TRACKED` | Quest title | Objective(s) here (max 2, then "+N"), `Giver` X, `On this map` N objectives | — | `[Ctrl] + click to track this quest` |
| Cluster | `1 · MAKING DOUGH · 5 CONTAINERS` | "5 containers" | Up to 4 members as `Container` + `chance %` (best first), then "+N more" | — | "Zoom in to separate" |
| Place | `PLACE` | Name | Quests with objectives here (dim) | — | — |
| Extraction | `EXTRACTION` | Name ("South River Dirigible") | Type, if the pack has one | — | — |
| Key / Lock | `KEY SPOT` / `LOCKED` | "Gold Key" / "Bronze Key door" | For a lock: "Key spot: N on this map" | — | — |
| Trap / Sound trap / Miniboss / Spawn | Layer name | Kind ("Explosive", "Bleed"…) | — | — | — |
| Done objective | `1 · MAKING DOUGH · DONE` | Objective | — | — | — |

Several markers under the cursor (within the hit radius) produce one tooltip with stacked sections (hairline between them), at most 3, then "+N more here".

---

## 7. Interactions

### 7.1 Tracking a quest from the map (Quests layer)

The recommended UX has two paths that do the same thing.

**1. Panel (primary, always reliable).** When the Quests layer is ON, the panel lists "AVAILABLE ON THIS MAP" with `[+ Track]` buttons. The panel already receives clicks without stealing focus.
- Hovering a row spotlights that quest's "!" tokens on the map: they pulse (ring r+4 → r+10, alpha 0.9 → 0, over 900 ms, twice), and everything else dims to 35 %.
- `[+ Track]` makes the tracking call, shows the toast, and moves the quest into the tracked list with the next number and a `NEW` tag for 10 s. The quest's tokens become tracked markers.

**2. Map shortcut: Ctrl + click on a "!" token (passive).**
- **Detection:**
  - Register raw mouse input (`RAWINPUTDEVICE{usUsagePage=1, usUsage=2, dwFlags=RIDEV_INPUTSINK}`) on the overlay's message window. Raw input never blocks, so the game still gets the click; on the map screen a left click only starts a pan.
  - On button-down with `GetAsyncKeyState(VK_CONTROL) < 0`, remember the cursor position.
  - On button-up, accept only if the cursor moved less than 6 px (not a pan), the overlay is in `MapState::Open` and not `uncertain`, the hovered marker is a Quests-layer token (or a cluster of them, in which case use the first quest, or the one chosen with the wheel; see the next point), and the cursor is not over the panel.
- **Ambiguity:** if the token groups 2 or more quests, the tooltip lists them numbered, and Ctrl+click tracks the **first** one. For the others, use the panel; the tooltip footer then says "Ctrl + click to track Signs Of Life · others in the panel".
- **Feedback:**
  - Immediately (optimistic): the token turns into a tracked marker with a pulse, and the toast shows "Now tracking … Undo".
  - On server failure: revert, and show a warn toast "Couldn't track (offline)".
- **Configurable modifier:** Ctrl (default), Alt, or Middle-click, under Settings › Map. Ctrl is the least likely to conflict on the map screen.
- **Untracking is never done from the map**, so a stray click cannot lose progress. Use the panel (hide) or the website. `Undo` in the toast is the safety net for a mistaken track.

A hotkey-while-hovering approach (`RegisterHotKey`) was rejected: it would steal the key from the game. A clickable map marker was also rejected: it would make the overlay window non-transparent to input.

Dependency: tracking needs a write endpoint on the website tracker (same API key). Until it exists, `[+ Track]` opens the quest page in the browser (`quest.url`) and the toast says "Opened on hunger-companion.com".

### 7.2 Panel

| Target | Action |
|---|---|
| Header (outside the buttons) | Drag to move; double-click to reset to the default position |
| `⌄` button | Collapse / expand the whole panel |
| `⚙` button | Settings window |
| Section chevron / title | Collapse / expand that section (remembered per section) |
| Owner row | Hide / show its markers (hidden = dim row + `ED1A` icon) |
| Owner row ✕ (searches) | Remove the search |
| `Other map: ② Still Alive` | Expand that quest's objectives inline |
| Warning action chip | Lowers the chance filter to the revealing step |
| Layer pill | Toggle the layer |
| Group label | Toggle the whole group |
| Search field | Opens the Search window (focus in its text box) |
| Toast `Undo` | Reverts the last track |

All clickable zones get the hover fill (`#FFFFFF` @ 0.07, or a gold edge for chips). The cursor stays the arrow because the window is non-activating.

### 7.3 Spotlight (panel → map)

Hovering an owner row (quest, recipe or search), a layer pill or an "available" row for 150 ms turns spotlight on:
- That owner's or layer's markers draw at full alpha, ignore clustering (they show individually, Z ≥ 1.5), and get a soft pulse once.
- Every other overlay marker is drawn at 35 % alpha.
- Spotlight ends 200 ms after the cursor leaves the row.

Spotlight is the best answer to "where exactly are this quest's points" on a busy map.

### 7.4 Keyboard

None by default: global keys would conflict with the game. The existing map-detection key stays as is.

---

## 8. Implementation checklist (mapping to the code)

- `render.rs`: add the `Display`, `Caps` and `Icon` font roles (and `Font::Small` 12.5 / `Text` 14 / `Bold` 15 base sizes). Add `fill_diamond()` (path geometry) and `text_spaced()` (character spacing).
- `panel.rs`:
  - Replace the chips row with the search field and the quick row.
  - Add a `section()` helper with collapse state (`cfg.sections_collapsed: Vec<String>`).
  - Add owner badges, the other-map one-liner, the warning callout with `Action::RevealFilter(u32)`, layer pills in groups (`Action::ToggleGroup(String)`), the available-quests list (`Action::Track(String)`, `Action::ShowMoreAvailable`), the toast (`Action::Undo`), and overflow scrolling (`scroll_y`, wheel handler).
- `main.rs::draw`:
  - Draw markers per §5.2 in the z-order of the table.
  - Run dedup, then clustering, then petals once per frame where the transform changed.
  - Add the label placement pass, the new tooltip layout (§6) and the spotlight alpha.
- `model.rs`: assign owner badges (quests in tracking order 1–9; recipes and searches A–Z), and build the **Quests layer** from the pack:
  - **Include** every untracked quest's objectives of kind `map_coords` (discover / inspect), plus `unresolved` objectives with ≤ 4 markers.
  - **Exclude** the `loot_sources` pools: on Map01 that is 2 703 generic containers versus 25 real quest spots. They would turn the layer into noise.
  - The layer's count is the number of spots; the panel sub-list counts quests.
- Raw-input Ctrl+click handler (§7.1) and the tracker write call (`net.rs`).

Real numbers for Map01 (cached pack 5.6-44973): 25 quest spots for 17 untracked quests; 23 extractions; 18 spawns; 289 sound traps; 68 placed loot; 5 key spots; 56 locks; 24 minibosses; 21 traps; 31 places (2 duplicate names).
