"""Downloads the server-side hotfixes of a build from wago.tools and applies them to `data/tables/`.

Run from anywhere: `python tools/fetch_hotfixes.py [options]`. `tools/fetch_tables.py` runs it
after its downloads, so it is only needed on its own to refresh the hotfixes of the same build
(Blizzard keeps pushing them) or to restore the raw tables (`--no-apply`).

The CSV dumps are the client's `.db2` files. Blizzard also changes rows server-side (hotfixes, the
client keeps them in its hotfix cache), and these are in the game and on Wowhead but not in the
dumps. wago.tools lists them on https://wago.tools/hotfixes, an Inertia.js page: the rows are the
JSON in its `data-page` attribute, 25 per page, and `search` matches every term as a substring
of `"<locale> <push id> <record id> <build> <table>"`. So every row of a build is
`search=<build number>`, filtered here on the exact build, region and locale (the number also
matches other builds' push and record ids). This is a web page, not an API: anything unexpected
in it stops the script.

1. Fetch: every page of the build into `data/tables/hotfixes.<build>.json` (the cache; `--force`
   downloads again). When a record has several hotfixes, the latest push wins.
2. Apply: each table with hotfixes is rebuilt from its raw copy in `data/tables/raw/` (made from
   the downloaded CSV the first time), so applying again, or after a refresh, starts over. A row
   of status 1 replaces the record with its ID or is appended, 2 (record removed) and 4 (not
   public) delete it, 3 (invalidated) leaves the `.db2` record. Records the hotfixes do not
   change keep their bytes, so a table without hotfixes is identical to its raw copy.

A new build often has no hotfixes on wago.tools for a few days. `--from-build` takes another
build's hotfixes (cached as that build's), and `--tables` keeps only the tables matching its
patterns (`--from-build 1.60.1.70235 --tables 'Item*'`: the item rows, which exist only as
hotfixes, while the spell and talent hotfixes of the older build are left out, since a newer
client usually has them and may have changed those rows since). The other tables stay raw.

Neither `raw/` nor the JSON cache matches `<Table>.<build>.csv`, so `TableDir` ignores them.

Only the standard library is used.
"""
import argparse
import concurrent.futures
import datetime
import fnmatch
import html
import io
import json
import os
import re
import shutil
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
DEFAULT_DIR = os.path.join(ROOT, "data", "tables")

PAGE = "https://wago.tools/hotfixes"
USER_AGENT = "ClassicSimForever-fetch-hotfixes/1.0"
# wago.tools' region of the Classic Beta (Forever) hotfixes.
DEFAULT_REGION = 70
DEFAULT_LOCALE = "enUS"
RAW_DIR = "raw"

# The hotfix row statuses (the client's hotfix cache).
VALID, REMOVED, INVALIDATED, NOT_PUBLIC = 1, 2, 3, 4

DATA_PAGE = re.compile(r'data-page="([^"]*)"')
# `<Table>.<build>.csv`, the same split as crates/csim-tables/src/dir.rs.
FILE_NAME = re.compile(r"^([A-Za-z0-9_]+)\.(\d+(?:\.\d+)*)\.csv$")


class PageError(Exception):
    """The hotfix page did not look as expected."""


def get(url, retries=4):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for attempt in range(1, retries + 1):
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                return resp.read().decode("utf-8")
        except urllib.error.HTTPError as e:
            if e.code == 404 or attempt == retries:
                raise
        except (urllib.error.URLError, TimeoutError):
            if attempt == retries:
                raise
        time.sleep(2 * attempt)


def fetch_page(search, page):
    """The paginator (`props.hotfixes`) of one page of the search."""
    url = f"{PAGE}?{urllib.parse.urlencode({'search': search, 'page': page})}"
    m = DATA_PAGE.search(get(url))
    if not m:
        raise PageError(f"{url}: no data-page attribute")
    try:
        paginator = json.loads(html.unescape(m.group(1)))["props"]["hotfixes"]
        rows, last_page, total = paginator["data"], paginator["last_page"], paginator["total"]
    except (ValueError, KeyError, TypeError) as e:
        raise PageError(f"{url}: unexpected page data ({e!r})") from e
    if not isinstance(rows, list):
        raise PageError(f"{url}: `data` is not a list")
    return rows, last_page, total


