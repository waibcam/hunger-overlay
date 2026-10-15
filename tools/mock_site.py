"""Fake Hunger Companion website, used to develop the overlay offline.

It implements the v1 contract (docs/from_site_To_PC/SITE_RESPONSE_OVERLAY.md) and the proposed v1.2
(HungerDB/docs/from_PC_To_Site/PC_RESPONSE_OVERLAY_12.md):
  GET  /api/overlay/v1/pack.json        public, ETag / 304; quests carry line / line_pos / requires (v1.2)
  GET  /api/overlay/v1/me/tracker       Bearer hco_..., ETag = rev / 304, 401, 429; `completed` and the chosen
                                        allies' progression `allies[]` (v1.2)
  POST /api/overlay/v1/me/track         {"quest_id", "tracked"} -> tracker
  POST /api/overlay/v1/me/quests        {"changes": [{"quest_id", "state"}], "base_rev"} -> tracker, 409 (v1.2)
  GET  /api/overlay/v1/me/allies        hub list of possible allies (v1.2)
  PUT  /api/overlay/v1/me/allies        {"user_ids": [<= 2]} chosen raid allies -> tracker (v1.2)
  GET  /api/overlay/v1/app.json         public

The pack is built from the datamine bundle (quest_full.json + calibrations) with rules close to
the website's (<= 40 points per loot objective). It is NOT the source of truth: only the real website is.

Usage: python tools/mock_site.py <HungerDB/export> [--port 8787]
Test key: hco_MOCKmockMOCKmockMOCKmockMOCKmock (a fake key that only this local mock accepts)
In the app: base_url = "http://127.0.0.1:8787" in config.json.
"""
import sys, json, re, hashlib, time, argparse, os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TEST_KEY = 'hco_MOCKmockMOCKmockMOCKmockMOCKmock'  # dummy key accepted by this mock only
# Colours assigned to tracked quests, in order (like the website does).
PALETTE = ['#e9745e', '#5ec5e9', '#7ed957', '#c77dff', '#ff9f43', '#4ecca3', '#f4c542', '#f06292', '#a1887f', '#26c6da']
MAPS = {'Map01': 'Sarlat Farm', 'Map02': 'Jacques Bridge', 'Map03': 'Sombre Forest'}  # map id -> display name


def slugify(s):
    """Turn a quest title into a URL slug (lowercase ASCII words joined by '-')."""
    return re.sub(r'[^a-z0-9]+', '-', s.lower()).strip('-')


