# Hunger Overlay: quest progression and allies design spec

Status: implemented (2026-10-15), against the website contract v1.2 (`PC_RESPONSE_OVERLAY_12` / `SITE_RESPONSE_OVERLAY_11`). It extends `docs/DESIGN.md` and keeps everything in it. The tokens, typography, panel frame, marker grammar, tooltip and spotlight stay as they are. Scope: the player's quest progression (states, cascades, undo), the new **Quest journal** window, the panel changes, and **allies** (on the map and in the panel).

Mockups (rendered by `docs/design/mockup_quests.py` with real titles, chains, prerequisites and Map01 marker positions; the player and ally states are invented but follow the model in §2):

| File | Shows |
|---|---|
| `docs/design/journal.jpg` | The Quest journal: subject tabs, UP NEXT, character cards with steppers, an open "I am here" confirm strip on Louis, the history footer. Also the Search window's quest results with their state, and the legend of the step glyphs. |
| `docs/design/allies_map.jpg` | The default map with 3 of my quests (filled) and 2 allies (hollow, with an initial): shared-spot pips, petals, a tooltip on Lou's "Search Sarlat." spot. The panel shows the ✓ buttons, UP NEXT and ALLIES. |
| `docs/design/panel_allies.jpg` | Panel states: (A) the toast after ✓ Done, with the next quest; (B) a hidden ally and folded ally quests; (C) no ally yet; (D) the collapsed summary; (E) the journal's "+ Ally" tab. |
| `docs/design/ally_markers.jpg` | Mine vs ally for each shape at 2×: cluster, hover, pip, petals, the difference from the Quests-layer token, spotlight. |

All sizes are **px at 1440p × `sc`**, as in DESIGN.md. The journal opens over the game, so it uses the same `sc` as the panel.

---

## 1. The real data (what the model must handle)

Sources: `HungerDB/export/quests/quests.json` (97 quests), `quest_chains.json` (7 ordered lines), `quest_preconditions.json` (prerequisites).

- **50 story quests** (`kind: standard`) and **47 repeatable quests**: 32 daily, 10 weekly and 5 monthly. The public pack has 95 quests: Hector's 2 are not in it.
- **6 characters with lines.** Eloise has 2 lines (8 + 2 quests). Louis has 10, Mr Sweet 9, Piro 9 and Reynauld 9. Hector has 2 and is not in the pack.
- **The play order is not the ID order.** For example, Eloise's line is Q01 → Q04 → Q05 → Q02 → Q06…, so the order always comes from the line.
- **Cross-character gates.** Louis, Piro and Mr Sweet start after Eloise · A Warm Welcome. Reynauld starts after Eloise · Making Dough. Eloise line 2 starts after Mr Sweet · *A Touch Of Bribery* (Q101). Q101 is in **no line**: it is a standalone story quest that needs Eloise · *Pushing Back The Shadows*.
- **Repeatables are gated by one story quest.** For example, every Eloise repeatable needs Making Dough. Mr Sweet · *Sweet Nothings* needs nothing. No quest requires a repeatable.
- **Data mismatch:** `quest_chains.json` says the Mr Sweet and Hector lines need nothing to start. `quest_preconditions.json` says they need Eloise Q01 and Eloise Q10. **The model uses the preconditions** (the `requires` list is the truth) and uses the lines **only for order**.
- **Characters without a chain** (the website may have some, such as Bear) are supported: their quests are listed in website order, with no stepper and no cascade.

---

## 2. Quest model

### 2.1 Stored and derived state

For each **subject** (me, or a manual ally), the app keeps only two things:
- `D`, the set of done quests;
- `T`, the ordered list of tracked quests, each with its badge slot.

For **me**, the website owns both and the app mirrors them. For a **manual ally**, they live on this PC only.

The state of a quest is **derived**:

| State | Rule | Glyph (journal, stepper) |
|---|---|---|
| **done** | in `D` | gold disc `GOLD` r 7 with a dark ✓ (two lines, 1.7) |
| **in progress** | in `T` | its owner badge (colour + number, white ring) |
| **available** | every `requires` is in `D` | hollow ring r 6.2, 1.7 px, `quest-avail` |
| **locked** | otherwise | dot r 2.6, `text-500` @ 0.7 |

**Invariant:** every ancestor (transitive `requires`) of a quest in `D ∪ T` is in `D`, and `D ∩ T = ∅`. The app never writes a state that breaks it. Data read from the website that breaks it (old accounts) is shown as it is, and the journal flags the quest with a `warn` dot ("Earlier quests not marked done", with an action chip `[Fix]` that opens the usual cascade preview).

