#!/usr/bin/env python3
"""Regenerate the Rust table in THIRD_PARTY_NOTICES.md and all of THIRD_PARTY_LICENSES.md.

Run from the repo root after any Cargo.lock change: python3 scripts/gen-third-party-notices.py
Both outputs cover the crates in the x86_64 Linux build (normal and build dependencies, no dev-dependencies).
The notices file is rewritten only between its BEGIN/END markers (exactly one pair, else this fails);
everything else in it is hand-written. Offline (cargo --frozen): texts upstream does not ship come from licenses/overrides/.
Fails, listing names, if a crate in the build ends up with no license text.
"""
import collections
import json
import os
import re
import subprocess
import sys

NOTICES = "THIRD_PARTY_NOTICES.md"
LICENSES = "THIRD_PARTY_LICENSES.md"
BEGIN = "<!-- BEGIN GENERATED: rust-dependencies -->"
END = "<!-- END GENERATED: rust-dependencies -->"
# Packages with no `license` field, checked against the upstream LICENSE file or SPDX headers.
FALLBACK = {
    "apply": "Unlicense",
    "cosmic-config": "MPL-2.0",
    "cosmic-config-derive": "MPL-2.0",
    "cosmic-settings-config": "MPL-2.0",
    "cosmic-settings-daemon": "MPL-2.0",
    "cosmic-theme": "MPL-2.0",
    "dnd": "MIT",
    "iced_accessibility": "MIT",  # no headers of its own: falls under iced/LICENSE
    "libcosmic": "MPL-2.0",
    "mime": "MIT",
}
# Cargo's license field (or the fallback) is not the whole story for these; see "Mixed Licensing" in the notices.
ANNOTATE = {
    "libcosmic": "MPL-2.0, with file-level GPL-3.0-only and MIT files",
    "iced_winit": "MIT, with one MPL-2.0 file",
}
# The repository-level LICENSE of these is not their crate's license, so do not collect it.
NO_WALK = {"cosmic-settings-config"}  # repo LICENSE is GPL-3.0, the crate's files say MPL-2.0
TARGET = "x86_64-unknown-linux-gnu"
# <crate>-<version>.txt: "Source: <url>", optional "Note: <exception>" lines, a blank line, then the upstream text
OVERRIDES = "licenses/overrides"
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright|unlicense)", re.I)
CODE_EXT = {".rs", ".toml", ".json", ".py", ".c", ".h", ".js", ".sh", ".yml", ".yaml", ".html", ".css", ".in"}
# Not scanned for license files: fixtures, examples and dev scripts are not the crate's license, and the other
# platforms' code is not in the Linux build (libcosmic's cosmic-icons are only bundled on non-Unix targets).
SKIP_DIRS = {"target", ".git", "tests", "test", "benches", "examples", "fixtures", "testdata", "scripts",
             "macos", "windows", "ios", "android", "wasm", "cosmic-icons"}
# Licenses whose text is the same for every project, recognised in files collected from other packages.
STANDARD = {
    "Apache-2.0": "Version 2.0, January 2004",
    "BSL-1.0": "Boost Software License - Version 1.0",
    "CC0-1.0": "CC0 1.0 Universal",
    "MPL-2.0": "Mozilla Public License Version 2.0",
}
# Embedded assets and file-level licenses that no package directory lists: (label, SPDX id, files in libcosmic).
EXTRAS = [
    ("Open Sans 1.10 fonts (OFL text shipped by libcosmic)", "OFL-1.1", ["res/open-sans/LICENSE"]),
    ("Open Sans 1.10 fonts (Apache-2.0, per the font metadata)", "Apache-2.0", []),
    ("Noto Sans Mono fonts (Copyright 2015 Google LLC, per the font metadata)", "OFL-1.1", ["res/noto/LICENSE"]),
    ("Iced-Icons.ttf and libcosmic's MIT files (iced)", "MIT", ["iced/LICENSE"]),
    ("vendor/iced_winit: compositor.rs", "MPL-2.0", ["LICENSE"]),
]
# Assets bundled in this repo: (label, SPDX id, license files relative to the repo root).
OWN_ASSETS = [
    ("Phosphor Icons (Fill weight)", "MIT", ["resources/icons/phosphor/LICENSE"]),
]

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--frozen", "--filter-platform", TARGET]))
# metadata's resolve keeps edges that only macOS/Windows feature flags enable, so cargo tree decides what ships.
tree = subprocess.check_output(
    ["cargo", "tree", "--frozen", "--target", TARGET, "-e", "normal,build", "--prefix", "none", "--format", "{p}"], text=True)
