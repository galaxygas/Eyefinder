#!/usr/bin/env python3
"""Import docs/research/GTA_Builds_Versions_Database.md into the build database
(crates/ef-core/data/database/*.toml).

    import_research_md.py <research .md> <database dir> [<report .md>]

Every heading becomes a [[section]] with its prose kept verbatim, every table row becomes an [[entry]]
whose cells are kept verbatim, and every numbered source becomes a [sources] item. Each imported item
records `origin = "md:<line>"`; the ef-core test `research_md_is_fully_covered` checks that every row,
heading and source of the research file is still in the database.

Nothing is inferred. The normalised fields (`games`, `platforms`, `date`, `serials`, `version`,
`confidence`, `sources`) are filled only by the explicit tables and strict patterns below; when a
cell does not match them, the field stays empty and the verbatim text is all there is. The report
lists every such cell so it can be reviewed.

After the import the TOML files are the source of truth: edit them (docs/DATABASE.md), do not re-run
this script over them.
"""
import json, re, sys, tomllib
from collections import Counter, defaultdict
from pathlib import Path

# ---------------------------------------------------------------------------------------------------
# Games, platforms and kinds known to the database (written to database.toml).

GAMES = [
    # id, title, Eyefinder cli id
    ('gta1', 'Grand Theft Auto', 'gta1'),
    ('london1969', 'Grand Theft Auto: London 1969', 'london1969'),
    ('london1961', 'Grand Theft Auto: London 1961', 'london1961'),
    ('gta2', 'Grand Theft Auto 2', 'gta2'),
    ('gta3', 'Grand Theft Auto III', 'gta3'),
    ('vc', 'Grand Theft Auto: Vice City', 'vc'),
    ('advance', 'Grand Theft Auto Advance', 'advance'),
    ('sa', 'Grand Theft Auto: San Andreas', 'sa'),
    ('lcs', 'Grand Theft Auto: Liberty City Stories', None),
    ('vcs', 'Grand Theft Auto: Vice City Stories', None),
    ('gta4', 'Grand Theft Auto IV', None),
    ('tlad', 'Grand Theft Auto IV: The Lost and Damned', None),
    ('ctw', 'Grand Theft Auto: Chinatown Wars', None),
    ('tbogt', 'Grand Theft Auto: The Ballad of Gay Tony', None),
    ('eflc', 'Grand Theft Auto: Episodes from Liberty City', None),
    ('gta5', 'Grand Theft Auto V', None),
    ('gta-online', 'Grand Theft Auto Online', None),
    ('trilogy-de', 'Grand Theft Auto: The Trilogy – The Definitive Edition', None),
    ('gta6', 'Grand Theft Auto VI', None),
    ('collections', 'Collections and compilations', None),
    ('cancelled', 'Cancelled, unreleased and lost projects', None),
    ('media', 'Related media and licensed releases', None),
    ('series', 'The whole series', None),
]

# The release index platforms plus those only the research file names.
PLATFORMS = {
    'pc': 'PC (DOS / Windows)', 'mac': 'Mac', 'linux': 'Linux', 'ps1': 'PlayStation', 'ps2': 'PlayStation 2',
    'psp': 'PlayStation Portable', 'vita': 'PlayStation Vita', 'ps3': 'PlayStation 3', 'ps4': 'PlayStation 4',
    'ps5': 'PlayStation 5', 'xbox': 'Xbox', 'x360': 'Xbox 360', 'xone': 'Xbox One', 'xsx': 'Xbox Series X/S',
    'dc': 'Dreamcast', 'saturn': 'Sega Saturn', 'n64': 'Nintendo 64', 'gc': 'GameCube', 'gbc': 'Game Boy Color',
    'gba': 'Game Boy Advance', 'nds': 'Nintendo DS', 'switch': 'Nintendo Switch', 'ios': 'iOS',
    'android': 'Android', 'fireos': 'Fire OS', 'winphone': 'Windows Phone', 'palm': 'Palm OS', 'j2me': 'J2ME',
    'stadia': 'Stadia',
}

KINDS = {
    'retail': 'Retail releases and disc revisions',
    'reissue': 'Re-releases and budget reissues',
    'patch': 'Patches',
    'title-update': 'Title updates and package versions',
    'exe-build': 'Executable builds',
    'digital': 'Digital releases',
    'mobile-version': 'Mobile app versions',
    'edition': 'Editions and offers',
    'compilation': 'Compilations',
    'dlc': 'Updates and DLC',
    'app': 'Companion apps',
    'demo': 'Demos',
    'prototype': 'Prototypes and pre-release builds',
    'leak': 'Leaks',
    'milestone': 'Development milestones',
    'cancelled': 'Cancelled projects',
    'regional': 'Regional versions and censorship',
    'rating': 'Ratings',
    'languages': 'Languages',
    'dump': 'Preserved dumps',
    'third-party': 'Third-party products',
    'media': 'Related media',
    'note': 'Notes',
    'gap': 'Known gaps',
}