### 2.2 Operations and cascades

| Action | Effect | Cascade |
|---|---|---|
| **Track** | `T += q` | **Up:** the ancestors of `q` not yet in `D` go to `D`, and any of them that was tracked leaves `T`. |
| **Untrack** | `T -= q` | None. |
| **Done** | `D += q`, `T -= q` | **Up:** as Track. |
| **Reopen** (done → not done) | `D -= q` | **Down:** every descendant of `q` leaves `D` and `T`. This applies across characters too (reopening Eloise · A Warm Welcome reopens Louis, Piro and Mr Sweet). |
| **I am here** = *Track here* (from a stepper dot or a locked row) | `q` is tracked, and every quest before it is done | Up as Track, **plus down:** descendants of `q` that are done or tracked are reset. This is "set where I am": before = done, this = in progress, after = not started. |
| **Done up to here** | Done on `q` | Up only. Descendants are untouched. |
| **Finished** (repeatables only) | `T -= q` | None. Repeatables have no done state (see open question 1). |

The cascades follow directly from the user's rules: "on a quest ⇒ everything before is done", and "No, I already did it ⇒ it and its prerequisites are done".

### 2.3 Confirmation rule (one rule everywhere)

- An action that changes **only the clicked quest** runs immediately. A toast appears with **Undo**.
- An action that changes **any other quest** first opens an inline **confirm strip** that lists the changes: up to 4 names, then "+N". It is never a modal dialog. The strip's primary button commits the change.

**Cascades never happen outside the journal.** The panel's `[+ Track]` and the Ctrl+click on the map only offer **available** quests (no ancestor is missing). The panel's ✓ only applies to **tracked** quests, whose ancestors are already done by the invariant. In-game actions are therefore always a single change and always undoable from the toast.

### 2.4 Undo (exact, even for cascades)

Every committed action is **one change record**:

```
{ id, subject, label, at, base_rev,
  ops: [ { quest_id, from: {state, slot}, to: {state, slot} } … ] }
```

`state` is `done`, `tracked` or `none`. `slot` is the badge number, so an undone untrack gets its old number and colour back.

- **Undo** applies the inverse ops (`to → from`) of that record **in one atomic write**. Only the quests in the record are touched.
- **Conflict check:** before undoing, the app compares each op quest's **current** state with `to`. If any differs (for example, it was changed on the website meanwhile), a strip shows "Changed on the website since: Making Dough. [Undo anyway] [Keep]". "Undo anyway" still only writes `from` states.
- **History:** the last 20 changes per subject, kept for 7 days in `%APPDATA%\HungerOverlay\history.json`, so "I tracked it just to look for a friend" can be reverted even after a restart.
  - The journal footer shows the last change with `[Undo]` and a `[History ⌄]` list.
  - Each entry in the list has "Undo back to here". That undoes the newer changes too, newest first, as one previewed batch.
- **Better path for the "for a friend" case:** add the quest to that friend as a manual ally (§6). It never touches the player's own progression.

### 2.5 Website sync

For subject *me*, the website is the reference.
- The app sends the **explicit op list** of a change. It is computed and previewed in the app, and the website stores exactly that, with no cascade on the server. This is what makes undo exact.
- Writes are optimistic: the UI changes at once. On failure, the UI reverts and shows a warn toast:
  - "Couldn't save on the website (offline)";
  - 403: a read-only key (existing message).
- A 409 (the account changed since `base_rev`) re-reads the tracker, re-computes the preview and asks again.
- Writes count toward `MAX_PER_HOUR`. No new polling.

Manual allies never touch the network.

### 2.6 "Up next" (the suggestions)

For each character, in the pack's giver order (Eloise, Louis, Mr Sweet, Piro, Reynauld, then any others), walk its **story** quests:
1. line 1 in order;
2. then line 2;
3. then standalone story quests (bridges such as *A Touch Of Bribery*).

Take the **first quest not in `D`**:
- **tracked**: an "in progress" row with `[✓ Done]`;
- **available**: `[+ Track]` and `[✓ Already done]`;
- **locked by another character's quest**: a dim row "waiting · needs Eloise · Pushing Back The Shadows", with `[✓ Already done]`, which opens a cascade preview;
- **none left**: the character shows "All done" and drops out of the list.

**"Already done"** on an available quest runs immediately. The row is replaced in place by that character's next quest, which is highlighted for 600 ms (`gold` @ 0.15 fill fading out). A toast says "Gardon's Gambit done · Undo". This is the "no, I already did this one → propose the next" loop.

Repeatables are not part of UP NEXT. One summary line counts the available ones ("13 repeatable quests available · Show").