def fetch_rows(build_number, jobs):
    """Every hotfix row the search for `build_number` returns, all builds and regions."""
    search = str(build_number)
    first, last_page, total = fetch_page(search, 1)
    print(f"hotfixes: {total} rows on {last_page} pages for '{search}'")
    rows = list(first)
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        pages = pool.map(lambda p: fetch_page(search, p)[0], range(2, last_page + 1))
        for done, page_rows in enumerate(pages, 2):
            rows.extend(page_rows)
            if done % 100 == 0:
                print(f"  {done}/{last_page} pages")
    by_id = {row["id"]: row for row in rows}
    # The list is newest first: a hotfix seen while paging shifts the pages, so a row is
    # fetched twice and another one never.
    if len(by_id) != total:
        raise PageError(f"got {len(by_id)} distinct rows of {total}; the list changed while "
                        "paging, run again")
    return list(by_id.values())


def select(rows, build_number, region, locale):
    """The rows of the build, region and locale, one per (table, record): the latest push."""
    latest = {}
    for row in rows:
        if (row.get("build"), row.get("region_id"), row.get("locale")) != (
                build_number, region, locale):
            continue
        key = (row["table_name"], row["record_id"])
        rank = (row["push_id"], row["created_at"], row["id"])
        if key not in latest or rank > latest[key][0]:
            latest[key] = (rank, row)
    picked = [row for _, row in latest.values()]
    picked.sort(key=lambda r: (r["table_name"], r["record_id"]))
    return [{
        "table": r["table_name"],
        "record_id": r["record_id"],
        "status": r["status"],
        "push_id": r["push_id"],
        "created_at": r["created_at"],
        "data": r["data"],
    } for r in picked]


def cache_path(directory, build):
    return os.path.join(directory, f"hotfixes.{build}.json")