shipped = {m.groups() for m in re.finditer(r"^(\S+) v(\S+)", tree, re.M)}
pkgs = [p for p in meta["packages"] if p["name"] != "signpost" and (p["name"], p["version"]) in shipped]


def version_key(p):
    return tuple(int(n) for n in re.findall(r"\d+", p["version"].split("+")[0])[:3])


def license_of(p):
    return p["license"] or FALLBACK[p["name"]]  # KeyError: a new unlicensed package needs a checked entry


def source_of(p):
    return p["repository"] or p["homepage"] or p["source"]


def license_files(p):
    """Every license-like file under the package dir; git workspace members with none take the nearest ancestor's."""
    d = os.path.dirname(p["manifest_path"])
    found = []
    for root, dirs, files in os.walk(d):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS]
        found += [os.path.join(root, f) for f in files if LICENSE_FILE.match(f) and os.path.splitext(f)[1] not in CODE_EXT]
    walk = (p["source"] or "").startswith("git+") and p["name"] not in NO_WALK
    while not found and walk and not os.path.exists(os.path.join(d, ".git")):
        d = os.path.dirname(d)
        found = [os.path.join(d, f) for f in os.listdir(d) if LICENSE_FILE.match(f) and os.path.isfile(os.path.join(d, f))]
    return sorted(found)


def override(p):
    """(url, notes, text) from licenses/overrides/<crate>-<version>.txt, if present."""
    path = f"{OVERRIDES}/{p['name']}-{p['version']}.txt"
    if not os.path.exists(path):
        return None
    head, _, body = open(path, encoding="utf-8").read().replace("\r\n", "\n").partition("\n\n")
    lines = head.split("\n")
    if not lines[0].startswith("Source: ") or not body.strip() or not all(x.startswith("Note: ") for x in lines[1:]):
        sys.exit(f"{path}: needs 'Source: <url>', optional 'Note: ...' lines, a blank line, then the license text")
    return lines[0][len("Source: "):], [x[len("Note: "):] for x in lines[1:]], body.strip()


def clean(path):
    return open(path, encoding="utf-8", errors="replace").read().replace("\r\n", "\n").strip()


def notices_table():
    rows = sorted(pkgs, key=lambda p: (p["name"], version_key(p), source_of(p)))
    lines = ["| Package | Version | License | Source |", "|---|---:|---|---|"]
    for p in rows:
        lic = ANNOTATE.get(p["name"], license_of(p))
        lines.append(f"| `{p['name']}` | `{p['version']}` | {lic} | {source_of(p)} |")
    return "\n".join(lines)


