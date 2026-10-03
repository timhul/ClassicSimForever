"""Downloads the icon names of the community listfile into `data/tables/listfile-icons.csv`.

Run from anywhere: `python tools/fetch_listfile.py [options]`. Streams the latest
`community-listfile.csv` release of https://github.com/wowdev/wow-listfile (lowercase
`FileDataID;path` lines, no header, ~150 MB) and keeps only the textures under
`interface/icons/`, sorted by FileDataID. csim-tables reads the file to export the texture name
of each `icon:` FileDataID (`interface/icons/inv_sword_39.blp` -> `inv_sword_39`), by which
Wowhead's CDN serves the icon.

The file is written next to the table dumps, which are gitignored. Its name does not match
`<Table>.<build>.csv`, so `TableDir` and `tools/fetch_tables.py`'s old-build cleanup leave it
alone. It is written to a temporary file and renamed at the end: a failed download keeps the
previous file. Run again after a re-export, or when the game adds textures.

Only the standard library is used.
"""
import argparse
import glob
import os
import re
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
DEFAULT_DATA = os.path.join(ROOT, "data")

URL = "https://github.com/wowdev/wow-listfile/releases/latest/download/community-listfile.csv"
USER_AGENT = "ClassicSimForever-fetch-listfile/1.0"
OUT_NAME = "listfile-icons.csv"
PREFIX = "interface/icons/"

# A listfile line: `FileDataID;path`.
LINE = re.compile(r"^(\d+);(.+)$")
# The exported `icon:` lines, as tools/fetch_icons.py reads them.
ICON_LINE = re.compile(r"^\s*icon:\s*(\d+)\s*$", re.M)


def open_url(url, retries=3):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for attempt in range(1, retries + 1):
        try:
            return urllib.request.urlopen(req, timeout=120)
        except urllib.error.HTTPError as e:
            # A 404 will not go away by asking again.
            if e.code == 404 or attempt == retries:
                raise
        except (urllib.error.URLError, TimeoutError):
            if attempt == retries:
                raise
        time.sleep(2 * attempt)


def icon_rows(lines, total=None):
    """The `(FileDataID, path)` rows of `lines` (bytes) under `interface/icons/`."""
    rows = {}
    read = 0
    next_report = 16 << 20
    for number, raw in enumerate(lines, 1):
        read += len(raw)
        if read >= next_report:
            of = f" of {total >> 20} MB" if total else " MB"
            print(f"  {read >> 20}{of}", flush=True)
            next_report += 16 << 20
        line = raw.decode("utf-8").strip()
        if not line:
            continue
        m = LINE.match(line)
        if not m:
            raise ValueError(f"line {number}: not `FileDataID;path`: {line[:80]!r}")
        path = m.group(2)
        if path.lower().startswith(PREFIX):
            rows[int(m.group(1))] = path
    return rows


def exported_icons(data):
    ids = set()
    for pattern in ("spells/*.yaml", "items/*.yaml"):
        for path in glob.glob(os.path.join(data, pattern)):
            with open(path, encoding="utf-8") as f:
                ids.update(int(i) for i in ICON_LINE.findall(f.read()))
    ids.discard(0)
    return ids


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--data", default=DEFAULT_DATA, help="data directory (default: data)")
    p.add_argument("--url", default=URL, help="listfile to download (default: latest release)")
    p.add_argument("--source", help="filter this local listfile instead of downloading one")
    args = p.parse_args()

    directory = os.path.join(args.data, "tables")
    out = os.path.join(directory, OUT_NAME)
    try:
        if args.source:
            print(f"{args.source} -> {out}")
            with open(args.source, "rb") as f:
                rows = icon_rows(f)
        else:
            print(f"{args.url} -> {out}")
            with open_url(args.url) as resp:
                length = resp.headers.get("Content-Length")
                rows = icon_rows(resp, int(length) if length else None)
    except ValueError as e:
        sys.exit(f"{e}; nothing written")
    if not rows:
        sys.exit(f"no {PREFIX} rows: not a listfile? nothing written")

    os.makedirs(directory, exist_ok=True)
    tmp = out + ".part"
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        for file_data_id in sorted(rows):
            f.write(f"{file_data_id};{rows[file_data_id]}\n")
    os.replace(tmp, out)
    print(f"{len(rows):,} icons written")

    exported = exported_icons(args.data)
    if exported:
        missing = sorted(exported - rows.keys())
        print(f"{len(exported) - len(missing)} of {len(exported)} exported icons have a name")
        if missing:
            print(f"without a name: {' '.join(map(str, missing))}")


if __name__ == "__main__":
    main()
