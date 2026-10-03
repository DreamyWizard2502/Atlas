"""Generate src/theme/palettes.ts.

Usage: python scripts/gen_palettes.py src/theme/palettes.ts

Hand-picked seeds give each palette its character; derived tokens (soft tints,
tiles, marks) are mixed from them. Any foreground that misses its contrast
target is nudged in HSL lightness only (hue and saturation kept) until it
passes, so palettes stay recognisably themselves. The TypeScript unit test is
the independent check.
"""
import colorsys, json, sys

AA = 4.5
NONTEXT = 3.0
MARGIN = 0.15  # aim a little above the bar so rounding never flips a result


def hex2rgb(h):
    h = h.lstrip('#')
    return tuple(int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))


def rgb2hex(c):
    return '#' + ''.join(f'{max(0, min(255, round(v * 255))):02x}' for v in c)


def lum(h):
    def ch(v):
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    r, g, b = (ch(v) for v in hex2rgb(h))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = lum(a), lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def mix(a, b, t):
    """t of a over b."""
    ca, cb = hex2rgb(a), hex2rgb(b)
    return rgb2hex(tuple(x * t + y * (1 - t) for x, y in zip(ca, cb)))


def shift_l(h, dl):
    r, g, b = hex2rgb(h)
    hh, ll, ss = colorsys.rgb_to_hls(r, g, b)
    ll = max(0.0, min(1.0, ll + dl))
    return rgb2hex(colorsys.hls_to_rgb(hh, ll, ss))


def fix(color, against, target, direction, cap=None):
    """Move `color`'s lightness in `direction` (+1 lighter, -1 darker) until it
    reaches `target` contrast against every colour in `against`."""
    c = color
    for _ in range(400):
        if min(contrast(c, b) for b in against) >= target + MARGIN:
            return c
        nxt = shift_l(c, 0.004 * direction)
        if cap and nxt.lower() == cap or nxt == c:
            break
        c = nxt
    raise SystemExit(f'cannot reach {target} for {color} on {against}')


def hsl(h, s_, l):
    return rgb2hex(colorsys.hls_to_rgb(h / 360, l, s_))


def oklab(h):
    def lin(v):
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    r, g, b = (lin(v) for v in hex2rgb(h))
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s_ = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m, s_ = (x ** (1 / 3) for x in (l, m, s_))
    return (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s_,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s_,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s_)


def delta_e(a, b):
    return sum((x - y) ** 2 for x, y in zip(oklab(a), oklab(b))) ** 0.5 * 100


# Treemap storage categories. Chromatic hues sit outside the amber (30-60),
# green (90-170) and red (330-20) bands so meaning colors stay unambiguous;
# lightness is staggered so neighbours differ in more than hue.
CATEGORY_HUES = [
    ('catGames', 286, 0.02),
    ('catCad', 180, 0.05),
    ('catDocuments', 212, -0.03),
    ('catMedia', 322, 0.00),
    ('catCode', 244, -0.06),
    ('catArchives', 72, 0.06),
]
CATS = ['catGames', 'catCad', 'catDocuments', 'catMedia', 'catCode', 'catArchives', 'catTemp', 'catOther']