# Section id -> (file, games, default kind). A subsection without its own row inherits its parent's.
SECTIONS = {
    'intro': ('series', ['series'], 'note'),
    'toc': ('series', ['series'], 'note'),
    '1': ('gta1', ['gta1'], 'note'),
    '1.1': ('gta1', ['gta1'], 'prototype'),
    '1.2': ('gta1', ['gta1'], 'demo'),
    '1.3': ('gta1', ['gta1'], 'retail'),
    '1.4': ('gta1', ['gta1'], 'retail'),
    '1.5': ('gta1', ['gta1'], 'retail'),
    '1.6': ('gta1', ['gta1'], 'reissue'),
    '1.7': ('london1969', ['london1969'], 'retail'),
    '1.8': ('london1961', ['london1961'], 'digital'),
    '1.9': ('gta1', ['gta1', 'london1969', 'london1961'], 'gap'),
    '2': ('gta2', ['gta2'], 'note'),
    '2.1': ('gta2', ['gta2'], 'prototype'),
    '2.2': ('gta2', ['gta2'], 'retail'),
    '2.3': ('gta2', ['gta2'], 'patch'),
    '2.4': ('gta2', ['gta2'], 'reissue'),
    '2.5': ('gta2', ['gta2'], 'regional'),
    '2.6': ('gta2', ['gta2'], 'media'),
    '2.7': ('gta2', ['gta2'], 'gap'),
    '3': ('gta3', ['gta3'], 'note'),
    '3.1': ('gta3', ['gta3'], 'milestone'),
    '3.2': ('gta3', ['gta3'], 'prototype'),
    '3.3': ('gta3', ['gta3'], 'retail'),
    '3.4': ('gta3', ['gta3'], 'retail'),
    '3.4a': ('gta3', ['gta3'], 'exe-build'),
    '3.5': ('gta3', ['gta3'], 'retail'),
    '3.6': ('gta3', ['gta3'], 'retail'),
    '3.7': ('gta3', ['gta3'], 'digital'),
    '3.8': ('gta3', ['gta3'], 'mobile-version'),
    '3.9': ('gta3', ['gta3'], 'regional'),
    '3.10': ('gta3', ['gta3'], 'note'),
    '3.11': ('gta3', ['gta3'], 'gap'),
    '4': ('vc', ['vc'], 'note'),
    '4.1': ('vc', ['vc'], 'milestone'),
    '4.2': ('vc', ['vc'], 'retail'),
    '4.3': ('vc', ['vc'], 'retail'),
    '4.3a': ('vc', ['vc'], 'exe-build'),
    '4.4': ('vc', ['vc'], 'retail'),
    '4.5': ('vc', ['vc'], 'retail'),
    '4.6': ('vc', ['vc'], 'mobile-version'),
    '4.7': ('vc', ['vc'], 'digital'),
    '4.8': ('vc', ['vc'], 'regional'),
    '4.9': ('vc', ['vc'], 'gap'),
    '5': ('sa', ['sa'], 'note'),
    '5.1': ('sa', ['sa'], 'prototype'),
    '5.2': ('sa', ['sa'], 'retail'),
    '5.3': ('sa', ['sa'], 'retail'),
    '5.4': ('sa', ['sa'], 'retail'),
    '5.5': ('sa', ['sa'], 'retail'),
    '5.6': ('sa', ['sa'], 'mobile-version'),
    '5.7': ('sa', ['sa'], 'retail'),
    '5.8': ('sa', ['sa'], 'digital'),
    '5.9': ('sa', ['sa'], 'regional'),
    '5.10': ('sa', ['sa'], 'gap'),
    '6': ('series', ['advance', 'lcs', 'vcs', 'ctw'], 'note'),
    '6.1': ('advance', ['advance'], 'note'),
    '6.1.1': ('advance', ['advance'], 'prototype'),
    '6.1.2': ('advance', ['advance'], 'retail'),
    '6.2': ('lcs', ['lcs'], 'note'),
    '6.2.1': ('lcs', ['lcs'], 'prototype'),
    '6.2.2': ('lcs', ['lcs'], 'retail'),
    '6.2.3': ('lcs', ['lcs'], 'retail'),
    '6.2.4': ('lcs', ['lcs'], 'digital'),
    '6.2.5': ('lcs', ['lcs'], 'mobile-version'),
    '6.3': ('vcs', ['vcs'], 'note'),
    '6.3.1': ('vcs', ['vcs'], 'prototype'),
    '6.3.2': ('vcs', ['vcs'], 'retail'),
    '6.3.3': ('vcs', ['vcs'], 'retail'),
    '6.4': ('ctw', ['ctw'], 'note'),
    '6.4.1': ('ctw', ['ctw'], 'prototype'),
    '6.4.2': ('ctw', ['ctw'], 'retail'),
    '6.4.3': ('ctw', ['ctw'], 'retail'),
    '6.4.4': ('ctw', ['ctw'], 'mobile-version'),
    '6.5': ('cancelled', ['cancelled'], 'cancelled'),
    '6.6': ('series', ['advance', 'lcs', 'vcs', 'ctw'], 'gap'),
    '7': ('gta4', ['gta4'], 'note'),
    '7.1': ('gta4', ['gta4'], 'milestone'),
    '7.2': ('gta4', ['gta4'], 'prototype'),
    '7.3': ('gta4', ['gta4'], 'note'),
    '7.4': ('gta4', ['gta4'], 'retail'),
    '7.5': ('gta4', ['gta4'], 'patch'),
    '7.6': ('eflc', ['eflc'], 'retail'),
    '7.7': ('gta4', ['gta4', 'eflc'], 'retail'),
    '7.8': ('gta4', ['gta4'], 'retail'),
    '7.9': ('eflc', ['tlad', 'tbogt', 'eflc'], 'retail'),
    '7.10': ('gta4', ['gta4'], 'compilation'),
    '7.11': ('gta4', ['gta4', 'tlad', 'tbogt', 'eflc'], 'gap'),
    '8': ('gta5', ['gta5'], 'note'),
    '8.1': ('gta5', ['gta5'], 'milestone'),
    '8.2': ('gta5', ['gta5'], 'retail'),
    '8.3': ('gta5', ['gta5'], 'retail'),
    '8.4': ('gta5', ['gta5'], 'edition'),
    '8.4b': ('gta5', ['gta5'], 'app'),
    '8.5': ('gta5', ['gta5'], 'title-update'),
    '8.5b': ('gta5', ['gta5'], 'title-update'),
    '8.6': ('gta5', ['gta5'], 'exe-build'),
    '8.7': ('gta5', ['gta5'], 'exe-build'),
    '8.8': ('gta5', ['gta5'], 'regional'),
    '8.9': ('gta5', ['gta5'], 'note'),
    '8.10': ('gta5', ['gta5'], 'gap'),
    '9': ('gta-online', ['gta-online'], 'dlc'),
    '9.A': ('gta-online', ['gta5'], 'cancelled'),
    '9.B': ('gta-online', ['gta-online'], 'note'),
    '9.C': ('gta-online', ['gta-online'], 'note'),
    '9.D': ('gta-online', ['gta-online'], 'edition'),
    '9.E': ('gta-online', ['gta-online'], 'dlc'),
    '9.F': ('gta-online', ['gta-online'], 'gap'),
    '10': ('trilogy-de', ['trilogy-de'], 'note'),
    '10.1': ('trilogy-de', ['trilogy-de'], 'milestone'),
    '10.2': ('trilogy-de', ['trilogy-de'], 'retail'),
    '10.3': ('trilogy-de', ['trilogy-de'], 'title-update'),
    '10.3a': ('trilogy-de', ['trilogy-de'], 'title-update'),
    '10.4': ('trilogy-de', ['trilogy-de', 'gta3', 'vc', 'sa'], 'reissue'),
    '10.5': ('trilogy-de', ['trilogy-de'], 'mobile-version'),
    '10.6': ('trilogy-de', ['trilogy-de'], 'gap'),
    '11': ('series', ['series'], 'note'),
    '11.1': ('gta6', ['gta6'], 'milestone'),
    '11.2': ('gta6', ['gta6'], 'edition'),
    '11.3': ('gta6', ['gta6'], 'leak'),
    '11.3a': ('gta6', ['gta6'], 'leak'),
    '11.4': ('gta6', ['gta6'], 'leak'),
    '11.5': ('gta6', ['gta6'], 'leak'),
    '11.6': ('gta5', ['gta5'], 'leak'),
    '11.7': ('series', ['series'], 'prototype'),
    '11.8': ('cancelled', ['cancelled'], 'cancelled'),
    '11.9': ('media', ['media'], 'media'),
    '11.10': ('series', ['series'], 'gap'),
    '12': ('collections', ['collections'], 'compilation'),
    '12.6': ('collections', ['collections'], 'reissue'),
    '13': ('regional', ['series'], 'regional'),
    '13.13': ('regional', ['series'], 'languages'),
    '13.14': ('regional', ['series'], 'rating'),
    '13.15': ('regional', ['series'], 'gap'),
    '14': ('audit', ['series'], 'note'),
    'A': ('dumps', ['series'], 'dump'),
    'A.20': ('dumps', ['series'], 'third-party'),
}
FILE_TITLES = {
    'series': 'The whole series: how the research is organised, and series-wide sections',
    'regional': 'Country-specific releases, censorship, localisation and ratings (research section 13)',
    'audit': 'Completeness audit notes of the research (section 14)',
    'dumps': 'Preserved retail disc and cartridge dumps (research appendix A)',
}

