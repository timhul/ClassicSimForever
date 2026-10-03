"""Downloads the icons of the exported spells and items into `data/icons/` as PNG files.

Run from anywhere: `python tools/fetch_icons.py [options]`. Collects every non-zero `icon:`
(a texture `FileDataID`: `SpellMisc.SpellIconFileDataID` / `Item.IconFileDataID`) of
`data/spells/*.yaml` and `data/items/*.yaml`, downloads each missing one from
https://wago.tools/api/casc/<FileDataID> (the client's BLP2 texture), decodes it and writes
`data/icons/<FileDataID>.png`. `csim-live` serves the directory. Its page loads icons from
Wowhead's CDN by `icon_name` first, so the cache is only needed offline or for an icon without
a name (a texture only Forever has).

The icons are Blizzard's assets: `data/icons/` is a local cache, ignored by git like
`data/tables/`. Run again after a re-export to fetch the new ones.

Only the standard library is used; BLP2 is decoded here (palettized, DXT1 / DXT3 / DXT5 and
uncompressed BGRA, the first mipmap only).
"""
import argparse
import concurrent.futures
import glob
import os
import re
import struct
import sys
import time
import urllib.error
import urllib.request
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
DEFAULT_DATA = os.path.join(ROOT, "data")

CASC = "https://wago.tools/api/casc"
USER_AGENT = "ClassicSimForever-fetch-icons/1.0"

# The exported `icon:` lines; the data files are written by csim-tables, one field per line.
ICON_LINE = re.compile(r"^\s*icon:\s*(\d+)\s*$", re.M)


def get(url, retries=3):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for attempt in range(1, retries + 1):
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            # A 404 will not go away by asking again.
            if e.code == 404 or attempt == retries:
                raise
        except (urllib.error.URLError, TimeoutError):
            if attempt == retries:
                raise
        time.sleep(2 * attempt)


def icon_ids(data_dir):
    """Every non-zero icon of the exported spells and items."""
    ids = set()
    for pattern in ("spells/*.yaml", "items/*.yaml"):
        for path in glob.glob(os.path.join(data_dir, pattern)):
            with open(path, encoding="utf-8") as f:
                ids.update(int(m.group(1)) for m in ICON_LINE.finditer(f.read()))
    ids.discard(0)
    return sorted(ids)


# --- BLP2 ---------------------------------------------------------------------------------

def rgb565(c):
    r, g, b = (c >> 11) & 31, (c >> 5) & 63, c & 31
    return (r * 255 + 15) // 31, (g * 255 + 31) // 63, (b * 255 + 15) // 31


