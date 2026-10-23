#!/usr/bin/env python3
"""Generate crates/ef-core/data/release_index.json: every Grand Theft Auto release the cited sources
list, by game, platform, region, version and edition.

Facts only (titles, regions, languages, serials, versions, sizes, hashes, dates, developer and
publisher names) are taken from pinned sources. Nothing is filled in by hand: a release, date or
version that no source lists is not in the index, and the known holes are listed under "gaps".

    gen_release_index.py <cache dir> <output .json>

The cache dir receives the downloaded inputs (DAT files, wikitext, titledb); a later run reads the
cache instead of the network. Inputs:

* libretro-database @ LIBRETRO (redump / No-Intro DAT files, CC BY-SA 4.0);
* tools/data/redump_pc_gta.dat: the Grand Theft Auto entries of redump's IBM PC compatible DAT
  "IBM - PC compatible - Datfile (58837) (2026-06-15 06-43-57)" (http://redump.org/datfile/pc/),
  extracted unchanged by `gen_release_index.py --extract-pc <zip> <out>`;
* tools/data/redump_dc_ipbin.tsv: the IP.BIN header fields shown on redump's Dreamcast disc pages;
* English Wikipedia articles at the revisions in WIKIPEDIA (infobox developer, publisher, platforms
  and release dates, CC BY-SA 4.0);
* blawar/titledb @ TITLEDB (Nintendo Switch title IDs, release dates and update versions);
* the PC executable builds Eyefinder identifies (crates/ef-core/src/edition.rs), whose sources are
  SilentPatch @74976544 and plugin-sdk @15f15b60.
"""
import json, os, re, sys, time, urllib.parse, urllib.request, zipfile

LIBRETRO = '12c21cf79f1e20eff6ac68b80875816c717d0a7a'
TITLEDB = 'd424690446eec6f4484209b9a1c067c7f7511398'
REDUMP_PC = 'IBM - PC compatible - Datfile (58837) (2026-06-15 06-43-57)'
HERE = os.path.dirname(os.path.abspath(__file__))
UA = 'Eyefinder-release-index/1.0 (https://github.com/galaxygas/Eyefinder)'

# Game id, title, Eyefinder game (cli id) or None, Wikipedia article, pinned revision.
GAMES = [
    ('gta1', 'Grand Theft Auto', 'gta1', 'Grand Theft Auto (video game)', 1381143487),
    ('london1969', 'Grand Theft Auto: London 1969', 'london1969', 'Grand Theft Auto: London 1969', 1374537946),
    ('london1961', 'Grand Theft Auto: London 1961', 'london1961', 'Grand Theft Auto: London 1969', 1374537946),
    ('gta2', 'Grand Theft Auto 2', 'gta2', 'Grand Theft Auto 2', 1376513985),
    ('gta3', 'Grand Theft Auto III', 'gta3', 'Grand Theft Auto III', 1381472347),
    ('vc', 'Grand Theft Auto: Vice City', 'vc', 'Grand Theft Auto: Vice City', 1379051054),
    ('advance', 'Grand Theft Auto Advance', 'advance', 'Grand Theft Auto Advance', 1380161919),
    ('sa', 'Grand Theft Auto: San Andreas', 'sa', 'Grand Theft Auto: San Andreas', 1380835714),
    ('lcs', 'Grand Theft Auto: Liberty City Stories', None, 'Grand Theft Auto: Liberty City Stories', 1380607482),
    ('vcs', 'Grand Theft Auto: Vice City Stories', None, 'Grand Theft Auto: Vice City Stories', 1373763617),
    ('gta4', 'Grand Theft Auto IV', None, 'Grand Theft Auto IV', 1379090820),
    ('tlad', 'Grand Theft Auto IV: The Lost and Damned', None, 'Grand Theft Auto IV: The Lost and Damned', 1381073998),
    ('ctw', 'Grand Theft Auto: Chinatown Wars', None, 'Grand Theft Auto: Chinatown Wars', 1381019415),
    ('tbogt', 'Grand Theft Auto: The Ballad of Gay Tony', None, 'Grand Theft Auto: The Ballad of Gay Tony', 1360468637),
    ('eflc', 'Grand Theft Auto: Episodes from Liberty City', None, None, None),
    ('gta5', 'Grand Theft Auto V', None, 'Grand Theft Auto V', 1381115130),
    ('trilogy-de', 'Grand Theft Auto: The Trilogy – The Definitive Edition', None,
     'Grand Theft Auto: The Trilogy – The Definitive Edition', 1380998467),
    ('collections', 'Collections and compilations', None, None, None),
]