# A "Game" cell -> game ids (tables in sections 11.7, 13 and 10.3a list several games).
GAME_CELLS = {
    "GTA (1997)": ['gta1'], "GTA (Race'n'Chase)": ['gta1'], "GTA / GTA 2": ['gta1', 'gta2'],
    "GTA: London 1969": ['london1969'], "GTA 2": ['gta2'],
    "GTA, London 1969, GTA 2, GTA III": ['gta1', 'london1969', 'gta2', 'gta3'],
    "GTA III": ['gta3'], "GTA III (2001 PC)": ['gta3'], "GTA III 10th Anniversary": ['gta3'],
    "GTA III (GBA) / GTA Advance": ['gta3', 'advance'], "GTA III + VC Double Pack": ['gta3', 'vc'],
    "GTA III, GTA: Vice City": ['gta3', 'vc'],
    "GTA: Vice City": ['vc'], "GTA: Vice City (PC)": ['vc'], "GTA: Vice City 10th Anniv.": ['vc'],
    "GTA: Vice City 10th Anniversary": ['vc'], "GTA: Vice City, San Andreas, GTA IV": ['vc', 'sa', 'gta4'],
    "GTA: Vice City, San Andreas, Liberty City Stories, GTA IV": ['vc', 'sa', 'lcs', 'gta4'],
    "GTA: San Andreas": ['sa'], "GTA: San Andreas (PC)": ['sa'], "GTA: SA 10th Anniversary": ['sa'],
    "GTA: LCS": ['lcs'], "GTA: Liberty City Stories": ['lcs'],
    "GTA: Liberty City Stories / Vice City Stories": ['lcs', 'vcs'],
    "GTA: VCS": ['vcs'], "GTA: Vice City Stories": ['vcs'], "GTA: Chinatown Wars": ['ctw'],
    "GTA IV": ['gta4'], "GTA IV / Complete Edition": ['gta4'], "GTA IV / TBoGT": ['gta4', 'tbogt'],
    "GTA IV / TLAD": ['gta4', 'tlad'], "GTA IV: The Ballad of Gay Tony": ['tbogt'],
    "GTA IV: The Lost and Damned": ['tlad'], "GTA IV: Episodes from Liberty City": ['eflc'],
    "GTA IV: Episodes from Liberty City / Complete Edition": ['eflc', 'gta4'],
    "GTA IV: Episodes from Liberty City / The Ballad of Gay Tony": ['eflc', 'tbogt'],
    "GTA: EFLC": ['eflc'], "GTA: Episodes from Liberty City": ['eflc'], "GTA IV → whole series": ['gta4', 'series'],
    "GTA V": ['gta5'], "GTA V (Legacy, PC 13/14 Apr 2015)": ['gta5'], "GTA V Enhanced (PC, 4 Mar 2025)": ['gta5'],
    "GTA V + GTA Online": ['gta5', 'gta-online'], "GTA V, Red Dead Redemption 2 and other Rockstar titles": ['gta5'],
    "GTA V, Trilogy DE": ['gta5', 'trilogy-de'], "GTA series / GTA V": ['series', 'gta5'],
    "GTA Online (Diamond Casino)": ['gta-online'], "GTA VI": ['gta6'],
    "GTA III / VC / SA – The Definitive Edition": ['trilogy-de'], "GTA Trilogy – DE": ['trilogy-de'],
    "GTA: The Trilogy – DE": ['trilogy-de'], "GTA: The Trilogy – The Definitive Edition": ['trilogy-de'],
    "GTA III – DE": ['trilogy-de'], "GTA III – DE (0100C3C012718000)": ['trilogy-de'],
    "GTA: SA – DE": ['trilogy-de'], "GTA: Vice City – DE": ['trilogy-de'],
    "San Andreas – DE (010065A014024000)": ['trilogy-de'], "Vice City – DE (0100182014022000)": ['trilogy-de'],
    "GTA (series)": ['series'], "Any GTA": ['series'], "entire series": ['series'], "—": ['series'],
    "n/a": ['series'], "ClassInd ratings": ['series'],
}

