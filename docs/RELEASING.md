# Releasing Tallybook

Releases are built by GitHub Actions (`.github/workflows/release.yml`) when
you push a version tag. Nothing is published automatically: the workflow
creates a **draft** release with installers attached, and you publish it by
hand.

## Steps

```sh
scripts/bump-version.py 0.2.0        # sets the version in Cargo, Tauri and npm
cargo check -q                       # refreshes Cargo.lock
git commit -am "Release v0.2.0"
git tag v0.2.0
git push && git push origin v0.2.0
```

Then:

1. Watch **Actions → Release** on GitHub. It first runs the same checks as CI
   (lint, tests, frontend build) and confirms the tag matches the app
   version, then builds four installers in parallel. A run takes about
   15–25 minutes.
2. Open **Releases**, find the draft "Tallybook v0.2.0", check the attached
   files and edit the notes.
3. Click **Publish release**.

A tag with a hyphen (`v0.2.0-beta.1`) is marked as a pre-release.

To throw away a bad tag before publishing:
`git push --delete origin v0.2.0 && git tag -d v0.2.0`, then delete the
draft release.

## What gets built

| Platform | Files |
|---|---|
| macOS, Apple silicon | `.dmg`, `.app.tar.gz` |
| macOS, Intel | `.dmg`, `.app.tar.gz` |
| Windows | `-setup.exe` (NSIS), `.msi` |
| Linux | `.AppImage`, `.deb`, `.rpm` |

## Code signing (optional, recommended before a public launch)

Unsigned builds work, but each operating system warns first-time users:

- **macOS:** a downloaded unsigned app is blocked ("can't be opened" or
  "is damaged"). Users can right-click → **Open**, or run
  `xattr -cr /Applications/Tallybook.app`. Signing and notarizing removes
  this. It needs an Apple Developer account ($99/year). Add these repository
  secrets and the workflow uses them automatically: `APPLE_CERTIFICATE`
  (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
  `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID`.
  Tauri's guide: <https://v2.tauri.app/distribute/sign/macos/>
- **Windows:** SmartScreen shows "Windows protected your PC" → **More info →
  Run anyway** until the app has a code-signing certificate and builds up
  reputation. Tauri's guide: <https://v2.tauri.app/distribute/sign/windows/>
- **Linux:** no signing needed.

## Costs to know about

While the repository is private, Actions minutes count against your
GitHub plan, and macOS runners count 10× (Windows 2×). One release with two
macOS builds can use a few hundred billed minutes. Public repositories run
Actions for free.
