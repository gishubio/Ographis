#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.md: the libraries that are built into Ographis on Windows,
each with its licence and the licence texts it comes with, and what else is shipped.

Run it from the top folder after the dependencies change:

    python tools/third_party.py

It asks cargo which crates the programs use on Windows (not those only used by the tests),
and reads their licence files from where cargo keeps the sources.
"""

import json
import os
import subprocess
import sys

TARGET = "x86_64-pc-windows-msvc"
OUT = "THIRD-PARTY-LICENSES.md"
LICENCE_FILES = ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE", "OFL")


def main():
    meta = json.loads(
        subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET], text=True)
    )
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    ours = set(meta["workspace_members"])

    # What the programs need: from our crates along normal and build dependencies.
    wanted, todo = set(), list(ours)
    while todo:
        pid = todo.pop()
        if pid in wanted:
            continue
        wanted.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    crates = sorted((packages[p] for p in wanted - ours), key=lambda p: (p["name"].lower(), p["version"]))

    lines = [
        "# Third-party licences",
        "",
        "Ographis is licensed under MIT OR Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE).",
        "It is built with the libraries below, and ships the font and the programs listed after them.",
        "This file is made by `tools/third_party.py`; do not edit it by hand.",
        "",
        "## Also shipped",
        "",
        "- **Inter** (the font of charts and maps), © The Inter Project Authors, under the SIL Open Font",
        "  License 1.1. The licence is in `crates/og-engine/fonts/OFL.txt` and below.",
        "- **GDAL and PROJ**, with the libraries they bring, when they are shipped with the program on",
        "  Windows. GDAL is under the MIT licence, PROJ under the MIT licence; the libraries that a GDAL",
        "  build includes have their own licences, which go with that build.",
        "- **Map tiles** are not shipped: the background map is fetched from OpenStreetMap,",
        "  © OpenStreetMap contributors, under the Open Database Licence, and the map says so.",
        "",
        f"## Crates ({len(crates)})",
        "",
        "| Crate | Version | Licence | Source |",
        "|---|---|---|---|",
    ]
    for p in crates:
        src = p.get("repository") or ("vendor/" + p["name"] if p["source"] is None else "")
        lines.append(f"| {p['name']} | {p['version']} | {p.get('license') or 'see below'} | {src} |")
    lines.append("")

    font = os.path.join("crates", "og-engine", "fonts", "OFL.txt")
    lines += ["## Inter", "", "```", read(font).rstrip(), "```", ""]

    lines += ["## Licence texts", ""]
    missing = []
    for p in crates:
        folder = os.path.dirname(p["manifest_path"])
        files = sorted(f for f in os.listdir(folder) if f.upper().startswith(LICENCE_FILES) and os.path.isfile(os.path.join(folder, f)))
        # Fonts and other files inside a crate often bring their licences in their folder.
        fonts = os.path.join(folder, "fonts")
        if os.path.isdir(fonts):
            files += sorted(os.path.join("fonts", f) for f in os.listdir(fonts) if f.lower().endswith(".txt"))
        lines.append(f"### {p['name']} {p['version']}")
        lines.append("")
        if p.get("license_file"):
            files = sorted(set(files) | {os.path.relpath(os.path.join(folder, p["license_file"]), folder)})
        if not files:
            missing.append(p["name"])
            lines += [f"Licensed under {p.get('license')}. The crate comes without a licence file; the standard texts are at the end.", ""]
            continue
        for f in files:
            lines += [f"`{f}`", "", "```", read(os.path.join(folder, f)).rstrip(), "```", ""]
    lines += [
        "## Standard licence texts",
        "",
        "For the crates above that come without licence files. The copyright holders are the authors",
        "named in each crate.",
        "",
        "### MIT",
        "",
        "```",
        read("LICENSE-MIT").split("\n", 3)[3].strip(),
        "```",
        "",
        "### Apache-2.0",
        "",
        "```",
        read("LICENSE-APACHE").rstrip(),
        "```",
        "",
    ]
    with open(OUT, "w", encoding="utf-8", newline="\n") as out:
        out.write("\n".join(lines))
    print(f"{OUT}: {len(crates)} crates", file=sys.stderr)
    if missing:
        print("Without a licence file: " + ", ".join(missing), file=sys.stderr)


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read().replace("```", "'''")


if __name__ == "__main__":
    main()