def categories(t, dark):
    """Pick a lightness per category tile that keeps text and secondary text at
    AA and makes the closest pair of tiles as different as possible
    (coordinate descent over the allowed lightness band)."""
    away = -1 if dark else 1  # direction that raises text contrast on a tile
    sat = 0.62 if dark else 0.85
    lo, hi = (0.14, 0.40) if dark else (0.68, 0.95)
    grid = [lo + i * (hi - lo) / 13 for i in range(14)]
    bg_hue = colorsys.rgb_to_hls(*hex2rgb(t['surface']))[0] * 360
    specs = [(k, h, sat) for k, h, _ in CATEGORY_HUES] + [('catTemp', bg_hue, 0.05), ('catOther', bg_hue, 0.05)]

    semantic = [colorsys.rgb_to_hls(*hex2rgb(t[k]))[0] * 360 for k in ('amber', 'green', 'danger')]

    def hue_gap(c):
        h = colorsys.rgb_to_hls(*hex2rgb(c))[0] * 360
        return min(min(abs(h - x) % 360, 360 - abs(h - x) % 360) for x in semantic)

    def tile(spec, l):
        _, h, s_ = spec
        if s_ <= 0.1:  # neutrals have no hue to collide with
            return fix(mix(hsl(h, s_, l), t['surface'], 0.93), [t['text'], t['text2']], AA, away)
        # Tinting toward the surface can drag a hue toward a meaning color; nudge
        # the base hue away until the finished tile keeps clear of all three.
        for dh in (0, 6, -6, 12, -12, 18, -18, 24, -24):
            c = fix(mix(hsl((h + dh) % 360, s_, l), t['surface'], 0.93), [t['text'], t['text2']], AA, away)
            if hue_gap(c) >= 18:
                return c
        raise SystemExit(f"{t['id']}: can't keep {spec[0]} clear of the meaning colors")

    def score(tiles):
        return min(delta_e(tiles[a], tiles[b]) for i, a in enumerate(CATS) for b in CATS[i + 1:])

    mid = grid[len(grid) // 2]
    light = {k: mid for k, _, _ in specs}
    tiles = {spec[0]: tile(spec, light[spec[0]]) for spec in specs}
    for _ in range(5):
        for spec in specs:
            k = spec[0]
            best = (score(tiles), light[k], tiles[k])
            for l in grid:
                cand = tile(spec, l)
                trial = dict(tiles)
                trial[k] = cand
                sc = score(trial)
                if sc > best[0] + 1e-9:
                    best = (sc, l, cand)
            light[k], tiles[k] = best[1], best[2]
    return tiles


def build(p):
    dark = p['scheme'] == 'dark'
    up = 1 if dark else -1  # direction that increases text contrast
    t = dict(p)
    t.setdefault('borderStrong', mix(t['text'], t['border'], 0.10))
    # Soft tints sit on the card surface.
    t['accentSoft'] = mix(t['accent'], t['surface'], 0.16 if dark else 0.11)
    t['amberSoft'] = mix(t['amberMark'], t['surface'], 0.14 if dark else 0.16)
    t['greenSoft'] = mix(t['greenMark'], t['surface'], 0.14 if dark else 0.13)

    for _ in range(6):
        surfaces = [t['bg'], t['surface'], t['surface2']]
        texty = surfaces + [t['accentSoft'], t['greenSoft']]
        for k in ('text', 'text2', 'text3'):
            t[k] = fix(t[k], texty, AA, up, cap='#ffffff' if dark else None)
        t['accent'] = fix(t['accent'], surfaces + [t['accentSoft']], AA, up)
        t['accentSoft'] = mix(t['accent'], t['surface'], 0.16 if dark else 0.11)
        t['accentStrong'] = shift_l(t['accent'], 0.07 * up)
        # Text on accent buttons: the dark bg in dark palettes, soft white in light.
        t['onAccent'] = t['bg'] if dark else t['surface']
        for k in ('accent', 'accentStrong'):
            t[k] = fix(t[k], [t['onAccent']], AA, up)
        t['amber'] = fix(t['amber'], surfaces + [t['amberSoft']], AA, up)
        t['green'] = fix(t['green'], surfaces + [t['greenSoft']], AA, up)
        t['danger'] = fix(t['danger'], surfaces, AA, up)
        t['cold'] = fix(t['cold'], surfaces, AA, up)
        t['onSuccess'] = t['bg'] if dark else t['surface']
        t['greenMark'] = fix(t['greenMark'], [t['onSuccess']], AA, up)
        t['amberMark'] = fix(t['amberMark'], [t['surface']], NONTEXT, up)
        t['amberSoft'] = mix(t['amberMark'], t['surface'], 0.14 if dark else 0.16)
        t['greenSoft'] = mix(t['greenMark'], t['surface'], 0.14 if dark else 0.13)

    t.update(categories(t, dark))

    # Scrim and shadow are tinted from the palette, never pure black.
    if dark:
        r, g, b = (round(v * 255) for v in hex2rgb(mix(t['bg'], '#000000', 0.55)))
        t['overlay'] = f'rgba({r}, {g}, {b}, 0.66)'
        t['shadow'] = f'0 8px 28px rgba({r}, {g}, {b}, 0.5)'
    else:
        r, g, b = (round(v * 255) for v in hex2rgb(t['text']))
        t['overlay'] = f'rgba({r}, {g}, {b}, 0.36)'
        t['shadow'] = f'0 8px 28px rgba({r}, {g}, {b}, 0.09)'
    return t


DARK_SEM = dict(amber='#e8b04a', amberMark='#c98a1a', green='#63cf9b', greenMark='#3fbf86', danger='#f07c7c', cold='#8f8e8a')
LIGHT_SEM = dict(amber='#8a5300', amberMark='#d99a12', green='#146c45', greenMark='#178452', danger='#b42318', cold='#6a6965')

PALETTES = [
    # ---- dark ----
    dict(id='graphite', name='Graphite', scheme='dark', mood='Neutral soft charcoal', bg='#18181a', surface='#1f1f22', surface2='#27272b', border='#303035', text='#e6e6e3', text2='#b8b7b1', text3='#8e8d88', accent='#5b9cf0', **DARK_SEM),
    dict(id='midnight', name='Midnight', scheme='dark', mood='Deep navy', bg='#0f1626', surface='#151e31', surface2='#1c263c', border='#27334b', text='#e1e7f2', text2='#adb8cc', text3='#8590a9', accent='#6cc4f5', **{**DARK_SEM, 'cold': '#8b93a3'}),
    dict(id='forest', name='Forest', scheme='dark', mood='Dark green-gray', bg='#141a17', surface='#1a221e', surface2='#212a25', border='#2b3630', text='#e0e8e2', text2='#b0beb4', text3='#86968b', accent='#9cc5a1', **{**DARK_SEM, 'green': '#7fd6a8', 'cold': '#8d958f'}),
    dict(id='ember', name='Ember', scheme='dark', mood='Warm dark brown-gray', bg='#1c1715', surface='#241e1b', surface2='#2c2521', border='#382e29', text='#ece4de', text2='#c1b4ab', text3='#988a80', accent='#e8894f', **{**DARK_SEM, 'amber': '#efc05a', 'danger': '#f58a95', 'cold': '#958d88'}),
    dict(id='plum', name='Plum', scheme='dark', mood='Dark purple-gray', bg='#1a1520', surface='#221b29', surface2='#2a2232', border='#362c40', text='#eae4f0', text2='#beb3ca', text3='#9589a2', accent='#b9a2f0', **{**DARK_SEM, 'cold': '#928c98'}),
    dict(id='slate', name='Slate', scheme='dark', mood='Cool blue-gray', bg='#161c22', surface='#1c242c', surface2='#232c35', border='#2d3741', text='#e1e8ed', text2='#b1bdc7', text3='#86939f', accent='#4fc1b5', **{**DARK_SEM, 'cold': '#8c949b'}),
    dict(id='mocha', name='Mocha', scheme='dark', mood='Coffee brown', bg='#1d1814', surface='#26201b', surface2='#2e2721', border='#3a3029', text='#ede5db', text2='#c5b7a7', text3='#9b8c7c', accent='#d39a6a', **{**DARK_SEM, 'amber': '#e9cf5a', 'amberMark': '#c9a61c', 'cold': '#958e87'}),
    dict(id='abyss', name='Abyss', scheme='dark', mood='Very dark teal', bg='#0e1a1c', surface='#132225', surface2='#192a2e', border='#22373b', text='#ddeeee', text2='#a8c2c3', text3='#7e9b9d', accent='#5fd6d6', **{**DARK_SEM, 'green': '#7ad9a0', 'cold': '#8a9696'}),
    dict(id='oxide', name='Oxide', scheme='dark', mood='Dark gray, rust-red accent', bg='#1a1919', surface='#222020', surface2='#2a2828', border='#363232', text='#eae5e3', text2='#bcb5b2', text3='#938b88', accent='#e07a4f', **{**DARK_SEM, 'danger': '#f2849b', 'cold': '#918d8b'}),
    dict(id='dusk', name='Dusk', scheme='dark', mood='Muted indigo', bg='#1b1a2b', surface='#222136', surface2='#2a2940', border='#35344d', text='#e8e6f3', text2='#bbb8d1', text3='#918da8', accent='#f0a3c4', **{**DARK_SEM, 'danger': '#f58a7c', 'cold': '#908e9c'}),
    # ---- light ----
    dict(id='paper', name='Paper', scheme='light', mood='Warm off-white', bg='#f5f4f0', surface='#fcfbf9', surface2='#efede8', border='#e2e0d9', text='#1c1b19', text2='#4e4c47', text3='#6b6963', accent='#2463c9', **LIGHT_SEM),
    dict(id='linen', name='Linen', scheme='light', mood='Cream', bg='#f6f0e5', surface='#fcf8f1', surface2='#f0e8db', border='#e3d9c8', text='#2a2219', text2='#574a3d', text3='#73655a', accent='#b0502c', **{**LIGHT_SEM, 'danger': '#a3203a'}),
    dict(id='mint', name='Mint', scheme='light', mood='Pale green-white', bg='#eff6f1', surface='#f9fcfa', surface2='#e6efe9', border='#d6e4da', text='#16231b', text2='#3f5246', text3='#5d6f63', accent='#0f7a52', **{**LIGHT_SEM, 'green': '#2f6b1c', 'greenMark': '#3d7f26'}),
    dict(id='sky', name='Sky', scheme='light', mood='Pale blue-white', bg='#eff4fb', surface='#f9fbfe', surface2='#e6edf8', border='#d6e0ef', text='#151d2b', text2='#3d4a60', text3='#5a6780', accent='#1f4fd1', **LIGHT_SEM),
    dict(id='sand', name='Sand', scheme='light', mood='Beige', bg='#f2ecdf', surface='#faf6ee', surface2='#ebe3d2', border='#ddd3be', text='#27241a', text2='#524c3c', text3='#6e6855', accent='#5c6b1e', **{**LIGHT_SEM, 'green': '#17664a', 'greenMark': '#1c7a57'}),
    dict(id='blossom', name='Blossom', scheme='light', mood='Faint pink-white', bg='#faf2f3', surface='#fdf9f9', surface2='#f4e8ea', border='#e9d9dc', text='#2a1b1f', text2='#5a454a', text3='#786267', accent='#bd355c', **{**LIGHT_SEM, 'danger': '#a83a12'}),
    dict(id='fog', name='Fog', scheme='light', mood='Cool light gray', bg='#edeff1', surface='#f7f8f9', surface2='#e5e8eb', border='#d6dbe0', text='#1b1f24', text2='#444b54', text3='#5f6670', accent='#485b76', **LIGHT_SEM),
    dict(id='citrus', name='Citrus', scheme='light', mood='Soft warm white', bg='#f9f5ea', surface='#fdfbf5', surface2='#f3ecdc', border='#e6dcc5', text='#28221a', text2='#564b3c', text3='#73675a', accent='#c2410c', **{**LIGHT_SEM, 'amber': '#6f5c00', 'amberMark': '#c9aa22', 'danger': '#a51d45'}),
    dict(id='lilac', name='Lilac', scheme='light', mood='Pale lavender', bg='#f4f1fa', surface='#fbfafd', surface2='#ece8f6', border='#dfd8ee', text='#1f1a2b', text2='#4b4360', text3='#686080', accent='#6a3dcc', **LIGHT_SEM),
    dict(id='stone', name='Stone', scheme='light', mood='Neutral light gray', bg='#eeeeec', surface='#f7f7f5', surface2='#e6e6e3', border='#d8d8d4', text='#1d1d1c', text2='#474745', text3='#636360', accent='#3a3d42', **LIGHT_SEM),
]

ORDER = ['bg', 'surface', 'surface2', 'border', 'borderStrong', 'text', 'text2', 'text3',
         'accent', 'accentStrong', 'accentSoft', 'onAccent', *CATS,
         'amber', 'amberMark', 'amberSoft', 'green', 'greenMark', 'greenSoft', 'onSuccess',
         'danger', 'cold', 'overlay', 'shadow']

out = []
changes = []
for p in PALETTES:
    t = build(p)
    for k in ('text', 'text2', 'text3', 'accent', 'amber', 'green', 'danger', 'cold'):
        if p.get(k) and p[k].lower() != t[k].lower():
            changes.append(f"{p['id']}.{k}: {p[k]} -> {t[k]}")
    out.append(t)

lines = [
    '// Generated by scripts/gen_palettes.py (python scripts/gen_palettes.py src/theme/palettes.ts).',
    '// 10 dark and 10 light palettes. Every text/background pair used by the UI',
    '// meets WCAG AA (enforced by palettes.test.ts). Colors carry meaning only: one',
    '// accent per palette; amber = needs attention, green = reclaimable/cleaned,',
    '// danger = destructive, cold = muted gray for cold files.',
    '',
    'export type Scheme = "light" | "dark";',
    '',
    'export interface Tokens {',
] + [f'  {k}: string;' for k in ORDER] + [
    '}',
    '',
    'export interface Palette {',
    '  id: string;',
    '  name: string;',
    '  scheme: Scheme;',
    '  mood: string;',
    '  tokens: Tokens;',
    '}',
    '',
    'export const PALETTES: Palette[] = [',
]
for t in out:
    lines.append('  {')
    lines.append(f'    id: "{t["id"]}",')
    lines.append(f'    name: "{t["name"]}",')
    lines.append(f'    scheme: "{t["scheme"]}",')
    lines.append(f'    mood: "{t["mood"]}",')
    lines.append('    tokens: {')
    for k in ORDER:
        lines.append(f'      {k}: "{t[k]}",')
    lines.append('    },')
    lines.append('  },')
lines += [
    '];',
    '',
    'export const DEFAULT_DARK = "graphite";',
    'export const DEFAULT_LIGHT = "paper";',
    '',
    'export function paletteById(id: string | null | undefined, scheme: Scheme): Palette {',
    '  const fallback = scheme === "dark" ? DEFAULT_DARK : DEFAULT_LIGHT;',
    '  return (',
    '    PALETTES.find((p) => p.id === id && p.scheme === scheme) ??',
    '    PALETTES.find((p) => p.id === fallback)!',
    '  );',
    '}',
    '',
]
target = sys.argv[1]
open(target, 'w', encoding='utf-8', newline='\n').write('\n'.join(lines))
print(f'wrote {len(out)} palettes to {target}')
print('closest pair of treemap categories (OKLab dE x100) per palette:')
for t in out:
    d, a, b = min((delta_e(t[x], t[y]), x, y) for i, x in enumerate(CATS) for y in CATS[i + 1:])
    print(f"   {t['id']:9} {d:5.1f}  ({a} vs {b})")
print('lightness nudges applied:')
for c in changes:
    print('  ', c)