def build_pack(export):
    """Build the public quest pack (maps + quests with their map markers) from the export folder."""
    # Maps: each one carries the affine transform from world coordinates to map-texture pixels.
    maps = []
    aff = {}
    for m, name in MAPS.items():
        c = json.load(open(os.path.join(export, 'maps', f'{m.lower()}_calibration.json'), encoding='utf-8'))
        aff[m] = c['affine_world_to_px']
        maps.append({'map_id': m, 'name': name, 'texture': f'T_UI_Map_{m}_01a', 'texture_px': 4096,
                     'affine_world_to_px': c['affine_world_to_px'], 'layers': [{'layer_id': 'surface'}]})

    def proj(m, x, y):
        """Project world (x, y) on map `m` to texture pixels (px, py)."""
        a = aff[m]
        return round(a['a'] * x + a['b'] * y + a['c'], 1), round(a['d'] * x + a['e'] * y + a['f'], 1)

    # Quests: one entry per quest, each objective gets a stable key and its list of markers.
    quests = {}
    for q in json.load(open(os.path.join(export, 'quests', 'quest_full.json'), encoding='utf-8')):
        slug = slugify(q['title'])
        objs = []
        seen = {}
        for o in q['objectives']:
            r = o.get('resolution') or {}
            tr = r.get('target_ref')
            # Objective key: "<slug>|<target>", suffixed with "@<seq>" when the same target repeats.
            if tr:
                seen[tr] = seen.get(tr, 0) + 1
                key = f'{slug}|{tr}' if seen[tr] == 1 else f'{slug}|{tr}@{o["seq"]}'
            else:
                key = f'{slug}|#{o["seq"]}'
            # Markers: a single point for "map_coords", up to 40 container locations for "loot_sources".
            markers = []
            if r.get('kind') == 'map_coords' and r.get('map') in MAPS:
                px, py = proj(r['map'], r['x'], r['y'])
                markers.append({'map': r['map'], 'x': r['x'], 'y': r['y'], 'z': r.get('z'), 'px': px, 'py': py,
                                'label': o['instruction'].rstrip('.')})
            elif r.get('kind') == 'loot_sources':
                for s in (r.get('sample') or [])[:40]:
                    if s['map'] not in MAPS:
                        continue
                    px, py = proj(s['map'], s['x'], s['y'])
                    cont = s.get('loot_table', '').replace('LIT_', '')
                    markers.append({'map': s['map'], 'x': s['x'], 'y': s['y'], 'z': s.get('z'), 'px': px, 'py': py,
                                    'label': f'{o["instruction"].rstrip(".")} ({cont})'})
            objs.append({'objective_key': key, 'seq': o['seq'], 'type': o.get('type'), 'kind': r.get('kind'),
                         'label': o['instruction'].rstrip('.'), 'count': None, 'optional': False, 'target_ref': tr,
                         'hint': None, 'markers': markers})
        quests[q['quest_id']] = {'slug': slug, 'title': q['title'], 'giver': q.get('npc'),
                                 'url': f'https://hunger-companion.com/quests/{slug}', 'objectives': objs, 'places': []}
    # v1.2: quest lines and direct prerequisites (datamine: quest_chains.json + quest_preconditions.json).
    try:
        pre = json.load(open(os.path.join(export, 'quests', 'quest_preconditions.json'), encoding='utf-8'))
        chains = json.load(open(os.path.join(export, 'quests', 'quest_chains.json'), encoding='utf-8'))
    except OSError:
        pre, chains = {}, []
    for qid, v in pre.items():
        if qid in quests:
            quests[qid]['requires'] = [r for r in v.get('requires', []) if r in quests]
    for ch in chains:
        for i, qid in enumerate(ch['order']):
            if qid in quests:
                quests[qid]['line'] = ch['questline_id']
                quests[qid]['line_pos'] = i
    # The pack version is a short hash of the content, so it also serves as the ETag.
    givers = []
    for ch in chains:
        g = ch['questline_id'].split('.')[1]
        if g not in givers:
            givers.append(g)
    body = {'schema': 1, 'game_build': 'mock', 'generated_at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            'locale': 'en', 'maps': maps, 'quests': quests, 'givers': givers}
    h = hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()[:8]
    body['pack_version'] = f'mock.{h}'
    return body


def pick_tracked(pack):
    """Pick the quests the fake user tracks: one per map (with a map_coords objective) plus one loot_sources quest.

    (The single objective marked as done is chosen later, in main().)
    """
    chosen = []
    for m in MAPS:
        for qid, q in pack['quests'].items():
            if qid in chosen:
                continue
            if any(mk['map'] == m for o in q['objectives'] for mk in o['markers']) and \
                    any(o['kind'] == 'map_coords' for o in q['objectives']):
                chosen.append(qid)
                break
    for qid, q in pack['quests'].items():
        if qid not in chosen and any(o['kind'] == 'loot_sources' and o['markers'] for o in q['objectives']):
            chosen.append(qid)
            break
    return chosen


class State:
    """Global server state (single fake user): pack, tracked quests, done objectives, request timestamps."""
    pack = None
    pack_bytes = b''
    pack_etag = ''
    tracked = []
    completed = set()
    done = set()
    hits = {}
    # Fake allies (v1.2): id -> (name, tracked quests, completed quests).
    allies = {}
    # Allies chosen by the user (PUT /me/allies), returned with the tracker.
    selected = []


def tracked_list(ids):
    """`tracked[]` entries for quest ids, each with a fixed colour (its rank in tracking order, as v1.2 asks)."""
    tr = []
    for i, qid in enumerate(ids):
        q = State.pack['quests'][qid]
        tr.append({'quest_id': qid, 'slug': q['slug'], 'title': q['title'], 'color': PALETTE[i % len(PALETTE)],
                   'completed': False,
                   'objectives': [{'objective_key': o['objective_key'],
                                   'state': 'done' if o['objective_key'] in State.done else 'active'}
                                  for o in q['objectives']]})
    return tr


def tracker_body(allies=()):
    """Build the /me/tracker response; its `rev` is a hash of the content (used as the ETag)."""
    body = {'schema': 1, 'user': 'overlay-test', 'pack_version': State.pack['pack_version'],
            'tracked': tracked_list(State.tracked), 'completed': sorted(State.completed)}
    if allies:
        body['allies'] = [{'id': a, 'name': State.allies[a][0], 'tracked': tracked_list(State.allies[a][1]),
                           'completed': sorted(State.allies[a][2])} for a in allies if a in State.allies]
    body['rev'] = hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()[:8]
    return body


class H(BaseHTTPRequestHandler):
    """HTTP handler implementing the three overlay API endpoints."""

    def _send(self, code, body=b'', etag=None, extra=None):
        """Send a response with an optional JSON body, ETag and extra headers."""
        self.send_response(code)
        if body:
            self.send_header('Content-Type', 'application/json; charset=utf-8')
        if etag:
            self.send_header('ETag', f'"{etag}"')
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_GET(self):
        """Route GET requests; log each one to the console."""
        p = self.path.split('?')[0]
        inm = (self.headers.get('If-None-Match') or '').strip('"')
        print(f'{time.strftime("%H:%M:%S")} GET {self.path} UA={self.headers.get("User-Agent")} INM={inm or "-"}')
        # Reject any request that puts the key in the URL (keys must only travel in the Authorization header).
        if '?' in self.path and 'hco_' in self.path:
            return self._send(400, b'{"error":"key_in_query"}')
        # Public quest pack (optionally localized: pack.<lang>.json), cached with ETag / 304.
        if re.fullmatch(r'/api/overlay/v1/pack(\.[a-z]{2})?\.json', p):
            if inm == State.pack_etag:
                return self._send(304, etag=State.pack_etag)
            return self._send(200, State.pack_bytes, etag=State.pack_etag)
        # Public app version info (update check).
        if p == '/api/overlay/v1/app.json':
            return self._send(200, json.dumps({'latest': '0.1.0', 'min_supported': '0.1.0', 'url': '', 'sha256': '',
                                               'notes_url': 'https://hunger-companion.com/overlay'}).encode())
        # The user's tracked quests: requires the Bearer key, rate-limited to 120 requests per hour.
        if p == '/api/overlay/v1/me/allies':
            if (self.headers.get('Authorization') or '') != f'Bearer {TEST_KEY}':
                return self._send(401, b'{"error":"invalid_key"}')
            return self._send(200, json.dumps({'allies': [{'id': a, 'name': v[0], 'relation': 'friend',
                                                           'selected': a in State.selected, 'n_tracked': len(v[1])}
                                                          for a, v in State.allies.items()]}).encode())
        if p == '/api/overlay/v1/me/tracker':
            auth = self.headers.get('Authorization') or ''
            if auth != f'Bearer {TEST_KEY}':
                return self._send(401, b'{"error":"invalid_key"}', extra={'WWW-Authenticate': 'Bearer'})
            now = time.time()
            h = [t for t in State.hits.get('k', []) if now - t < 3600] + [now]
            State.hits['k'] = h
            if len(h) > 120:
                return self._send(429, b'{"error":"rate_limited"}', extra={'Retry-After': '600'})
            body = tracker_body(State.selected)
            if inm == body['rev']:
                return self._send(304, etag=body['rev'])
            return self._send(200, json.dumps(body, ensure_ascii=False).encode(), etag=body['rev'])
        self._send(404, b'{"error":"not_found"}')

    def do_POST(self):
        """Writes: /me/track (v1.1) and /me/quests (v1.2, atomic, 409 when base_rev is stale)."""
        p = self.path.split('?')[0]
        n = int(self.headers.get('Content-Length') or 0)
        data = json.loads(self.rfile.read(n) or b'{}')
        print(f'{time.strftime("%H:%M:%S")} POST {p} {data}')
        if (self.headers.get('Authorization') or '') != f'Bearer {TEST_KEY}':
            return self._send(401, b'{"error":"invalid_key"}')
        if p == '/api/overlay/v1/me/track':
            qid = data.get('quest_id')
            if qid not in State.pack['quests']:
                return self._send(400, b'{"error":"unknown_quest"}')
            if data.get('tracked') and qid not in State.tracked:
                State.tracked.append(qid)
            if not data.get('tracked') and qid in State.tracked:
                State.tracked.remove(qid)
        elif p == '/api/overlay/v1/me/quests':
            if data.get('base_rev') and data['base_rev'] != tracker_body(State.selected)['rev']:
                return self._send(409, b'{"error":"stale_rev"}')
            ch = data.get('changes') or []
            if any(c.get('quest_id') not in State.pack['quests'] or c.get('state') not in ('done', 'tracked', 'none')
                   for c in ch):
                return self._send(400, b'{"error":"bad_change"}')
            for c in ch:
                q, s = c['quest_id'], c['state']
                State.completed.discard(q)
                if q in State.tracked:
                    State.tracked.remove(q)
                if s == 'done':
                    State.completed.add(q)
                elif s == 'tracked':
                    State.tracked.append(q)
        else:
            return self._send(404, b'{"error":"not_found"}')
        body = tracker_body(State.selected)
        self._send(200, json.dumps(body, ensure_ascii=False).encode(), etag=body['rev'])

    def do_PUT(self):
        """PUT /me/allies: the raid allies (at most 2) chosen by the user."""
        n = int(self.headers.get('Content-Length') or 0)
        data = json.loads(self.rfile.read(n) or b'{}')
        print(f'{time.strftime("%H:%M:%S")} PUT {self.path} {data}')
        if (self.headers.get('Authorization') or '') != f'Bearer {TEST_KEY}':
            return self._send(401, b'{"error":"invalid_key"}')
        if self.path.split('?')[0] != '/api/overlay/v1/me/allies':
            return self._send(404, b'{"error":"not_found"}')
        State.selected = [a for a in (data.get('user_ids') or []) if a in State.allies][:2]
        body = tracker_body(State.selected)
        self._send(200, json.dumps(body, ensure_ascii=False).encode(), etag=body['rev'])

    def log_message(self, *a):
        """Silence the default access log (do_GET prints its own line)."""
        pass


def main():
    """Parse arguments, build the pack and fake tracker state, then serve on 127.0.0.1 only."""
    ap = argparse.ArgumentParser()
    ap.add_argument('export')
    ap.add_argument('--port', type=int, default=8787)
    a = ap.parse_args()
    State.pack = build_pack(a.export)
    State.pack_bytes = json.dumps(State.pack, ensure_ascii=False).encode()
    State.pack_etag = State.pack['pack_version']
    State.tracked = pick_tracked(State.pack)
    # Mark the first objective of the first tracked quest as done, to exercise the "done" rendering.
    first = State.pack['quests'][State.tracked[0]]['objectives'][0]['objective_key']
    State.done = {first}
    # Two fake allies, each on the first quest of a quest line.
    lines = [q for q, v in State.pack['quests'].items() if v.get('line_pos') == 0][:2]
    State.allies = {'u_ally1': ('Alice', lines[:1], set()), 'u_ally2': ('Bob', lines[1:2], set())}
    print('pack', State.pack['pack_version'], f'{len(State.pack_bytes) // 1024} KB', 'tracked quests:', State.tracked)
    print('test key:', TEST_KEY)
    ThreadingHTTPServer(('127.0.0.1', a.port), H).serve_forever()


if __name__ == '__main__':
    main()