# Dump name prefix -> game ids, longest prefix first (appendix A; as tools/gen_release_index.py TITLES).
DUMP_TITLES = sorted([
    ('Grand Theft Auto - Mission Pack 1 - London 1969', ['london1969']),
    ('Grand Theft Auto - Mission Pack #1 - London 1969', ['london1969']),
    ('Grand Theft Auto - London 1969', ['london1969']), ('Grand Theft Auto - London', ['london1969']),
    ('G.T.A - Grand Theft Auto', ['gta1']), ('Grand Theft Auto 2', ['gta2']), ('Grand Theft Auto III', ['gta3']),
    ('Grand Theft Auto - Vice City Stories', ['vcs']), ('Grand Theft Auto - Vice City', ['vc']),
    ('Grand Theft Auto - San Andreas', ['sa']), ('Grand Theft Auto - Liberty City Stories', ['lcs']),
    ('Grand Theft Auto - Chinatown Wars', ['ctw']),
    ('Grand Theft Auto IV & Episodes from Liberty City', ['gta4', 'eflc']),
    ('Grand Theft Auto IV - The Ballad of Gay Tony', ['tbogt']),
    ('Grand Theft Auto IV - The Lost and Damned', ['tlad']), ('GTA IV - Theme Pack', ['gta4']),
    ('Grand Theft Auto IV', ['gta4']), ('Grand Theft Auto - Episodes from Liberty City', ['eflc']),
    ('Grand Theft Auto V', ['gta5']), ('GTA Online', ['gta-online']),
    ('Grand Theft Auto - The Trilogy', ['gta3', 'vc', 'sa']), ('Grand Theft Auto Trylogia', ['gta3', 'vc', 'sa']),
    ('Grand Theft Auto - Double Pack', ['gta3', 'vc']), ('Grand Theft Auto Advance', ['advance']),
    ('Grand Theft Auto', ['gta1']),
], key=lambda t: -len(t[0]))
# Third-party cheat discs (A.20): the game named in the dump name.
CHEAT_GAMES = [('San Andreas', 'sa'), ('Vice City', 'vc'), ('III', 'gta3')]

# Platform words -> ids, tried in order on each part of a Platform cell (case-insensitive, whole part).
PLATFORM_WORDS = [
    (r'windows phone', 'winphone'), (r'fire os|fire phone|fire tv', 'fireos'),
    (r'(sony - )?playstation 2|ps2( dev)?', 'ps2'), (r'(sony - )?playstation 3|ps3', 'ps3'),
    (r'playstation 4|ps4', 'ps4'), (r'playstation 5|ps5', 'ps5'),
    (r'ps vita|playstation vita', 'vita'), (r'psp|playstation portable', 'psp'),
    (r'(sony - )?playstation|ps1', 'ps1'),
    (r'xbox 360|x360', 'x360'), (r'xbox one|xb1', 'xone'), (r'(xbox )?xseries', 'xsx'), (r'xbox', 'xbox'),
    (r'(nintendo )?switch', 'switch'), (r'(nintendo )?ds', 'nds'), (r'gba|game boy advance', 'gba'),
    (r'game boy color', 'gbc'), (r'dreamcast|dc', 'dc'), (r'saturn', 'saturn'), (r'n64', 'n64'),
    (r'gamecube', 'gc'), (r'palm os', 'palm'), (r'j2me', 'j2me'),
    (r'ios|ipados|ipod', 'ios'), (r'android', 'android'),
    (r'apple-silicon mac|mac os x|macos|mac', 'mac'),
    (r'pc|windows( \d.*)?|win95|ms-dos|dos|pc legacy|pc enhanced|enhanced|steam|egs|epic games store|epic|'
     r'rockstar games launcher', 'pc'),
]

