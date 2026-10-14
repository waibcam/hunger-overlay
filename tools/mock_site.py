"""Fake Hunger Companion website, used to develop the overlay offline.

It implements the v1 contract (docs/from_site_To_PC/SITE_RESPONSE_OVERLAY.md):
  GET /api/overlay/v1/pack.json        public, ETag / 304
  GET /api/overlay/v1/me/tracker       Bearer hco_..., ETag = rev / 304, 401, 429
  GET /api/overlay/v1/app.json         public

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
    # The pack version is a short hash of the content, so it also serves as the ETag.
    body = {'schema': 1, 'game_build': 'mock', 'generated_at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            'locale': 'en', 'maps': maps, 'quests': quests}
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
    done = set()
    hits = {}


def tracker_body():
    """Build the /me/tracker response; its `rev` is a hash of the content (used as the ETag)."""
    tr = []
    for i, qid in enumerate(sorted(State.tracked, key=lambda q: State.pack['quests'][q]['slug'])):
        q = State.pack['quests'][qid]
        tr.append({'quest_id': qid, 'slug': q['slug'], 'title': q['title'], 'color': PALETTE[i % len(PALETTE)],
                   'completed': False,
                   'objectives': [{'objective_key': o['objective_key'],
                                   'state': 'done' if o['objective_key'] in State.done else 'active'}
                                  for o in q['objectives']]})
    body = {'schema': 1, 'user': 'overlay-test', 'pack_version': State.pack['pack_version'], 'tracked': tr}
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
        if p == '/api/overlay/v1/me/tracker':
            auth = self.headers.get('Authorization') or ''
            if auth != f'Bearer {TEST_KEY}':
                return self._send(401, b'{"error":"invalid_key"}', extra={'WWW-Authenticate': 'Bearer'})
            now = time.time()
            h = [t for t in State.hits.get('k', []) if now - t < 3600] + [now]
            State.hits['k'] = h
            if len(h) > 120:
                return self._send(429, b'{"error":"rate_limited"}', extra={'Retry-After': '600'})
            body = tracker_body()
            if inm == body['rev']:
                return self._send(304, etag=body['rev'])
            return self._send(200, json.dumps(body, ensure_ascii=False).encode(), etag=body['rev'])
        self._send(404, b'{"error":"not_found"}')

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
    print('pack', State.pack['pack_version'], f'{len(State.pack_bytes) // 1024} KB', 'tracked quests:', State.tracked)
    print('test key:', TEST_KEY)
    ThreadingHTTPServer(('127.0.0.1', a.port), H).serve_forever()


if __name__ == '__main__':
    main()