# DAT title prefix (before " (" or " - " variants) -> game ids. Longest prefix wins.
TITLES = [
    ('Grand Theft Auto - Mission Pack 1 - London 1969', ['london1969']),
    ('Grand Theft Auto - Mission Pack #1 - London 1969', ['london1969']),
    ('Grand Theft Auto - London 1969', ['london1969']),
    ('Grand Theft Auto - London', ['london1969']),
    ('G.T.A - Grand Theft Auto', ['gta1']),
    ('Grand Theft Auto 2', ['gta2']),
    ('Grand Theft Auto III', ['gta3']),
    ('Grand Theft Auto - Vice City Stories', ['vcs']),
    ('Grand Theft Auto - Vice City', ['vc']),
    ('Grand Theft Auto - San Andreas', ['sa']),
    ('Grand Theft Auto - Liberty City Stories', ['lcs']),
    ('Grand Theft Auto - Chinatown Wars', ['ctw']),
    ('Grand Theft Auto IV & Episodes from Liberty City', ['gta4', 'eflc']),
    ('Grand Theft Auto IV - The Ballad of Gay Tony', ['tbogt']),
    ('Grand Theft Auto IV - The Lost and Damned', ['tlad']),
    ('GTA IV - Theme Pack', ['gta4']),
    ('Grand Theft Auto IV', ['gta4']),
    ('Grand Theft Auto - Episodes from Liberty City', ['eflc']),
    ('Grand Theft Auto V', ['gta5']),
    ('GTA Online', ['gta5']),
    ('Grand Theft Auto Trylogia', ['gta3', 'vc', 'sa']),
    ('Grand Theft Auto', ['gta1']),
]
# Not releases of a GTA game: cheat discs, other games, press discs.
EXCLUDE = re.compile(r"Action Replay|GameShark|Cheat|CD avec|Screen Shots|Skin Disc|^Advance GTA|GTA CZ a SK")

# (source id, path in libretro-database, platform id, media)
DATS = [
    ('libretro-redump-ps1', 'metadat/redump/Sony - PlayStation.dat', 'ps1', 'disc'),
    ('libretro-redump-ps2', 'metadat/redump/Sony - PlayStation 2.dat', 'ps2', 'disc'),
    ('libretro-redump-psp', 'metadat/redump/Sony - PlayStation Portable.dat', 'psp', 'disc'),
    ('libretro-nointro-psp', 'metadat/no-intro/Sony - PlayStation Portable.dat', 'psp', 'umd'),
    ('libretro-nointro-psn-psp', 'metadat/no-intro/Sony - PlayStation Portable (PSN).dat', 'psp', 'digital'),
    ('libretro-redump-ps3', 'metadat/redump/Sony - PlayStation 3.dat', 'ps3', 'disc'),
    ('libretro-nointro-psn-ps3', 'metadat/no-intro/Sony - PlayStation 3 (PSN).dat', 'ps3', 'digital'),
    ('libretro-redump-xbox', 'metadat/redump/Microsoft - Xbox.dat', 'xbox', 'disc'),
    ('libretro-redump-x360', 'metadat/redump/Microsoft - Xbox 360.dat', 'x360', 'disc'),
    ('libretro-nointro-x360', 'metadat/no-intro/Microsoft - Xbox 360 (Digital).dat', 'x360', 'digital'),
    ('libretro-redump-dc', 'metadat/redump/Sega - Dreamcast.dat', 'dc', 'disc'),
    ('libretro-nointro-gbc', 'metadat/no-intro/Nintendo - Game Boy Color.dat', 'gbc', 'rom'),
    ('libretro-nointro-gba', 'metadat/no-intro/Nintendo - Game Boy Advance.dat', 'gba', 'rom'),
    ('libretro-nointro-nds', 'metadat/no-intro/Nintendo - Nintendo DS.dat', 'nds', 'rom'),
]