MONTHS = {m: i + 1 for i, m in enumerate(
    'January February March April May June July August September October November December'.split())}
MONTHS.update({m[:3]: v for m, v in list(MONTHS.items())})
MONTHS['Sept'] = 9

# ---------------------------------------------------------------------------------------------------
# Column roles. A header cell maps to a core field; every other cell goes to `fields` verbatim.

TITLE_COLS = ['Version/Build', 'Version', 'Update name', 'Item', 'Name', 'Release', 'Demo', 'Exe build', 'Build',
              'Project', 'Milestone', 'Edition / offer', 'Label', 'App', 'App (package)', 'TU / version',
              'Release / version', 'Build / item', 'Dump name (database label)', 'Dump name', 'Audit item',
              'Item from brief', 'Event', 'Card', 'PS5 pkg', 'PS4 pkg', 'PS5 pkg (PPSA01721 EU / PPSA03420 US)',
              'PS4 pkg (CUSA00419 / CUSA00411)', 'Field', 'Section', 'Country', 'Build (GTA5.exe)',
              'Build date (per Hidden Palace)', 'Game (title ID)', 'Pack', 'Game', 'Date (2026)', 'Date', '#']
ROLES = {
    'platform': ['Platform', 'Platform(s)', 'Platforms', 'Platform / type', 'Platform/build', 'Platform / source'],
    'region': ['Region', 'Region(s)'],
    'date': ['Release date', 'Release date(s)', 'Date', 'Build / leak date', 'Build date', 'Period', 'Introduced',
             'First seen', 'Creation date', 'Creation date(s)', 'Dates first seen (ProsperoPatches)', 'Date (2026)',
             'Play "Released on"', 'Release'],
    'id': ['Serial / ID / Exe version', 'Serial / ID / Version', 'Serial / ID', 'Serial', 'ID / size', 'ID / Exe',
           'Serial(s)', 'Serial / SKU', 'Cart ID', 'Plugin-SDK ID', 'Game version / TU', 'Build name / version'],
    'notes': ['Notes', 'Notes/changes', 'Changes', 'Details', 'Detail', 'Contents', 'Contents / notes',
              'Differences / cuts / rating', 'Differences / localisation', 'What is documented', 'Key content',
              'Notes / associated update', 'Detail as given in [70]', 'Details (dates/IDs)', 'Outcome', 'Status',
              'Detail', 'Clips from that date named in [70] → what the document says they show'],
    'confidence': ['Conf.'],
    'sources': ['Src', 'Src / see'],
}


def roles_of(header):
    """Header cells -> role per column (core field name or None for `fields`)."""
    out = [None] * len(header)
    taken = set()
    for want in TITLE_COLS:
        if want in header and 'title' not in taken:
            i = header.index(want)
            out[i] = 'title'
            taken.add('title')
    # A "Game" column names the games, unless it is the title (no other title column).
    for i, h in enumerate(header):
        if out[i] is None and h in ('Game', 'Game (title ID)'):
            out[i] = 'game'
    for role, names in ROLES.items():
        for name in names:
            if role in taken:
                break
            if name in header:
                i = header.index(name)
                if out[i] is None:
                    out[i] = role
                    taken.add(role)
    return out


# ---------------------------------------------------------------------------------------------------
# Markdown parsing.

def split_cells(line):
    return [c.strip() for c in re.split(r'(?<!\\)\|', line.strip())[1:-1]]


def is_separator(cells):
    return all(re.fullmatch(r':?-{3,}:?', c) for c in cells)


HEAD = re.compile(r'^(#{2,4}) (.*)$')
SOURCE_LINE = re.compile(r'^\s*(?:(\d+)\. |- (S\d+[a-z]?) — |- \[(A\d+)\] )(.*)$')


def section_id(title, parent, used):
    m = re.match(r'^(\d+\.[A-Z]|[A-Z]\.\d+|\d+(?:\.\d+)*[a-z]?)\.? ', title + ' ')
    if re.match(r'^\d{4}\b', title):
        m = None  # "2013 — Launch year": a year, not a section number
    if title.startswith('Appendix A'):
        sid = 'A'
    elif title.startswith('How to read'):
        sid = 'intro'
    elif title.startswith('Table of contents'):
        sid = 'toc'
    elif m:
        sid = m.group(1)
    else:
        slug = re.sub(r'[^a-z0-9]+', '-', title.lower()).strip('-')
        m2 = re.match(r'^(\d{4})\b', title)
        sid = f'{parent}.{m2.group(1) if m2 else slug}'
    base, n = sid, 2
    while sid in used:
        sid = f'{base}-{n}'
        n += 1
    used.add(sid)
    return sid


def lookup(sid):
    """SECTIONS row of a section id or its nearest ancestor."""
    s = sid
    while True:
        if s in SECTIONS:
            return SECTIONS[s]
        if '.' not in s:
            raise SystemExit(f'no SECTIONS row for {sid}')
        s = s.rsplit('.', 1)[0]


def top_of(sid):
    return 'A' if sid.startswith('A') else sid.split('.')[0]


