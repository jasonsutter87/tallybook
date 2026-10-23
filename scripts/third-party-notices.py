#!/usr/bin/env python3
"""Writes THIRD_PARTY_NOTICES.md: every open-source component that ships inside
Tallybook (Rust crates linked into the app, npm packages bundled into the UI),
with its license and where to find its source and full license text.

Run from the repo root before each release:  python3 scripts/third-party-notices.py
"""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OURS = {"tally-core", "tally-store", "tally-feed", "tally-api", "tally-mcp", "tally-devserver", "tallybook-desktop", "tallybook"}


def rust():
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"], cwd=ROOT))
    # Only what the desktop app actually links: walk the resolved graph from it.
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    pkgs = {p["id"]: p for p in meta["packages"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == "tallybook-desktop")
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] in (None, "normal") for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    out = []
    for i in seen:
        p = pkgs[i]
        if p["name"] in OURS:
            continue
        out.append((p["name"], p["version"], p.get("license") or "see repository", p.get("repository") or ""))
    return sorted(out)


def npm():
    lock = json.loads((ROOT / "app/package-lock.json").read_text())
    out = []
    for path, info in lock.get("packages", {}).items():
        if not path or info.get("dev") or info.get("optional") and "node_modules/@esbuild" in path:
            continue
        name = path.split("node_modules/")[-1]
        pkg = ROOT / "app" / path / "package.json"
        lic, repo = info.get("license", ""), ""
        if pkg.exists():
            data = json.loads(pkg.read_text())
            lic = lic or data.get("license", "")
            r = data.get("repository")
            repo = (r.get("url") if isinstance(r, dict) else r) or data.get("homepage", "")
        if info.get("dev"):
            continue
        out.append((name, info.get("version", ""), lic or "see repository", repo.replace("git+", "")))
    return sorted(set(out))


def table(rows):
    lines = ["| Component | Version | License | Source |", "|---|---|---|---|"]
    lines += [f"| {n} | {v} | {l} | {r} |" for n, v, l, r in rows]
    return "\n".join(lines)


body = f"""# Third-party notices

Tallybook is source-available under the Functional Source License (FSL-1.1-ALv2),
and it is built on open-source components whose authors we thank. Each is used under its own license, listed below; the
full text of every license, with each component's copyright notice, is in the
component's source repository.

Where a component offers a choice of licenses, Tallybook uses it under the
permissive option (MIT or Apache-2.0). MPL-2.0 components are used unmodified;
their source is available at the linked repositories.

Plus Jakarta Sans is used under the SIL Open Font License 1.1. Icons are from
Lucide (ISC).

## Rust crates

{table(rust())}

## JavaScript packages

{table(npm())}
"""
(ROOT / "THIRD_PARTY_NOTICES.md").write_text(body)
print("wrote THIRD_PARTY_NOTICES.md")