PLATFORMS = {
    'pc': 'PC (DOS / Windows)', 'mac': 'Mac', 'ps1': 'PlayStation', 'ps2': 'PlayStation 2', 'psp': 'PlayStation Portable',
    'ps3': 'PlayStation 3', 'ps4': 'PlayStation 4', 'ps5': 'PlayStation 5', 'xbox': 'Xbox', 'x360': 'Xbox 360',
    'xone': 'Xbox One', 'xsx': 'Xbox Series X/S', 'dc': 'Dreamcast', 'gbc': 'Game Boy Color', 'gba': 'Game Boy Advance',
    'nds': 'Nintendo DS', 'switch': 'Nintendo Switch', 'ios': 'iOS', 'android': 'Android', 'fireos': 'Fire OS',
    'winphone': 'Windows Phone', 'stadia': 'Stadia', 'linux': 'Linux',
}
# Wikipedia infobox platform labels -> platform id.
WIKI_PLATFORMS = [
    (r'^NS$', 'switch'), (r'^Win$', 'pc'), (r'^XONE$', 'xone'), (r'^XSXS$|Series X', 'xsx'),
    (r'PlayStation 5|^PS5$', 'ps5'), (r'PlayStation 4|^PS4$', 'ps4'), (r'PlayStation 3|^PS3$', 'ps3'),
    (r'PlayStation 2|^PS2$', 'ps2'),
    (r'PlayStation Portable|PSP', 'psp'), (r'PlayStation', 'ps1'), (r'Xbox Series', 'xsx'), (r'Xbox One', 'xone'),
    (r'Xbox 360', 'x360'), (r'Xbox', 'xbox'), (r'Game Boy Color', 'gbc'), (r'Game Boy Advance', 'gba'),
    (r'Nintendo DS', 'nds'), (r'Nintendo Switch|Switch', 'switch'), (r'Dreamcast', 'dc'), (r'Windows Phone', 'winphone'),
    (r'MS-DOS|DOS|Windows|PC', 'pc'), (r'Mac', 'mac'), (r'iOS', 'ios'), (r'Android', 'android'), (r'Fire OS', 'fireos'),
    (r'Stadia', 'stadia'), (r'Linux', 'linux'),
]

GAPS = [
    ('ps4, ps5, xone, xsx', 'Disc and digital releases for PlayStation 4/5 and Xbox One / Series X|S: no redump or No-Intro DAT for these platforms exists in libretro-database or on redump.org, so their regional releases, serials and versions are not listed.'),
    ('ios, android, fireos, winphone', 'Mobile releases: the iTunes Search API gives only the current version, and no source of the version history was reachable (PCGamingWiki answered 403); the stores were not used.'),
    ('pc', 'PC patches of GTA IV (1.0.0.0 to 1.0.8.0, Complete Edition 1.2.0.x) and GTA V builds: the citable lists (PCGamingWiki, gtamods.com) could not be reached (403 / 503).'),
    ('pc', 'Digital PC releases (Steam, Rockstar Games Launcher, Epic, GOG and the old Rockstar Classics downloads): store build numbers carry no version names; only the executable builds Eyefinder identifies are listed.'),
    ('mac', 'Mac releases: redump publishes a Mac disc DAT, but it has not been added to the index yet; App Store versions are not listed.'),
    ('psp, ps3, x360', 'Digital entries (PSN, Xbox Live) come from No-Intro and list licence or add-on files, not every regional storefront release.'),
]


def fetch(cache, name, url, wait=0.0):
    path = os.path.join(cache, name)
    if not os.path.exists(path):
        time.sleep(wait)
        req = urllib.request.Request(url, headers={'User-Agent': UA})
        data = urllib.request.urlopen(req, timeout=120).read()
        open(path, 'wb').write(data)
    return open(path, 'rb').read()