def parse(md_lines):
    """Sections (with prose and table rows) and source lists, in file order."""
    sections, sources = [], {}
    used, stack = set(), []
    cur, src_top = None, None
    table = None  # (header, roles) of the table being read
    for no, line in enumerate(md_lines, 1):
        h = HEAD.match(line)
        if h:
            table = None
            level, title = len(h.group(1)), h.group(2).strip()
            sm = re.match(r'^Sources \((Section (\d+)|Appendix A)\)$', title)
            if sm:
                src_top = sm.group(2) or 'A'
                cur = None
                continue
            src_top = None
            while stack and stack[-1][0] >= level:
                stack.pop()
            parent = stack[-1][1] if stack else ''
            sid = section_id(title, parent, used)
            stack.append((level, sid))
            cur = dict(id=sid, title=title, level=level, line=no, prose=[], rows=[])
            sections.append(cur)
            continue
        if src_top is not None:
            m = SOURCE_LINE.match(line)
            if m:
                num = m.group(1) or m.group(2) or m.group(3)
                key = f'{src_top}-{num}'
                if key in sources:
                    raise SystemExit(f'duplicate source {key} at line {no}')
                sources[key] = dict(text=m.group(4).strip(), line=no)
            elif line.strip():
                # Prose inside a source list (e.g. "Sources and method") stays with the list.
                sources.setdefault(f'{src_top}-notes', dict(text='', line=no))
                sources[f'{src_top}-notes']['text'] += line.rstrip() + '\n'
            continue
        if cur is None:
            continue
        if line.startswith('|'):
            cells = split_cells(line)
            if table is None:
                table = (cells, roles_of(cells))
                continue
            if is_separator(cells):
                continue
            header, roles = table
            if len(cells) != len(header):
                raise SystemExit(f'line {no}: {len(cells)} cells, header has {len(header)}')
            cur['rows'].append(dict(line=no, header=header, roles=roles, cells=cells))
            continue
        table = None
        cur['prose'].append(line)
    return sections, sources


# ---------------------------------------------------------------------------------------------------
# Normalised fields.

report = defaultdict(list)


def iso_date(cell):
    # The research bolds the date it settles on: "**30 Jun 1998** (Wikipedia, TCRF). PSXDC's ... is ruled out".
    bold = {d for d in (iso_date(b) for b in re.findall(r'\*\*([^*]+)\*\*', cell)) if d}
    if len(bold) == 1:
        return bold.pop()
    t = re.sub(r'\*\*|\*', '', cell).strip()
    # One date followed only by a comment in parentheses: "30 Apr 1999 (GameSpot, 30 Apr 1999)".
    m = re.fullmatch(r'([^()]+?) \((?:[^()]|\([^()]*\))*\)', t)
    if m and iso_date(m.group(1)):
        return iso_date(m.group(1))
    if re.fullmatch(r'\d{4}(-\d{2}(-\d{2})?)?', t):
        return t
    m = re.fullmatch(r'(\d{1,2}) ([A-Z][a-z]+)\.? (\d{4})', t)
    if m and m.group(2) in MONTHS:
        return f'{m.group(3)}-{MONTHS[m.group(2)]:02d}-{int(m.group(1)):02d}'
    m = re.fullmatch(r'([A-Z][a-z]+)\.? (\d{1,2}),? (\d{4})', t)
    if m and m.group(1) in MONTHS:
        return f'{m.group(3)}-{MONTHS[m.group(1)]:02d}-{int(m.group(2)):02d}'
    m = re.fullmatch(r'([A-Z][a-z]+)\.? (\d{4})', t)
    if m and m.group(1) in MONTHS:
        return f'{m.group(2)}-{MONTHS[m.group(1)]:02d}'
    return ''


SERIAL = re.compile(
    r'\b(?:[A-Z]{4}-\d{5}|(?:CUSA|PPSA|NP[A-Z]{2}|BL[A-Z]{2}|BC[A-Z]{2}|UL[A-Z]{2}|UC[A-Z]{2})\d{5}|'
    r'TT-\d{3,4}|(?:NTR|AGB|DMG|CGB|TWL|CTR)-[A-Z0-9]{4}(?:-[A-Z]{3})?|0100[0-9A-F]{12})\b')


def serials_of(cell):
    out = []
    for s in SERIAL.findall(cell):
        if s not in out:
            out.append(s)
    return out


def version_of(*cells):
    for cell in cells:
        found = set(re.findall(r'(?<![\w.])(?:v|ver\.? |VER |version )(\d+(?:\.\d+)+[a-z]?)\b', cell, re.I))
        if len(found) == 1:
            return found.pop()
    return ''


def confidence_of(cell):
    marks = {'✅': 'confirmed', '⚠️': 'single-source', '⚠': 'single-source', '❓': 'unverified'}
    found = {v for k, v in marks.items() if k in cell}
    if not found:
        return ''
    return found.pop() if len(found) == 1 else 'mixed'


def platforms_of(cell):
    t = re.sub(r'\*\*', '', cell)
    t = re.sub(r'\([^()]*\)', '', t)
    t = re.sub(r'(?i)xbox series x\s*(\\\||/)\s*s|xsx\s*(\\\||/)\s*s|\bxsx\b|series x', 'xseries', t)
    t = t.replace('\\|', '/')
    out, unknown = [], []
    for part in re.split(r',|/|;|\+|→|\band\b|\bthen\b|\balso\b|\.', t):
        part = part.strip().strip('*').strip()
        part = re.sub(r'(?i)\b(only|later|digital|first ed|home console|context|as labelled)\b', '', part).strip()
        if not part or re.fullmatch(r'\d+', part):
            continue
        if re.fullmatch(r'(?i)[—–-]|all(?![a-z]).*|various|consoles|mobile|n/a', part):
            continue  # every platform, or none named
        for pat, pid in PLATFORM_WORDS:
            # The part starts with the platform name ("Xbox 360 Test Kit", "PS2 v1.03", "Steam app 12100").
            if re.match(rf'(?:{pat})(?![\w-])', part, re.I):
                if pid not in out:
                    out.append(pid)
                break
        else:
            unknown.append(part)
    return out, unknown