def load_cache(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def write_cache(path, build, region, locale, hotfixes):
    doc = {
        "build": build,
        "region": region,
        "locale": locale,
        "fetched_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "source": PAGE,
        "hotfixes": hotfixes,
    }
    tmp = path + ".part"
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        json.dump(doc, f, ensure_ascii=False, indent=1)
        f.write("\n")
    os.replace(tmp, path)


def fetch_hotfixes(directory, build, region=DEFAULT_REGION, locale=DEFAULT_LOCALE, jobs=6,
                   force=False):
    """The hotfixes of `build`, from the cache or downloaded into it."""
    path = cache_path(directory, build)
    if os.path.exists(path) and not force:
        doc = load_cache(path)
        if (doc.get("region"), doc.get("locale")) == (region, locale):
            print(f"hotfixes: cached {os.path.basename(path)} ({doc['fetched_at']}), "
                  "--force downloads them again")
            return doc["hotfixes"]
    build_number = int(build.rsplit(".", 1)[-1])
    rows = fetch_rows(build_number, jobs)
    hotfixes = select(rows, build_number, region, locale)
    if not hotfixes and rows:
        seen = sorted({(r.get("build"), r.get("region_id"), r.get("locale")) for r in rows},
                      key=str)
        raise PageError(f"no hotfix of build {build_number}, region {region}, {locale}; "
                        f"seen (build, region, locale): {seen[:20]}")
    write_cache(path, build, region, locale, hotfixes)
    print(f"hotfixes: {len(hotfixes)} records -> {os.path.basename(path)}")
    return hotfixes


# ---------------------------------------------------------------------------------------------
# CSV records


def split_records(text):
    """Splits a CSV text into its records, each with its line ending, quoted line breaks kept."""
    records, start, quoted, i, n = [], 0, False, 0, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            quoted = not quoted
        elif c == "\n" and not quoted:
            records.append(text[start:i + 1])
            start = i + 1
        i += 1
    if start < n:
        records.append(text[start:])
    return records


def parse_record(record):
    """The fields of one record, each as (value, quoted)."""
    body = record[:-2] if record.endswith("\r\n") else record.rstrip("\n")
    fields, value, quoted, in_quotes, i = [], [], False, False, 0
    while i < len(body):
        c = body[i]
        if in_quotes:
            if c == '"':
                if i + 1 < len(body) and body[i + 1] == '"':
                    value.append('"')
                    i += 1
                else:
                    in_quotes = False
            else:
                value.append(c)
        elif c == '"':
            in_quotes = quoted = True
        elif c == ",":
            fields.append(("".join(value), quoted))
            value, quoted = [], False
        else:
            value.append(c)
        i += 1
    fields.append(("".join(value), quoted))
    return fields


def flatten(values):
    out = []
    for v in values:
        if isinstance(v, list):
            out.extend(flatten(v))
        else:
            out.append(v)
    return out


def to_text(value):
    if value is None:
        return ""
    if isinstance(value, bool):
        return str(int(value))
    return str(value)


class Table:
    """One CSV table: its header and records, the untouched ones kept byte for byte."""

    def __init__(self, text):
        records = split_records(text)
        if not records:
            raise ValueError("empty table")
        self.header_record = records[0]
        self.newline = "\r\n" if self.header_record.endswith("\r\n") else "\n"
        self.columns = [v for v, _ in parse_record(self.header_record)]
        if "ID" not in self.columns:
            raise ValueError("no ID column")
        self.id_column = self.columns.index("ID")
        self.records = records[1:]
        self.index = {}
        for i, record in enumerate(self.records):
            self.index[int(parse_record(record)[self.id_column][0])] = i

    def values(self, i):
        return [v for v, _ in parse_record(self.records[i])]

    def format(self, values):
        """One record as wago.tools writes it: a value is quoted when it holds whitespace, a
        comma or a quote."""
        out = []
        for v in values:
            if any(c.isspace() or c in ',"' for c in v):
                out.append('"' + v.replace('"', '""') + '"')
            else:
                out.append(v)
        return ",".join(out) + self.newline

    def apply(self, hotfixes):
        """Applies the hotfixes; returns the counts of what they did."""
        counts = {"added": 0, "replaced": 0, "unchanged": 0, "removed": 0, "ignored": 0}
        removed = set()
        for fix in hotfixes:
            record_id, status = fix["record_id"], fix["status"]
            at = self.index.get(record_id)
            if status == VALID:
                values = [to_text(v) for v in flatten(fix["data"])]
                if len(values) != len(self.columns):
                    raise ValueError(f"record {record_id}: {len(values)} values, "
                                     f"{len(self.columns)} columns")
                if int(values[self.id_column]) != record_id:
                    raise ValueError(f"record {record_id}: its ID column holds "
                                     f"{values[self.id_column]}")
                if at is None or at in removed:
                    self.index[record_id] = len(self.records)
                    self.records.append(self.format(values))
                    counts["added"] += 1
                elif self.values(at) == values:
                    counts["unchanged"] += 1
                else:
                    self.records[at] = self.format(values)
                    counts["replaced"] += 1
            elif status in (REMOVED, NOT_PUBLIC):
                if at is not None and at not in removed:
                    removed.add(at)
                    counts["removed"] += 1
                else:
                    counts["ignored"] += 1
            else:
                counts["ignored"] += 1
        if removed:
            self.records = [r for i, r in enumerate(self.records) if i not in removed]
        return counts

    def text(self):
        return self.header_record + "".join(self.records)


def read_text(path):
    with open(path, encoding="utf-8", newline="") as f:
        return f.read()


def write_text(path, text):
    tmp = path + ".part"
    with open(tmp, "w", encoding="utf-8", newline="") as f:
        f.write(text)
    os.replace(tmp, path)


def local_tables(directory, build):
    """{table: file name} of the build's CSVs in `directory`."""
    tables = {}
    for name in os.listdir(directory):
        m = FILE_NAME.match(name)
        if m and m.group(2) == build:
            tables[m.group(1)] = name
    return tables


def apply_hotfixes(directory, build, hotfixes):
    """Rebuilds the build's tables from their raw copies plus the hotfixes."""
    raw_dir = os.path.join(directory, RAW_DIR)
    tables = local_tables(directory, build)
    by_table = {}
    for fix in hotfixes:
        by_table.setdefault(fix["table"], []).append(fix)
    raw = {t for t in tables if os.path.exists(os.path.join(raw_dir, tables[t]))}
    for table in sorted(set(by_table) & set(tables) | raw):
        path = os.path.join(directory, tables[table])
        raw_path = os.path.join(raw_dir, tables[table])
        if not os.path.exists(raw_path):
            os.makedirs(raw_dir, exist_ok=True)
            shutil.copyfile(path, raw_path)
        fixes = by_table.get(table, [])
        parsed = Table(read_text(raw_path))
        try:
            counts = parsed.apply(fixes)
        except ValueError as e:
            raise ValueError(f"{table}: {e}") from e
        write_text(path, parsed.text())
        done = ", ".join(f"{n} {what}" for what, n in counts.items() if n)
        print(f"  {table}: {done or 'no hotfixes, raw restored'}")
    skipped = sorted(set(by_table) - set(tables))
    if skipped:
        counts = ", ".join(f"{t} {len(by_table[t])}" for t in skipped)
        print(f"hotfixed tables not in {directory} (not fetched, not applied): {counts}")


def restore_raw(directory, build):
    """Puts the raw copies back in place of the hotfixed tables."""
    apply_hotfixes(directory, build, [])


def only_tables(hotfixes, patterns):
    """The hotfixes of the tables matching one of the patterns (all of them without patterns)."""
    if not patterns:
        return hotfixes
    kept = [f for f in hotfixes if any(fnmatch.fnmatchcase(f["table"], p) for p in patterns)]
    tables = sorted({f["table"] for f in kept})
    print(f"hotfixes: {len(kept)} of {len(hotfixes)} kept, of {', '.join(tables) or 'no table'}")
    return kept


def print_spells(hotfixes):
    """The hotfixed `SpellName` rows, the quickest view of which spells changed."""
    names = [(f["record_id"], f["status"], (f["data"] or [None, None])[1])
             for f in hotfixes if f["table"] == "SpellName"]
    if names:
        print("hotfixed spell names:")
        for record_id, status, name in names:
            print(f"  {record_id} {name if status == VALID else f'(status {status})'}")


def single_build(directory):
    builds = {m.group(2) for name in os.listdir(directory) if (m := FILE_NAME.match(name))}
    if len(builds) != 1:
        sys.exit(f"{directory} holds builds {sorted(builds) or 'none'}; pass --build")
    return builds.pop()


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--build", help="the build (default: the single build in the directory)")
    p.add_argument("--dir", default=DEFAULT_DIR, help="table directory (default: data/tables)")
    p.add_argument("--region", type=int, default=DEFAULT_REGION,
                   help="wago.tools region id (default: %(default)s, Classic Beta)")
    p.add_argument("--locale", default=DEFAULT_LOCALE, help="locale (default: %(default)s)")
    p.add_argument("--force", action="store_true", help="download again over the cache")
    p.add_argument("--no-apply", action="store_true",
                   help="only fetch; restore the raw tables")
    p.add_argument("--from-build", metavar="BUILD",
                   help="apply this build's hotfixes instead of the tables' own")
    p.add_argument("--tables", nargs="+", metavar="PATTERN",
                   help="apply only the hotfixes of the matching tables ('Item*')")
    p.add_argument("--jobs", type=int, default=6, help="parallel requests (default: %(default)s)")
    args = p.parse_args()

    build = args.build or single_build(args.dir)
    try:
        hotfixes = fetch_hotfixes(args.dir, args.from_build or build, args.region, args.locale,
                                  args.jobs, args.force)
    except (PageError, urllib.error.URLError) as e:
        sys.exit(f"hotfixes: {e}")
    hotfixes = only_tables(hotfixes, args.tables)
    print_spells(hotfixes)
    if args.no_apply:
        restore_raw(args.dir, build)
    else:
        apply_hotfixes(args.dir, build, hotfixes)


if __name__ == "__main__":
    main()