def clp_games(text):
    """libretro clrmamepro DAT entries."""
    for m in re.finditer(r'^game \($(.*?)^\)$', text, re.M | re.S):
        body = m.group(1)
        get = lambda k: (re.search(rf'^\s*{k} "([^"]*)"', body, re.M) or [None, ''])[1]
        roms = [dict(name=n, size=int(s), sha1=h.lower())
                for n, s, h in re.findall(r'rom \( name "([^"]*)" size (\d+) crc \w+ (?:md5 \w+ )?sha1 (\w+)', body)]
        yield dict(name=get('name'), region=get('region'), serial=get('serial'), roms=roms)


def xml_games(text):
    """Logiqx XML DAT entries (redump.org downloads)."""
    for m in re.finditer(r'<game name="([^"]*)">(.*?)</game>', text, re.S):
        roms = [dict(name=n.replace('&amp;', '&'), size=int(s), sha1=h.lower())
                for n, s, h in re.findall(r'<rom name="([^"]*)" size="(\d+)" crc="\w+" md5="\w+" sha1="(\w+)"', m.group(2))]
        name = m.group(1).replace('&amp;', '&')
        yield dict(name=name, region='', serial='', roms=roms)


def games_of(name):
    if EXCLUDE.search(name):
        return None
    best = None
    for prefix, ids in TITLES:
        if (name == prefix or name.startswith(prefix + ' (') or name.startswith(prefix + ' - ')) and (
                best is None or len(prefix) > len(best[0])):
            best = (prefix, ids)
    if best is None and ' + Grand Theft Auto + ' in name:
        return ['gta1'], True  # "Pro Foot Contest '98 + Grand Theft Auto + Courier Crisis"
    return (best[1], False) if best else None


LANG = re.compile(r'^[A-Z][a-z](,[A-Z][a-z])*$')


def split_name(name):
    """Region, languages, version and the other tags of a No-Intro / redump title."""
    tags = re.findall(r'\(([^()]*)\)', name)
    out = dict(region=tags[0] if tags else '', languages='', version='', tags=[])
    for t in tags[1:]:
        if LANG.match(t):
            out['languages'] = t
        elif re.match(r'^(v\d[\d.]*|Rev \d+|v\d+\.\d+.*)$', t):
            out['version'] = t
        else:
            out['tags'].append(t)
    return out


def date_iso(text):
    text = re.sub(r'\{\{[^{}]*\}\}|<[^>]*>|\[\[|\]\]|\'\'+', '', text).strip()
    for fmt in ('%d %B %Y', '%B %d, %Y', '%B %d %Y', '%B %Y', '%Y'):
        try:
            t = time.strptime(text, fmt)
            return time.strftime({'%B %Y': '%Y-%m', '%Y': '%Y'}.get(fmt, '%Y-%m-%d'), t)
        except ValueError:
            pass
    return None


def infobox(wikitext):
    i = wikitext.find('{{Infobox video game')
    if i < 0:
        return {}
    depth, j = 0, i
    while j < len(wikitext):
        if wikitext.startswith('{{', j):
            depth += 1
            j += 2
        elif wikitext.startswith('}}', j):
            depth -= 1
            j += 2
            if depth == 0:
                break
        else:
            j += 1
    body = wikitext[i:j]
    fields, cur = {}, None
    depth = 0
    for line in body.split('\n')[1:]:
        m = re.match(r'^\s*\|\s*([a-z_]+)\s*=(.*)$', line)
        if m and depth <= 0:
            cur = m.group(1)
            fields[cur] = m.group(2)
        elif cur:
            fields[cur] += '\n' + line
        depth += line.count('{{') - line.count('}}') if cur else 0
        if m:
            depth = m.group(2).count('{{') - m.group(2).count('}}')
    return fields