def source_keys(cell, top, sources):
    keys, missing = [], []
    for tok in re.findall(r'(?<![§\w])\[?(S\d+[a-z]?|A\d+|\d+[a-z]?)\]?(?![\w.])', re.sub(r'\(§[^)]*\)|§[\d.]+', '', cell)):
        key = f'{top}-{tok}'
        if key in sources:
            if key not in keys:
                keys.append(key)
        else:
            missing.append(tok)
    return keys, missing


def games_of_dump(name, sid):
    if name.startswith('Advance GTA'):
        return ['series']  # an unrelated racing game (research section 6.1: "Disambiguation")
    if name.startswith('GTA Online '):
        return ['gta-online']  # "GTA Online Beach Bum (World) (Title Update)"
    if ' + Grand Theft Auto + ' in name:
        return ['gta1']  # "Pro Foot Contest '98 + Grand Theft Auto + Courier Crisis", a demo sampler
    if sid == 'A.20':
        return [g for w, g in CHEAT_GAMES if w in name][:1] or ['series']
    for prefix, ids in DUMP_TITLES:
        if name == prefix or name.startswith(prefix + ' (') or name.startswith(prefix + ' - ') or \
                name.startswith(prefix + ':'):
            return ids
    return None


def entry_of(sec, row, n, sources):
    file_id, games, kind = lookup(sec['id'])
    top = top_of(sec['id'])
    e = dict(id=f'{sec["id"]}-{n}', origin=f'md:{row["line"]}', section=sec['id'], kind=kind)
    fields = {}
    target = {'title': 'title', 'platform': 'platform_text', 'region': 'region', 'date': 'date_text',
              'id': 'id_text', 'notes': 'notes', 'confidence': 'confidence_text', 'sources': 'sources_text',
              'game': 'game_text'}
    for h, role, cell in zip(row['header'], row['roles'], row['cells']):
        if role is None or target[role] in e:
            if cell:
                fields[h] = cell
        else:
            e[target[role]] = cell
    if 'game_text' in e:
        g = GAME_CELLS.get(re.sub(r'\*\*', '', e['game_text']).strip())
        if g is None:
            raise SystemExit(f'line {row["line"]}: no GAME_CELLS row for {e["game_text"]!r}')
        games = g
    if kind in ('dump', 'third-party'):
        g = games_of_dump(e.get('title', ''), sec['id'])
        if g is None:
            report['dump names matching no game'].append(f'md:{row["line"]} {e.get("title")}')
        else:
            games = g
    e['games'] = games
    if 'platform_text' in e:
        e['platforms'], unknown = platforms_of(e['platform_text'])
        if unknown:
            report['platform words not mapped (kept in platform_text)'].append(
                f'md:{row["line"]} {e["platform_text"]!r}: {unknown}')
    elif kind == 'dump':
        plat = {'A.1': ['x360'], 'A.2': ['x360'], 'A.3': ['x360'], 'A.4': ['gba'], 'A.5': ['gbc'], 'A.6': ['nds'],
                'A.7': ['ps3'], 'A.8': ['psp'], 'A.9': ['psp'], 'A.10': ['psp'], 'A.11': ['psp'], 'A.12': ['vita'],
                'A.13': ['x360'], 'A.14': ['xbox'], 'A.15': ['dc'], 'A.16': ['ps2'], 'A.17': ['ps3'], 'A.18': ['psp'],
                'A.19': ['ps1']}.get(sec['id'])
        if plat:
            e['platforms'] = plat  # the appendix heading names the platform of every row
    if 'date_text' in e:
        d = iso_date(e['date_text'])
        if d:
            e['date'] = d
        elif e['date_text'] not in ('', '—', '-', '–', 'not documented'):
            report['dates kept as text only'].append(f'md:{row["line"]} {e["date_text"][:90]!r}')
    s = serials_of(e.get('id_text', '')) or (serials_of(e.get('title', '')) if kind == 'dump' else [])
    if kind == 'dump' and 'Serial(s)' in row['header']:
        s = serials_of(row['cells'][row['header'].index('Serial(s)')]) or s
    if s:
        e['serials'] = s
    v = version_of(e.get('id_text', ''), e.get('title', ''))
    if v:
        e['version'] = v
    if 'confidence_text' in e:
        c = confidence_of(e['confidence_text'])
        if c:
            e['confidence'] = c
    if 'sources_text' in e:
        keys, missing = source_keys(e['sources_text'], top, sources)
        if keys:
            e['sources'] = keys
        if missing:
            report['source references not in the section source list'].append(
                f'md:{row["line"]} {e["sources_text"]!r}: {missing}')
        if not keys:
            report['rows without a resolvable source'].append(f'md:{row["line"]} {e.get("title", "")[:80]!r}')
    if fields:
        e['fields'] = fields
    return file_id, e


# ---------------------------------------------------------------------------------------------------
# TOML output (a small writer: tomllib can only read).

