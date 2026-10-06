"""Builds the browser version of csim-live into `site/` (gitignored), as GitHub Pages serves it.

Run from anywhere: `python tools/build_web.py [options]`. It

1. builds `csim-web` for `wasm32-unknown-unknown` (release);
2. binds it with `wasm-bindgen --target web` into `site/live/`. The CLI must be the version the
   workspace pins (`wasm-bindgen = "=X"` in Cargo.toml): `cargo install wasm-bindgen-cli
   --version X --locked`;
3. shrinks it with `wasm-opt -Oz` (binaryen) when that is on the PATH, else leaves it as is;
4. copies the page (`crates/csim-live/src/index.html`), `crates/csim-web/web/web.js` and the Sim
   view's Web Worker (`crates/csim-web/web/sim-worker.js`) next to it;
5. writes `site/index.html`, a redirect to `live/` (other tools can get their own subpages
   later without moving the viewer), and `site/.nojekyll` (Pages serves the files as they are).

Test it locally with `python -m http.server -d site 8000`, then open http://localhost:8000/
(it redirects to /live/).

Only the standard library is used. cargo, wasm-bindgen and wasm-opt are looked up on the PATH,
then in ~/.cargo/bin.
"""
import argparse
import gzip
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
TARGET = "wasm32-unknown-unknown"
WASM = os.path.join(ROOT, "target", TARGET, "release", "csim_web.wasm")
PAGE = os.path.join(ROOT, "crates", "csim-live", "src", "index.html")
WEB_JS = os.path.join(ROOT, "crates", "csim-web", "web", "web.js")
SIM_WORKER = os.path.join(ROOT, "crates", "csim-web", "web", "sim-worker.js")

REDIRECT = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="0; url=live/">
<link rel="canonical" href="live/">
<title>ClassicSimForever</title>
<style>
  :root { color-scheme: dark; }
  body { margin: 0; padding: 16px; background: #111317; color: #d8dbe0;
         font: 14px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif; }
  a { color: #e0b04a; }
</style>
</head>
<body>
<p>ClassicSimForever: <a href="live/">the live viewer</a>.</p>
</body>
</html>
"""


def tool(name):
    """The executable `name`, from the PATH or ~/.cargo/bin; None when missing."""
    found = shutil.which(name)
    if found:
        return found
    cargo_bin = os.path.join(os.path.expanduser("~"), ".cargo", "bin")
    for candidate in (name, name + ".exe"):
        path = os.path.join(cargo_bin, candidate)
        if os.path.isfile(path):
            return path
    return None


def run(command):
    print("$ " + " ".join(os.path.basename(command[0]) if i == 0 else part
                          for i, part in enumerate(command)), flush=True)
    subprocess.run(command, cwd=ROOT, check=True)


def pinned_wasm_bindgen():
    """The wasm-bindgen version the workspace pins (`wasm-bindgen = "=X"`)."""
    with open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8") as f:
        m = re.search(r'^wasm-bindgen = "=([0-9][0-9.]*)"', f.read(), re.M)
    if not m:
        sys.exit('Cargo.toml pins no wasm-bindgen version (`wasm-bindgen = "=X"`)')
    return m.group(1)


def size(path):
    with open(path, "rb") as f:
        data = f.read()
    return f"{len(data) / 1e6:.2f} MB ({len(gzip.compress(data, 9)) / 1e6:.2f} MB gzipped)"


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--out", default=os.path.join(ROOT, "site"),
                   help="output directory (default: site/)")
    p.add_argument("--no-opt", action="store_true", help="skip wasm-opt even when available")
    args = p.parse_args()

    cargo = tool("cargo")
    if not cargo:
        sys.exit("cargo not found (on the PATH or in ~/.cargo/bin)")
    version = pinned_wasm_bindgen()
    bindgen = tool("wasm-bindgen")
    install = f"cargo install wasm-bindgen-cli --version {version} --locked"
    if not bindgen:
        sys.exit(f"wasm-bindgen not found: {install}")
    found = subprocess.run([bindgen, "--version"], capture_output=True, text=True, check=True)
    if found.stdout.split()[-1] != version:
        sys.exit(f"{found.stdout.strip()}, but the workspace pins {version}: {install}")

    run([cargo, "build", "--release", "-p", "csim-web", "--target", TARGET])
    print(f"csim_web.wasm: {size(WASM)}")

    live = os.path.join(args.out, "live")
    if os.path.isdir(args.out):
        shutil.rmtree(args.out)
    os.makedirs(live)
    run([bindgen, "--target", "web", "--no-typescript", "--out-dir", live, WASM])
    bound = os.path.join(live, "csim_web_bg.wasm")

    wasm_opt = None if args.no_opt else tool("wasm-opt")
    if wasm_opt:
        optimized = bound + ".opt"
        # The features rustc enables by default for the target; wasm-opt must accept them.
        features = ["--enable-bulk-memory", "--enable-mutable-globals",
                    "--enable-nontrapping-float-to-int", "--enable-sign-ext",
                    "--enable-reference-types", "--enable-multivalue"]
        try:
            run([wasm_opt, "-Oz", *features, bound, "-o", optimized])
            os.replace(optimized, bound)
        except subprocess.CalledProcessError:
            print("warning: wasm-opt failed; keeping the unoptimized module", file=sys.stderr)
            if os.path.exists(optimized):
                os.remove(optimized)
    else:
        print("wasm-opt not found (binaryen): the module is not shrunk")
    print(f"csim_web_bg.wasm: {size(bound)}")

    shutil.copy(PAGE, os.path.join(live, "index.html"))
    shutil.copy(WEB_JS, os.path.join(live, "web.js"))
    shutil.copy(SIM_WORKER, os.path.join(live, "sim-worker.js"))
    with open(os.path.join(args.out, "index.html"), "w", encoding="utf-8", newline="\n") as f:
        f.write(REDIRECT)
    open(os.path.join(args.out, ".nojekyll"), "w").close()

    print(f"built {args.out}: test with `python -m http.server -d {os.path.relpath(args.out)} "
          "8000`, then http://localhost:8000/")


if __name__ == "__main__":
    main()