def plain(text):
    text = re.sub(r'<ref[^>]*/>|<ref[^>]*>.*?</ref>', '', text, flags=re.S)
    text = re.sub(r'\{\{efn[^{}]*(\{\{[^{}]*\}\}[^{}]*)*\}\}', '', text)
    text = re.sub(r'\{\{(?:spaces|nbsp)[^{}]*\}\}', ' ', text)
    text = re.sub(r'\{\{(?:ubl|unbulleted list|plainlist|flatlist)\|', '', text, flags=re.I)
    text = re.sub(r'\[\[(?:[^|\]]*\|)?([^\]]*)\]\]', r'\1', text)
    text = re.sub(r'<br\s*/?>', ', ', text)
    text = re.sub(r'\{\{|\}\}|\'\'+', '', text)
    parts = [p.strip(' *,') for p in re.split(r'\n|\|', text)]
    return ', '.join(p for p in parts if p)


def unref(text):
    text = re.sub(r'<ref[^>]*/>', '', text)
    return re.sub(r'<ref[^>]*>.*?</ref>', '', text, flags=re.S)


MONTHS = 'January|February|March|April|May|June|July|August|September|October|November|December'
DATE = rf'\d{{1,2}} (?:{MONTHS}) \d{{4}}|(?:{MONTHS}) \d{{1,2}}, \d{{4}}|(?:{MONTHS}) \d{{4}}'
LINK = re.compile(r'\[\[(?:[^|\]]*\|)?([^\]]*)\]\]')
TEMPLATE = r'\{\{\s*(?:Video game release|vgrelease)\s*\|([^{}]*)\}\}'


def releases_from_wiki(field):
    """[(platform label, region, date)] from an infobox `released` field: bold platform headings,
    each followed by `{{Video game release|REGION|date|...}}` / `{{vgrelease|...}}` or by a bare date
    (no region given)."""
    field = re.sub(r'title=\{\{[Nn]obold\|[^{}]*\}\}', '', unref(field))
    out, labels, fresh = [], [], True
    for tok in re.finditer(rf"'''([^']+)'''|{TEMPLATE}|({DATE})", field):
        if tok.group(1):
            if not fresh:
                labels, fresh = [], True
            labels.append(LINK.sub(r'\1', tok.group(1)))
            continue
        fresh = False
        label = ', '.join(labels) or None
        if tok.group(2):
            parts = [p.strip() for p in tok.group(2).split('|')]
            for k in range(0, len(parts) - 1, 2):
                d = date_iso(parts[k + 1])
                if d:
                    out.append((label, parts[k], d))
        else:
            out.append((label, '', date_iso(tok.group(3))))
    return [o for o in out if o[2]]


def names(field):
    """Company names of a developer / publisher field: link texts and the values of release
    templates, without regions, platform headings or layout templates."""
    field = re.sub(r'\{\{efn[^{}]*(\{\{[^{}]*\}\}[^{}]*)*\}\}', '', unref(field))
    field = re.sub(r"'''[^']+'''", '', field)
    found = []
    for t in re.findall(TEMPLATE, field):
        found += [LINK.sub(r'\1', v).strip() for v in t.split('|')[1::2]]
    field = re.sub(TEMPLATE, '', field)
    found = LINK.findall(field) + found
    if not found:
        return plain(field)
    out = []
    for n in found:
        n = n.strip()
        if n and n not in out:
            out.append(n)
    return ', '.join(out)


def wiki_platforms(label):
    ids = []
    for part in re.split(r',|/| and |\band\b', label or ''):
        part = part.strip()
        for pat, pid in WIKI_PLATFORMS:
            if re.search(pat, part):
                ids.append(pid)
                break
    return ids


def extract_pc(zip_path, out):
    z = zipfile.ZipFile(zip_path)
    (inner,) = z.namelist()
    lines = z.read(inner).decode('utf-8').splitlines(keepends=True)
    head, keep, cur = [], [], None
    for line in lines:
        if cur is None and line.startswith('\t<game name="'):
            cur = [line]
        elif cur is not None:
            cur.append(line)
            if line.startswith('\t</game>'):
                name = cur[0].split('"')[1].replace('&amp;', '&')
                if games_of(name):
                    keep.extend(cur)
                cur = None
        elif not keep and not line.startswith('</datafile>'):
            head.append(line)
    open(out, 'w').write(''.join(head) + ''.join(keep) + '</datafile>\n')