---

## 3. Quest journal window

`journal.jpg`. A new top-level window (`journal.rs`), built like the Search window: topmost, native title bar in DWM dark mode, takes focus, and Esc closes it. It is drawn with the **panel tokens** (ink, gold hairlines, Palatino caps) because it lives over the game.
- Size: 800 × min(1150, screen − 80), resizable in height. The body scrolls; the tabs and footer stay fixed.
- It remembers the last tab and the expanded cards.

**It opens from:**
- the panel's QUESTS header link "Journal" (`E82D`);
- the tray menu "Quest journal…";
- the panel's ALLIES "Manage" / "Add" (opens on the "+ Ally" tab);
- Search results for done or locked quests (opens at that quest);
- the panel's UP NEXT footer link.

### 3.1 Layout

```
┌ Quest journal — Hunger Companion ──────────────────────────── – ▢ ✕ ┐
│ [K Kamille you] [◯M Marius ☁] [◯L Lou ▭] [⇪+ Ally]          ● synced │  subject tabs (36)
│ [⌕ Filter quests…                               ] [☐ Repeatables]   │  (34)
│─────────────────────────────────────────────────────────────────────│
│ UP NEXT   first quest to do for each character                      │
│ Eloise    ① Last Embers 4/8          in progress           [✓ Done] │  rows 36
│ Louis     ○ Gardon's Gambit 2/10      [+ Track] [✓ Already done]    │
│ Mr Sweet  ③ A Long Boy 2/9           in progress           [✓ Done] │
│ Piro      ○ Remains Of The Day 1/9    [+ Track] [✓ Already done]    │
│ Reynauld  ② Bearing Arms 2/9         in progress           [✓ Done] │
│           ⟳ 13 repeatable quests available · Show                   │
│─────────────────────────────────────────────────────────────────────│
│ CHARACTERS                                click a step: "I am here" │
│ ⌄ ELOISE   ✓─✓─✓─①─·─·─·─·                              3 / 8 done │  card header 40
│     ✓ The Gavroche Family  done 3 min ago               [⟲ Reopen]  │  rows 28 (actions on hover,
│     ① Last Embers  tracked                      [Untrack] [✓ Done]  │   always for the tracked row)
│     · Mapping The Front  after Last Embers                          │
│     🔒 Line 2 starts after Mr Sweet · A Touch Of Bribery (…)         │
│     ⟳ Repeatable: 7 daily · 4 weekly · 1 monthly — available · Show │
│ › LOUIS    ✓─○─·─◉─·─·─·─·─·─·                              1 / 10 │
│   ┌ A Gift For The Giver  Louis · step 4 of 10 ───────────────────┐ │  confirm strip
│   │ "I am here" also marks 2 earlier quests done: Gardon's Gambit, │ │
│   │ A Friend In Need.  [+ Track here] [✓ Done up to here] [Cancel] │ │
│   └────────────────────────────────────────────────────────────────┘ │
│ › MR SWEET … › PIRO … › REYNAULD …                                  │
│─────────────────────────────────────────────────────────────────────│
│ ⟲ Last change: The Gavroche Family done · 3 min ago  [↶ Undo] [History ⌄] │  footer 52
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 Elements

- **Subject tabs (36 high, radius 6).**
  - Selected tab: `ink-700` with a `gold` @ 0.7 edge. Other tabs: `ink-800`.
  - *Me* shows a gold disc with the account initial, the name, and "you".
  - Each ally tab shows the hollow ally disc (§5), the name, and its source icon: `E753` for site, `E7F8` for local.
  - "+ Ally" is a ghost chip (`E8FA`). It is hidden once 2 allies are set; the ally tabs' "Remove from raid" frees the place.
  - On the right: the sync dot (DESIGN.md §4.3 colours) and "synced".
- **Filter field.** Same look as the panel's search field, but it is a real EDIT control (this window can take focus). It filters by title with accent folding (`search::fold`), across characters, and includes repeatables. Matching cards expand, and other rows dim to 35 %. The **Repeatables** checkbox chip lists repeatables inside each card (default off).
- **UP NEXT (§2.6).**
  - Columns: giver name (`text-300` 13, width 104), state glyph, title (Semibold when in progress), step "4/8" (`text-500` 12.5), and actions right-aligned.
  - Buttons are 26 high, radius 5. `[+ Track]` is the ghost style; `[✓ Done]` and `[✓ Already done]` are the normal style.
  - Hovering a row spotlights that quest on the map if the map is open (DESIGN.md §7.3).
- **Character card.**
  - **Header (40):** chevron, name in caps (Display 12.5 `text-100`, tracking 1.5), the **stepper** of line 1 (26 px pitch, glyphs from §2.1, connectors 1.4 `gold` @ 0.6 between done steps and 1.0 `text-500` @ 0.35 elsewhere), and "n / N done" on the right. Clicking the name or chevron expands the card. Clicking a dot opens the confirm strip for that quest; the selected dot gets the focus ring.
  - **Expanded rows (28):** glyph, title (`text-300` done / `text-100` tracked or available / `text-500` locked), a dim note (`done 3 min ago`, `tracked`, `after Last Embers`), and the actions in the table below. Long lines show 6 rows around the current step, then "+ N more".
  - **Line separators:** a lock icon `E72E` with "Line 2 starts after Mr Sweet · A Touch Of Bribery", plus that quest's own requirement in dim text.
  - **Standalone story quests** (bridges) are listed under their giver as "side step · needs …".
  - The **repeatables** line expands into rows with `[+ Track]` / `[Finished]`.
  - A character with no chain has no stepper and the hint "No known order".
- **Confirm strip.** Inset 10 under its card. `gold` @ 0.07 fill, `gold` @ 0.45 edge, radius 6, with a 7 px pointer to the dot.
  - Line 1: title (Semibold 14.5) and "Louis · step 4 of 10".
  - Line 2: the cascade sentence, with the names in Semibold.
  - Line 3: buttons, with the primary one first.
  - Right side: "Undo stays available" (`text-500` 12).
  - Esc or Cancel closes the strip. Only one strip is open at a time.
- **Footer (52, fixed).** `E81C` icon, "Last change: <label> · <age>", then `[↶ Undo]` (ghost) and `[History ⌄]`. History is a popover (same look as the panel's language list) with the last 20 changes. Each entry has a label, an age and "Undo back to here".

### 3.3 Row actions per state

| State | Me / manual ally | Repeatable |
|---|---|---|
| done | `[⟲ Reopen]`, with a preview when descendants are done or tracked | not shown |
| in progress | `[✓ Done]` `[Untrack]` | `[Finished]` |
| available | `[+ Track]` `[✓ Done]` | `[+ Track]` |
| locked | `[+ Track here]` `[✓ Done up to here]`, both previewed | `[+ Track]`, previewed (it marks the gate done) |

A website ally's tab is **read-only**: no actions on rows. The header row offers `[Hide on map]` and `[Remove from raid]`, and a dim note says "Marius updates their quests on hunger-companion.com".

### 3.4 The Search window's quest results

The Search window keeps its look. Quest rows replace the emoji with the state glyph of §2.1 and show `<giver> · <state>`.

| State | Second line | Right action (Enter / click) |
|---|---|---|
| available | "Available · 40 spots here" | **Track** (direct, toast + Undo), as today |
| in progress | "Tracked ①" | **Tracked** (closes the window and spotlights it) |
| locked | "Locked · 2 before" / "Locked · repeatable, after …" | **Open in journal**: opens the journal at that quest with its confirm strip already open |
| done | "Done" | **Open in journal**: opens at the row, where Reopen is offered |

Search never cascades on its own.

---

## 4. Panel changes

`allies_map.jpg` (right), `panel_allies.jpg`. New section order: header, toast, search field, quick settings, **QUESTS** (with UP NEXT inside), **ALLIES**, CRAFTING, SEARCH, MAP LAYERS.

1. **QUESTS header:** a gold link "Journal" with `E82D` on the right (Semibold 12.5).
2. **✓ on owner rows** (quests only). A 22×22 ghost button at the far right: 1 px white @ 0.10 edge, `E73E` in `text-300`; on hover `ink-700` fill, gold edge and `gold-hi` icon. The "N spots" count moves left of it.
   - Click = **Done**. For a repeatable it is **Finished**.
   - It never cascades (§2.3).
   - It shows the **next-quest toast** (item 4).
3. **"AVAILABLE ON THIS MAP" becomes "UP NEXT".** It is the same sub-list, now state-aware and always shown when it is not empty (it no longer needs the Quests layer). Sub-header: "UP NEXT n" with the hint "first per character" on the right.
   - Rows (30): a mini "!" token when the quest has spots on this map, otherwise a hollow ring (r 5, 1.4, `quest-avail` @ 0.8); the title; "giver · step"; `[+ Track]`.
   - Order: the available UP NEXT quest of each character, then (Quests layer ON) other available quests with spots on this map, by giver, then title.
   - Show 3 rows, then "Show N more". The footer link is "13 repeatable · Journal".
   - The Quests layer's "!" tokens now show **available quests only**: locked and done quests drop out. Today's panel offers *Down From Mount Sinai*, which is 9 steps away.
4. **Toast with a suggestion** (56 high, shown 10 s). Line 1: "✓ **Last Embers** done" and `Undo`. Line 2: "Next: Mapping The Front" and a `[+ Track]` ghost button. When the character has nothing available next, it falls back to the one-line toast.
5. **ALLIES section** (collapsible). The header count is the number of allies; the gold link on the right is "Manage" (`E716`), or "Add" (`E8FA`) when empty.
   - **Ally row (30):** the hollow ally disc (r 9.5, §5) with its initial, the name in Semibold 15, the source tag (icon `E753`/`E7F8` + "SITE"/"LOCAL", caps 10 Semibold `text-500`), and "N spots" on the right. Click = hide/show all their markers (a hidden ally is dimmed to 40 % with the `ED1A` icon, as hidden quests are). Hover = spotlight that ally.
   - **Ally quest row (24):** a ring glyph (r 4.2, 1.8, ally colour), the title (14 `text-200`), "also ①" when I track it too (my badge at 0.75), and "N spots". Click = hide/show that quest; hover = spotlight it. Manual allies get a 22×20 ✕ to remove the quest. Objectives are not listed: they are in the tooltip.
   - **Empty:** a dim "+" disc and "Nobody yet. Pick a raid mate from your hub, or add someone who has no app."
   - **Overflow** (DESIGN.md §4.5): at level 3, each ally's quests fold into one dim line ("2 quests: Signs Of Life, Mapping The Front"). At level 4, the section folds to its header, with the summary "Marius, Lou".
6. **Collapsed summary:** "3 quests · 2 allies · 1 layer". The allies part is omitted when there are none.

### 4.1 Picking allies (journal, "+ Ally" tab; `panel_allies.jpg` E)

The tab has two blocks and a counter on the right: "1 of 2".

1. **From your hub on hunger-companion.com.** The list is fetched when the tab opens (an explicit user action, one call).
   - Each row shows the name, the relation ("Friend", "Follows you", "You follow"), and "N quests".
   - The button is `[+ Add]`, or `[✓ Added]` once added. When 2 allies are already set, `Add` is disabled with the hint "Remove one first".
2. **Someone without the app or an account.** A name field (max 24 characters) and `[+ Add]` (primary).
   - This creates a **manual ally**, stored in `%APPDATA%\HungerOverlay\allies.json`, and switches to their tab.
   - That tab works exactly like *Me* (UP NEXT, steppers, cascades, undo), but it is local. The player asks the friend which quest they are on and clicks "I am here" on it.

There are at most 2 allies in total (raids hold 3 players). The slot (A1/A2) and colour are given in adding order and kept.

---

## 5. Allies on the map

`allies_map.jpg`, `ally_markers.jpg`.

**Grammar addition** (DESIGN.md §1.2): **filled = mine, hollow = an ally's.** A **white hairline ring** means "tracked by someone in the raid", which keeps ally markers apart from information tokens (tokens have no white ring). The shape keeps its meaning: circle = container or spot, square = harvest node, diamond = placed object.

| Kind | Look | Size |
|---|---|---|
| `Ally` (container / spot) | halo +2.6 @ 0.6; `token-fill` @ 0.94 disc; ring 2.2 in ally colour at r − 1.1; white ring 1.1 @ 0.85 at r + 0.3; initial in Segoe UI Bold 10.5, ally colour | r **9** (mine: 10) |
| `AllyNode` | the same as a rounded square (radius 3) | k 8 |
| `AllyPlaced` | the same as a diamond | r 10.5 |
| Ally cluster | the ally marker ×1.15 + count pill with an ally-colour edge. Clustering is **per ally** (all their quests together), so 2 allies add at most 2 marker sets. | |
| **Pip** (same spot **and** same quest as one of my markers) | No ally marker is drawn. A satellite sits on **my** marker: r 6.2 at 45° (second ally at 135°), centre at r_owner + 1.5; halo 7.6 @ 0.7, `token-fill`, ring 1.6 in ally colour, initial Bold 7.5. | |

- **Ally colours** are fixed and never come from the website: **A1 mint `#5FE0B5`**, **A2 orchid `#E58CF2`**. Because ally markers are always hollow, a same-hue owner of mine (filled) still reads as different, and the initial carries identity for colour-blind players.
- **Initial:** the first letter of the name. If both allies share it, both use two letters (Ma / Mi) at 9.5 px.
- **Z-order:** ally markers are drawn after `Done` (9) and before every marker of mine (in code: `Ally*` = 10, and today's 10–13 move up by one). The player's own markers stay on top.
- **Petals** (DESIGN.md §5.4.2): ally markers and clusters take part as owners. Order around the centroid: my owners by badge first (left, or from the top), then A1, then A2. A tracked marker of mine is never hidden by an ally's.
- **Layers:** the same rule as tracked markers. A layer marker within 8 px of an ally marker is skipped. A Quests-layer "!" token of the same quest within 8 px of an ally marker is skipped too (the ally's tooltip says "Available for you too").
- **Done objectives** of an ally are not drawn.
- **Spotlight:**
  - Hovering an ally row shows all of that ally's markers at full alpha; their pips are **unfolded into full ally markers** so every spot shows. Everything else is drawn at 35 % alpha, and the markers pulse once in the ally colour.
  - Hovering an ally quest row does the same, limited to that quest.
  - Hovering one of my rows dims ally markers like any other owner.
- **Tooltip** (DESIGN.md §6): the accent bar uses the ally colour.
  - Eyebrow: `M · MARIUS · ALLY · CONTAINER`.
  - Title: the container or spot.
  - Lines: `Quest` and the title; the objective; for a manual ally, `Added by you` · `local ally`. Chance bar as usual.
  - On one of my markers that carries pips, one extra line: `Also for` · `Marius (M)`.

---

## 6. Data the app needs from the website

**Pack** (static, ETag; no new call):
1. Per quest: `requires: [quest_id]` (from `quest_preconditions.json`), and `kind` (`story` | `daily` | `weekly` | `monthly`).
2. The `lines` (from `quest_chains.json`: `id`, `giver`, `order`) and `givers` (`id`, localised display name, order). The two mismatches of §1 should be fixed at the source.

**Tracker** (`GET /me/tracker`, unchanged budget: opened map, ≥ 30 s apart):

3. `done: [quest_id]`: the account's done story quests.
4. `tracked[]` as today, plus a fixed `color` and `tracked_at` (already asked in PC_RESPONSE_OVERLAY_11).
5. `allies: [{ user_id, name, relation, tracked: [{ quest_id, objectives: [{objective_key, state}] }], done?: [quest_id] }]`, only for the ≤ 2 selected allies. `rev`/ETag must change when an ally's tracker changes, so 304 still works.

**On demand:**

6. `GET /me/hub` → `[{ user_id, name, relation: friend|follower|following, n_tracked }]`. It is called only when the "+ Ally" tab opens.

**Writes** (explicit user actions only, same key, counted in the hourly cap):

7. `POST /me/quests` `{ base_rev, ops: [{ quest_id, state: "done"|"tracked"|"none" }] }`.
   - It is atomic, and the website stores exactly the ops it receives (no cascade on the server).
   - It returns the tracker.
   - It returns **409** with the current tracker when an op quest changed since `base_rev`.
   - `/me/track` stays for compatibility.
8. `PUT /me/allies` `{ user_ids: [≤ 2] }`: the raid selection (see open question 4).

---

## 7. UI strings (EN / FR)

| Key | EN | FR |
|---|---|---|
| `menu_journal` | Quest journal… | Journal de quêtes… |
| `j_title` | Quest journal | Journal de quêtes |
| `j_you` | you | vous |
| `j_add_ally` | Ally | Allié |
| `j_synced` | synced | synchronisé |
| `j_filter` | Filter quests… | Filtrer les quêtes… |
| `j_repeatables` | Repeatables | Répétables |
| `j_up_next` / `p_up_next` | UP NEXT | À SUIVRE |
| `j_up_next_hint` | first quest to do for each character | première quête à faire pour chaque personnage |
| `p_up_next_hint` | first per character | la première par personnage |
| `j_in_progress` | in progress | en cours |
| `j_waiting` | waiting · needs {giver} · {quest} | en attente · requiert {giver} · {quest} |
| `j_all_done` | All done | Tout est fait |
| `a_track` | Track | Suivre |
| `a_untrack` | Untrack | Ne plus suivre |
| `a_done` | Done | Faite |
| `a_already_done` | Already done | Déjà faite |
| `a_reopen` | Reopen | Rouvrir |
| `a_finished` | Finished | Terminée |
| `a_track_here` | Track here | Suivre ici |
| `a_done_up_to` | Done up to here | Faites jusqu'ici |
| `a_fix` | Fix | Corriger |
| `j_characters` | CHARACTERS | PERSONNAGES |
| `j_click_step` | click a step: "I am here" | cliquez une étape : « J'en suis là » |
| `j_n_done` | {n} / {t} done | {n} / {t} faites |
| `j_step` | {giver} · step {n} of {t} | {giver} · étape {n} sur {t} |
| `j_cascade_done` | "I am here" also marks {n} earlier quests done: | « J'en suis là » marque aussi {n} quêtes précédentes comme faites : |
| `j_cascade_up` | Also marks {n} earlier quests done: | Marque aussi {n} quêtes précédentes comme faites : |
| `j_cascade_reopen` | Also reopens {n} later quests: | Rouvre aussi {n} quêtes suivantes : |
| `j_cascade_untrack` | and stops tracking: | et arrête de suivre : |
| `j_undo_note` | Undo stays available | Annulable à tout moment |
| `j_after` | after {quest} | après {quest} |
| `j_line_starts` | Line {n} starts after {giver} · {quest} | La ligne {n} commence après {giver} · {quest} |
| `j_side_step` | side step · needs {giver} · {quest} | étape annexe · requiert {giver} · {quest} |
| `j_repeat_sum` | Repeatable: {d} daily · {w} weekly · {m} monthly | Répétables : {d} quotidiennes · {w} hebdomadaires · {m} mensuelles |
| `j_repeat_avail` | {n} repeatable quests available | {n} quêtes répétables disponibles |
| `j_no_order` | No known order | Ordre inconnu |
| `j_inconsistent` | Earlier quests not marked done | Quêtes précédentes non marquées faites |
| `j_last_change` | Last change: | Dernière modification : |
| `j_history` | History | Historique |
| `j_undo_to` | Undo back to here | Annuler jusqu'ici |
| `j_conflict` | Changed on the website since: {list}. | Modifié sur le site depuis : {list}. |
| `j_undo_anyway` | Undo anyway | Annuler quand même |
| `j_keep` | Keep | Conserver |
| `j_ally_site_note` | {name} updates their quests on hunger-companion.com | {name} met ses quêtes à jour sur hunger-companion.com |
| `j_ally_local_note` | Kept on this PC only, not on the website | Conservé sur ce PC uniquement, pas sur le site |
| `a_hide_map` | Hide on map | Masquer sur la carte |
| `a_remove_ally` | Remove from raid | Retirer du raid |
| `a_rename` | Rename | Renommer |
| `al_title` | RAID ALLIES | ALLIÉS DU RAID |
| `al_count` | {n} of 2 | {n} sur 2 |
| `al_help` | Their quests show hollow on your map, with their initial. | Leurs quêtes apparaissent en creux sur votre carte, avec leur initiale. |
| `al_hub` | From your hub on hunger-companion.com | Depuis votre hub sur hunger-companion.com |
| `al_friend` / `al_follower` / `al_following` | Friend / Follows you / You follow | Ami / Vous suit / Vous suivez |
| `al_n_quests` | {n} quests | {n} quêtes |
| `al_add` / `al_added` | Add / Added | Ajouter / Ajouté |
| `al_full` | Remove one first | Retirez-en un d'abord |
| `al_manual` | Someone without the app or an account | Quelqu'un sans l'appli ni compte |
| `al_name` | Name | Nom |
| `al_manual_help` | Kept on this PC only. Pick their quests in their tab. | Conservé sur ce PC uniquement. Choisissez ses quêtes dans son onglet. |
| `p_journal` | Journal | Journal |
| `p_sec_allies` | ALLIES | ALLIÉS |
| `p_manage` / `p_add` | Manage / Add | Gérer / Ajouter |
| `tag_site` / `tag_local` | SITE / LOCAL | SITE / LOCAL |
| `p_also` | also | aussi |
| `p_allies_empty` | Nobody yet. Pick a raid mate from your hub, or add someone who has no app. | Personne pour l'instant. Choisissez un coéquipier dans votre hub, ou ajoutez quelqu'un qui n'a pas l'appli. |
| `p_ally_quests` | {n} quests: {list} | {n} quêtes : {list} |
| `p_repeat_link` | {n} repeatable · Journal | {n} répétables · Journal |
| `p_summary` (new) | {q} quests · {a} allies · {l} layers | {q} quêtes · {a} alliés · {l} calques |
| `toast_done` | {quest} done | {quest} faite |
| `toast_done_n` | {quest} done (+{n} earlier) | {quest} faite (+{n} précédentes) |
| `toast_next` | Next: | Ensuite : |
| `toast_finished` | {quest} finished | {quest} terminée |
| `toast_reopened` | {quest} reopened | {quest} rouverte |
| `toast_undone_change` | Undone: {label} | Annulé : {label} |
| `toast_write_failed` | Couldn't save on the website (offline) | Enregistrement sur le site impossible (hors ligne) |
| `s_available` | Available · {n} spots here | Disponible · {n} points ici |
| `s_locked_n` | Locked · {n} before | Verrouillée · {n} avant |
| `s_locked_rep` | Locked · repeatable, after {quest} | Verrouillée · répétable, après {quest} |
| `s_done` / `s_tracked` | Done / Tracked | Faite / Suivie |
| `s_open_journal` | Open in journal | Ouvrir dans le journal |
| `k_ally` | ALLY | ALLIÉ |
| `t_quest` | Quest | Quête |
| `t_also_for` | Also for | Aussi pour |
| `t_added_by_you` / `t_local_ally` | Added by you / local ally | Ajouté par vous / allié local |
| `t_avail_you` | Available for you too | Disponible pour vous aussi |

---

## 8. Implementation checklist

- **`model.rs`**
  - `QuestGraph`: built from the pack `requires`, `lines`, `givers` and `kind`, with `ancestors()`, `descendants()`, `story(giver)`, `state(q, &D, &T)` and `up_next(giver)`.
  - `Change` / `Op` records, and `plan_track / plan_done / plan_reopen / plan_here / plan_done_to`, each returning the op list (which drives both the preview and the write).
  - New `Tracker` fields: `done`, `allies`.
  - `Ally` (site or manual) and ally `DrawPoint`s, with dedup into pips.
  - The Quests layer is filtered to available quests.
- **`net.rs`**
  - `POST /me/quests` (batch, `base_rev`, 409 re-read). It replaces the `track()` call site; the `/me/track` fallback is kept on 404.
  - `GET /me/hub`, `PUT /me/allies`.
  - Optimistic apply and revert on failure.
- **`config.rs`**
  - Manual allies (`allies.json`: name, slot, `D`, `T`).
  - Hidden allies and ally quests.
  - `history.json` (20 per subject, 7 days); `journal_tab`, `journal_open_cards`.
  - Badge-slot restoration on undo (`quest_slots`).
- **`journal.rs` (new):** the window (pattern of `search.rs`: topmost, EDIT filter, custom paint with panel tokens), tabs, UP NEXT, cards with the stepper and confirm strip, footer, history popover, "+ Ally" tab, and scrolling.
- **`panel.rs`**
  - New actions: `Action::Done(String)`, `Journal`, `TrackNext(String)` (toast), `ToggleAlly(String)`, `ToggleAllyQuest(String, String)`, `RemoveAllyQuest(String, String)`, `ManageAllies`.
  - The ✓ owner button, UP NEXT replacing the available list, the 2-line toast, the ALLIES section with its overflow levels, and the collapsed summary.
- **`markers.rs`:** `Kind::Ally`, `AllyNode`, `AllyPlaced`, `ally_pip()`; z renumbering (§5); `radius()` for the new kinds.
- **`scene.rs`:** per-ally clustering, pip folding (unfolded under spotlight), allies in petals after my owners, layer and Quests-token skipping near ally markers, spotlight for an ally or an ally quest.
- **`main.rs`:** wiring (journal open and close, change execution, toast with next, undo stack, conflict strip), and ally tooltips (§5).
- **`search.rs`:** state glyphs and lines, and routing of locked and done quests to the journal.
- **`tray.rs`:** a "Quest journal…" entry.
- **`i18n.rs`:** the strings of §7 (EN, FR; other languages fall back to EN until translated).
- **Tests:** a graph test on the real chains covering cascades both ways, "I am here", that undo restores the exact previous `D`/`T`/slots, and that repeatables are never cascaded down. The tests must never write to `%APPDATA%` (see the 2026-10-15 incident).

---

## 9. Open questions

1. **Repeatables:** does the website store a done state for daily, weekly and monthly quests, with a reset? This design assumes it does not: they can only be tracked and finished (untracked).
2. **Reopen cascade:** when a done quest is reopened, should its later quests (including other characters' lines) be reopened automatically, after a preview (this design), or should Reopen be refused while later quests are done?
3. **Data source:** this design takes `quest_preconditions.json` as the truth over `quest_chains.json.requires_to_start` (Mr Sweet, Hector), and treats *A Touch Of Bribery* as a standalone story step of Mr Sweet. Is that the in-game reality?
4. **Raid selection:** should the ≤ 2 chosen website allies be stored on the website (`PUT /me/allies`, so the site's hub could show the raid too) or only in the app (then the tracker request must name them)?
5. **Ally eligibility and consent:** can the player pick anyone in the hub (one-way followers too), or only mutual friends? And does the ally have to allow their quests to be shown?