def q(s):
    if '\n' in s and "'''" not in s and not s.startswith('\n'):
        return "'''\n" + s + "'''" if not s.endswith("'") else json.dumps(s, ensure_ascii=False)
    return json.dumps(s, ensure_ascii=False)


def key(k):
    return k if re.fullmatch(r'[A-Za-z0-9_-]+', k) else json.dumps(k, ensure_ascii=False)


def value(v):
    if isinstance(v, str):
        return q(v)
    if isinstance(v, bool):
        return 'true' if v else 'false'
    if isinstance(v, int):
        return str(v)
    if isinstance(v, list):
        return '[' + ', '.join(value(x) for x in v) + ']'
    if isinstance(v, dict):
        return '{ ' + ', '.join(f'{key(k)} = {value(x)}' for k, x in v.items()) + ' }'
    raise TypeError(v)


ORDER = ['id', 'origin', 'section', 'kind', 'games', 'title', 'platforms', 'platform_text', 'region', 'date',
         'date_text', 'version', 'serials', 'id_text', 'notes', 'confidence', 'confidence_text', 'sources',
         'sources_text', 'game_text']


def write_file(path, title, sections, entries, sources):
    out = [f'# {title}',
           '# Imported from docs/research/GTA_Builds_Versions_Database.md by tools/db/import_research_md.py.',
           '# This file is the source of truth now: edit it by hand (see docs/DATABASE.md).', '']
    for s in sections:
        out.append('[[section]]')
        for k in ('id', 'origin', 'title', 'games', 'text'):
            if s.get(k):
                out.append(f'{key(k)} = {value(s[k])}')
        out.append('')
    for e in entries:
        out.append('[[entry]]')
        for k in ORDER:
            if k in e and e[k] not in ('', []):
                out.append(f'{key(k)} = {value(e[k])}')
        if e.get('fields'):
            out.append('[entry.fields]')
            for k, v in e['fields'].items():
                out.append(f'{key(k)} = {value(v)}')
        out.append('')
    if sources:
        out.append('[sources]')
        for k, s in sources.items():
            out.append(f'{key(k)} = {value(s)}')
        out.append('')
    text = '\n'.join(out)
    tomllib.loads(text)  # must parse
    path.write_text(text, encoding='utf-8', newline='\n')


def main():
    md_path, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
    report_path = Path(sys.argv[3]) if len(sys.argv) > 3 else None
    md_lines = md_path.read_text(encoding='utf-8').split('\n')
    sections, raw_sources = parse(md_lines)
    sources = {}
    for k, s in raw_sources.items():
        url = re.search(r'https?://[^\s;)>\]]+', s['text'])
        item = dict(text=s['text'].strip())
        if url:
            item['url'] = url.group(0).rstrip('.,')
        item['origin'] = f'md:{s["line"]}'
        sources[k] = item

    files = defaultdict(lambda: dict(sections=[], entries=[], sources={}))
    counts = Counter()
    for sec in sections:
        file_id, games, _ = lookup(sec['id'])
        prose = '\n'.join(sec['prose']).strip('\n')
        files[file_id]['sections'].append(dict(id=sec['id'], origin=f'md:{sec["line"]}', title=sec['title'],
                                               games=games, text=prose))
        for n, row in enumerate(sec['rows'], 1):
            fid, e = entry_of(sec, row, n, sources)
            files[fid]['entries'].append(e)
            counts['entries'] += 1
    # Each source lives in the file of its research section's first sub-section, plus every file
    # whose entries cite it.
    for k, s in sources.items():
        top = k.split('-')[0]
        home = lookup('A' if top == 'A' else top)[0]
        users = {fid for fid, f in files.items() for e in f['entries'] if k in e.get('sources', [])}
        for fid in users or {home}:
            files[fid]['sources'][k] = s

    out_dir.mkdir(parents=True, exist_ok=True)
    titles = dict(FILE_TITLES)
    titles.update({g[0]: f'{g[1]}: builds, versions, releases and history' for g in GAMES})
    for fid, f in sorted(files.items()):
        write_file(out_dir / f'{fid}.toml', titles[fid], f['sections'], f['entries'], f['sources'])
    db = ['# The build database: games, platforms and kinds. Every other .toml file in this folder holds',
          '# [[section]], [[entry]] and [sources] items (see docs/DATABASE.md).', '',
          '[database]', 'version = "2026.10.24"', 'schema = 1', '']
    for gid, title, ef in GAMES:
        db.append('[[game]]')
        db.append(f'id = {value(gid)}')
        db.append(f'title = {value(title)}')
        if ef:
            db.append(f'eyefinder = {value(ef)}')
        db.append('')
    db.append('[platforms]')
    db += [f'{key(k)} = {value(v)}' for k, v in PLATFORMS.items()]
    db += ['', '[kinds]'] + [f'{key(k)} = {value(v)}' for k, v in KINDS.items()] + ['']
    text = '\n'.join(db)
    tomllib.loads(text)
    (out_dir / 'database.toml').write_text(text, encoding='utf-8', newline='\n')

    rows = sum(len(s['rows']) for s in sections)
    print(f'{len(sections)} sections, {rows} rows -> {counts["entries"]} entries, {len(sources)} sources, '
          f'{len(files)} files')
    if report_path:
        lines = ['# Import review report', '',
                 f'{len(sections)} sections, {rows} table rows, {len(sources)} sources.', '']
        for title, items in report.items():
            lines += [f'## {title} ({len(items)})', ''] + [f'* {i}' for i in items] + ['']
        report_path.write_text('\n'.join(lines), encoding='utf-8')


if __name__ == '__main__':
    main()