def main():
    if sys.argv[1] == '--extract-pc':
        return extract_pc(sys.argv[2], sys.argv[3])
    cache, out = sys.argv[1], sys.argv[2]
    os.makedirs(cache, exist_ok=True)
    sources = {}
    games = {g[0]: dict(id=g[0], title=g[1], eyefinder=g[2], developer='', publisher='', first_release=None,
                        platform_dates=[], releases=[], builds=[]) for g in GAMES}
    order = [g[0] for g in GAMES]

    def add(gids, rel):
        rel['games'] = gids
        if len(gids) > 1 or rel.pop('compilation', False):
            games['collections']['releases'].append(rel)
        else:
            games[gids[0]]['releases'].append(rel)

    for sid, rel_path, platform, media in DATS:
        url = f'https://raw.githubusercontent.com/libretro/libretro-database/{LIBRETRO}/' + urllib.parse.quote(rel_path)
        sources[sid] = dict(name=f'{os.path.basename(rel_path)} (libretro-database)', url=url, pin=LIBRETRO,
                            license='CC BY-SA 4.0 (libretro-database)')
        text = fetch(cache, sid + '.dat', url).decode('utf-8')
        for g in clp_games(text):
            m = games_of(g['name'])
            if not m:
                continue
            gids, comp = m
            # GTA Advance is the Game Boy Advance "Grand Theft Auto".
            if platform == 'gba' and gids == ['gta1']:
                gids = ['advance']
            info = split_name(g['name'])
            add(gids, dict(platform=platform, title=g['name'], region=g['region'] or info['region'],
                           languages=info['languages'], version=info['version'], tags=info['tags'], serial=g['serial'],
                           media=media, files=g['roms'], source=sid, compilation=comp))

    pc_dat = os.path.join(HERE, 'data', 'redump_pc_gta.dat')
    sources['redump-pc'] = dict(name=f'{REDUMP_PC}, Grand Theft Auto entries (tools/data/redump_pc_gta.dat)',
                                url='http://redump.org/datfile/pc/', pin=REDUMP_PC, license='redump.org (no licence stated)')
    for g in xml_games(open(pc_dat, encoding='utf-8').read()):
        m = games_of(g['name'])
        if not m:
            continue
        gids, comp = m
        info = split_name(g['name'])
        add(gids, dict(platform='pc', title=g['name'], region=info['region'], languages=info['languages'],
                       version=info['version'], tags=info['tags'], serial='', media='disc', files=g['roms'],
                       source='redump-pc', compilation=comp))

    # Dreamcast IP.BIN header fields, as shown on each redump disc page.
    ip_path = os.path.join(HERE, 'data', 'redump_dc_ipbin.tsv')
    for line in open(ip_path, encoding='utf-8').read().splitlines()[1:]:
        disc, serials, product, version, date, region = line.split('\t')
        sources[f'redump-disc-{disc}'] = dict(name=f'redump disc {disc} (IP.BIN header)', url=f'http://redump.org/disc/{disc}/',
                                              pin='page read 2026-10-23', license='redump.org (no licence stated)')
        for r in games['gta2']['releases']:
            if r['platform'] == 'dc' and r['serial'] in serials.split(','):
                r['ip_bin'] = dict(product=product, version=version, date=date, source=f'redump-disc-{disc}')

    # Wikipedia infoboxes.
    for gid, title, _, article, rev in GAMES:
        if not article or not rev:
            continue
        sid = f'wikipedia-{gid}'
        url = f'https://en.wikipedia.org/w/index.php?title={urllib.parse.quote(article)}&oldid={rev}&action=raw'
        sources[sid] = dict(name=f'Wikipedia: {article} (revision {rev})', url=url, pin=str(rev), license='CC BY-SA 4.0')
        text = fetch(cache, f'{sid}.wiki', url, wait=3).decode('utf-8')
        if gid == 'london1961':
            continue  # the London article's infobox describes London 1969
        box = infobox(text)
        g = games[gid]
        g['developer'] = names(box.get('developer', ''))
        g['publisher'] = names(box.get('publisher', ''))
        g['wikipedia'] = sid
        for label, region, date in releases_from_wiki(box.get('released', '')):
            pids = wiki_platforms(label) if label else wiki_platforms(plain(box.get('platforms', '')))
            for pid in pids or ['?']:
                g['platform_dates'].append(dict(platform=pid, label=label or plain(box.get('platforms', '')),
                                                region=region, date=date, source=sid))
        dates = sorted(d['date'] for d in g['platform_dates'])
        g['first_release'] = dates[0] if dates else None

    # Nintendo Switch (titledb).
    sources['titledb'] = dict(name='blawar/titledb US.en.json and versions.json', pin=TITLEDB,
                              url=f'https://github.com/blawar/titledb/tree/{TITLEDB}', license='no licence stated')
    db = json.loads(fetch(cache, 'titledb_US.en.json', f'https://raw.githubusercontent.com/blawar/titledb/{TITLEDB}/US.en.json'))
    versions = json.loads(fetch(cache, 'titledb_versions.json', f'https://raw.githubusercontent.com/blawar/titledb/{TITLEDB}/versions.json'))
    switch = {'Grand Theft Auto III – The Definitive Edition': 'gta3', 'Grand Theft Auto: Vice City – The Definitive Edition': 'vc',
              'Grand Theft Auto: San Andreas – The Definitive Edition': 'sa'}
    for e in db.values():
        game = switch.get(e.get('name') or '')
        if not game or not e.get('id'):
            continue
        tid = e['id']
        d = str(e.get('releaseDate') or '')
        upd = versions.get(tid.lower(), {})
        games['trilogy-de']['releases'].append(dict(
            platform='switch', title=e['name'], region='USA (US eShop)', languages='', version='', tags=['Definitive Edition'],
            serial=tid, media='digital', files=[], source='titledb', games=['trilogy-de'],
            date=f'{d[:4]}-{d[4:6]}-{d[6:]}' if len(d) == 8 else None,
            updates=[dict(version=int(v), date=dt) for v, dt in sorted(upd.items(), key=lambda kv: int(kv[0]))]))

    # PC executable builds Eyefinder identifies (edition.rs).
    sources['silentpatch'] = dict(name='SilentPatch MemoryMgr.GTA.h', pin='74976544',
                                  url='https://github.com/CookiePLMonster/SilentPatch/blob/74976544/SilentPatch/MemoryMgr.GTA.h',
                                  license='MIT')
    sources['plugin-sdk'] = dict(name='plugin-sdk GameVersion.h', pin='15f15b60',
                                 url='https://github.com/DK22Pac/plugin-sdk/blob/15f15b60/shared/GameVersion.h', license='MIT')
    edition = open(os.path.join(HERE, '..', 'crates', 'ef-core', 'src', 'edition.rs'), encoding='utf-8').read()
    for const, gid in (('III_CHECKS', 'gta3'), ('VC_CHECKS', 'vc'), ('SA_CHECKS', 'sa')):
        block = edition[edition.index(f'const {const}'):]
        block = block[:block.index('];')]
        for bid, label in re.findall(r'id: "([^"]+)", label: "([^"]+)"', block):
            src = 'plugin-sdk' if 'compact' in bid or 'hoodlum' in bid or 'other' in bid else 'silentpatch'
            games[gid]['builds'].append(dict(platform='pc', id=bid, label=label, identify='executable signature', source=src))

    for g in games.values():
        g['releases'].sort(key=lambda r: (r['platform'], r['title'], r['serial']))
    index = dict(generator='tools/gen_release_index.py', sources=sources, platforms=PLATFORMS,
                 games=[games[i] for i in order], gaps=[dict(platforms=p, text=t) for p, t in GAPS])
    open(out, 'w').write(json.dumps(index, indent=1, ensure_ascii=False) + '\n')
    n = sum(len(g['releases']) for g in index['games'])
    print(f'{n} releases, {sum(len(g["platform_dates"]) for g in index["games"])} dates, {len(sources)} sources')


if __name__ == '__main__':
    main()