def dxt_colors(data, off, dxt1):
    """The 16 RGBA texels of a DXT color block; DXT1's three-color mode has transparency."""
    c0, c1, bits = struct.unpack_from("<HHI", data, off)
    p0, p1 = rgb565(c0), rgb565(c1)
    if c0 > c1 or not dxt1:
        p2 = tuple((2 * a + b) // 3 for a, b in zip(p0, p1))
        p3 = tuple((a + 2 * b) // 3 for a, b in zip(p0, p1)) + (255,)
    else:
        p2 = tuple((a + b) // 2 for a, b in zip(p0, p1))
        p3 = (0, 0, 0, 0)
    palette = [p0 + (255,), p1 + (255,), p2 + (255,), p3]
    return [palette[(bits >> (2 * i)) & 3] for i in range(16)]


def dxt3_alpha(data, off):
    bits = int.from_bytes(data[off:off + 8], "little")
    return [((bits >> (4 * i)) & 15) * 17 for i in range(16)]


def dxt5_alpha(data, off):
    a0, a1 = data[off], data[off + 1]
    bits = int.from_bytes(data[off + 2:off + 8], "little")
    if a0 > a1:
        lut = [a0, a1] + [((6 - i) * a0 + (i + 1) * a1) // 7 for i in range(6)]
    else:
        lut = [a0, a1] + [((4 - i) * a0 + (i + 1) * a1) // 5 for i in range(4)] + [0, 255]
    return [lut[(bits >> (3 * i)) & 7] for i in range(16)]


def decode_blp(data):
    """(width, height, rows of RGBA tuples) of the first mipmap of a BLP2 texture."""
    if data[:4] != b"BLP2":
        raise ValueError(f"not a BLP2 texture (starts with {data[:4]!r})")
    _, _, compression, alpha_depth, alpha_type, _, width, height = struct.unpack_from(
        "<4sIBBBBII", data, 0)
    offset = struct.unpack_from("<I", data, 20)[0]
    size = struct.unpack_from("<I", data, 84)[0]
    texels = [[(0, 0, 0, 255)] * width for _ in range(height)]

    if compression == 2:
        if alpha_depth <= 1:
            block, alpha = 8, None
        elif alpha_type == 1:
            block, alpha = 16, dxt3_alpha
        elif alpha_type == 7:
            block, alpha = 16, dxt5_alpha
        else:
            raise ValueError(f"unknown DXT alpha type {alpha_type}")
        blocks_wide = max(1, width // 4)
        for by in range(max(1, height // 4)):
            for bx in range(blocks_wide):
                at = offset + (by * blocks_wide + bx) * block
                if alpha is None:
                    colors = dxt_colors(data, at, dxt1=True)
                    if alpha_depth == 0:
                        colors = [c[:3] + (255,) for c in colors]
                else:
                    colors = [c[:3] + (a,) for c, a in
                              zip(dxt_colors(data, at + 8, dxt1=False), alpha(data, at))]
                for i, color in enumerate(colors):
                    y, x = by * 4 + i // 4, bx * 4 + i % 4
                    if y < height and x < width:
                        texels[y][x] = color
    elif compression == 1:
        palette = [struct.unpack_from("<BBBB", data, 148 + 4 * i) for i in range(256)]
        count = width * height
        indices = data[offset:offset + count]
        alphas = data[offset + count:offset + size]
        for i, index in enumerate(indices):
            b, g, r, _ = palette[index]
            if alpha_depth == 8:
                a = alphas[i]
            elif alpha_depth == 4:
                a = ((alphas[i // 2] >> (4 * (i % 2))) & 15) * 17
            elif alpha_depth == 1:
                a = 255 if alphas[i // 8] >> (i % 8) & 1 else 0
            else:
                a = 255
            texels[i // width][i % width] = (r, g, b, a)
    elif compression == 3:
        for i in range(width * height):
            b, g, r, a = data[offset + 4 * i:offset + 4 * i + 4]
            texels[i // width][i % width] = (r, g, b, a)
    else:
        raise ValueError(f"unknown BLP2 compression {compression}")
    return width, height, texels


def png(width, height, texels):
    raw = b"".join(b"\0" + bytes(v for texel in row for v in texel) for row in texels)

    def chunk(kind, body):
        return (struct.pack(">I", len(body)) + kind + body
                + struct.pack(">I", zlib.crc32(kind + body)))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


# --- Fetching -----------------------------------------------------------------------------

def fetch(icon, directory, force):
    path = os.path.join(directory, f"{icon}.png")
    if os.path.exists(path) and not force:
        return "present"
    body = get(f"{CASC}/{icon}")
    image = png(*decode_blp(body))
    tmp = path + ".part"
    with open(tmp, "wb") as f:
        f.write(image)
    os.replace(tmp, path)
    return "fetched"


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--data", default=DEFAULT_DATA, help="data directory (default: data)")
    p.add_argument("--force", action="store_true", help="re-download icons that already exist")
    p.add_argument("--jobs", type=int, default=8, help="parallel downloads (default: %(default)s)")
    p.add_argument("--dry-run", action="store_true", help="count the icons, download nothing")
    args = p.parse_args()

    icons = icon_ids(args.data)
    directory = os.path.join(args.data, "icons")
    print(f"{len(icons)} icons -> {directory}")
    if args.dry_run:
        return

    os.makedirs(directory, exist_ok=True)
    counts = {"fetched": 0, "present": 0}
    failed = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        jobs = {pool.submit(fetch, icon, directory, args.force): icon for icon in icons}
        for future in concurrent.futures.as_completed(jobs):
            icon = jobs[future]
            try:
                counts[future.result()] += 1
            except Exception as e:  # noqa: BLE001 - report every failure at the end
                print(f"{icon}: FAILED ({e})", file=sys.stderr)
                failed.append(icon)

    print(f"{counts['fetched']} fetched, {counts['present']} already there, "
          f"{len(failed)} failed")
    if failed:
        sys.exit(f"failed: {' '.join(map(str, sorted(failed)))}")


if __name__ == "__main__":
    main()