def licenses_doc():
    libcosmic = os.path.dirname(next(p for p in pkgs if p["name"] == "libcosmic")["manifest_path"])
    entries = [(f"{p['name']} {p['version']}", license_of(p), license_files(p), p) for p in sorted(pkgs, key=lambda p: (p["name"], version_key(p)))]
    entries += [(label, lic, [os.path.join(libcosmic, f) for f in files], None) for label, lic, files in EXTRAS]
    own = next(p for p in meta["packages"] if p["name"] == "signpost")
    entries += [(label, lic, [os.path.join(os.path.dirname(own["manifest_path"]), f) for f in files], own) for label, lic, files in OWN_ASSETS]
    texts = collections.defaultdict(lambda: ([], set()))  # text -> (labels, file names)

    def add(text, label, name):
        texts[text][0].append(label)
        texts[text][1].add(name)

    for label, _, files, p in entries:
        base = os.path.dirname(p["manifest_path"]) if p else libcosmic
        for f in files:
            rel = os.path.relpath(f, base)
            add(clean(f), label, os.path.basename(f) if rel.startswith("..") else rel)
    standard = {}
    for spdx, marker in STANDARD.items():
        found = [t for t in texts if marker in t[:400]]
        if found:
            standard[spdx] = max(found, key=lambda t: len(texts[t][0]))
    gaps, missing, exceptions = [], [], []
    for label, lic, files, p in entries:
        if files:
            continue
        spdx = next((s for s in re.split(r"\s+OR\s+|/", lic) if s in standard), None)
        upstream = override(p) if p else None
        if upstream:
            url, notes, text = upstream
            add(text, label, f"{OVERRIDES}/{p['name']}-{p['version']}.txt")
            gaps.append((label, lic, f"fetched from {url}" + (" (exception, see below)" if notes else "")))
            if notes:
                exceptions.append(f"- `{label}`: {' '.join(notes)}")
        elif spdx:
            add(standard[spdx], label, f"standard {spdx} text")
            gaps.append((label, lic, f"standard {spdx} text"))
        else:
            missing.append(label)
    if missing:
        sys.exit("no license text (add licenses/overrides/<crate>-<version>.txt) for: " + ", ".join(missing))
    out = [
        "# Third-Party License Texts",
        "",
        "Generated by `python3 scripts/gen-third-party-notices.py`; do not edit. These are the license, copying, copyright and notice files",
        "found anywhere in the source of every Rust package in the Linux build (see `THIRD_PARTY_NOTICES.md` for the inventory), the vendored",
        "`iced_winit`, the fonts libcosmic embeds, and the Phosphor icons Signpost bundles. Identical texts appear once, followed by the packages that ship them.",
        "",
        "## Packages Without Their Own License File",
        "",
        "These packages ship no license file in their source. Their text is the upstream project's file at the revision Cargo recorded for the",
        "release (`.cargo_vcs_info.json`, or the pinned git revision), fetched once into `licenses/overrides/`, except the entries under Provenance",
        "Exceptions. Where the license is or includes Apache-2.0, BSL-1.0, CC0-1.0 or MPL-2.0, the standard text from another package is used instead.",
        "",
        "| Package | License | Text |",
        "|---|---|---|",
    ]
    out += [f"| `{label}` | {lic} | {note} |" for label, lic, note in gaps if label not in {e[0] for e in EXTRAS}]
    if exceptions:
        out += ["", "## Provenance Exceptions", ""] + exceptions
    out += ["", "## Texts", ""]
    for n, (text, (labels, names)) in enumerate(sorted(texts.items(), key=lambda kv: (sorted(kv[1][0])[0], kv[0])), 1):
        fence = "`" * max(3, max((len(m) + 1 for m in re.findall(r"`+", text)), default=3))
        title = next(line.strip() for line in text.splitlines() if line.strip())[:70]
        out += [f"### {n}. {title}", "", f"Files: {', '.join(f'`{f}`' for f in sorted(names))}", "", "Used by: " + ", ".join(f"`{x}`" for x in sorted(labels)), "", fence, text, fence, ""]
    return "\n".join(out)


doc = open(NOTICES, encoding="utf-8").read()
if doc.count(BEGIN) != 1 or doc.count(END) != 1 or doc.index(BEGIN) > doc.index(END):
    sys.exit(f"{NOTICES} must contain exactly one {BEGIN} ... {END} pair")
licenses = licenses_doc()  # exits before anything is written if a crate has no text
head, rest = doc.split(BEGIN)
tail = rest.split(END)[1]
open(NOTICES, "w", encoding="utf-8").write(f"{head}{BEGIN}\n{notices_table()}\n{END}{tail}")
open(LICENSES, "w", encoding="utf-8").write(licenses + "\n")
print(f"{len(pkgs)} crates, {sum(1 for p in pkgs if override(p))} overrides")
