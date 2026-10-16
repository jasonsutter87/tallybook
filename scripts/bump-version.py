#!/usr/bin/env python3
"""Set Tallybook's version everywhere it lives.

    scripts/bump-version.py 0.2.0

Updates the Cargo workspace, the Tauri config and app/package.json, then
prints the git commands to commit and tag. It doesn't run them, so you can
look at the diff first.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", sys.argv[1]):
        sys.exit("usage: scripts/bump-version.py X.Y.Z   (optionally X.Y.Z-beta.1)")
    version = sys.argv[1]

    cargo = ROOT / "Cargo.toml"
    text = cargo.read_text()
    new, n = re.subn(r'(?m)^(\[workspace\.package\][^\[]*?^version\s*=\s*)"[^"]*"', rf'\g<1>"{version}"', text, count=1, flags=re.S)
    if n != 1:
        sys.exit("couldn't find [workspace.package] version in Cargo.toml")
    cargo.write_text(new)

    for rel in ("app/src-tauri/tauri.conf.json", "app/package.json"):
        path = ROOT / rel
        data = json.loads(path.read_text())
        data["version"] = version
        path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")

    print(f"Version set to {version}. Next:")
    print("  cargo check -q          # refreshes Cargo.lock")
    print(f'  git commit -am "Release v{version}"')
    print(f"  git tag v{version}")
    print(f"  git push && git push origin v{version}")


if __name__ == "__main__":
    main()
