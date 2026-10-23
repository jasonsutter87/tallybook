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

## Auto-update

Installed copies check
`https://github.com/jasonsutter87/tallybook/releases/latest/download/latest.json`
a few seconds after launch and every six hours. When a newer **published**
release exists, a card offers to update. It backs up the open book,
downloads, installs and restarts. Drafts and pre-releases are never offered.

- **Every update is signed.** The release workflow signs update packages with
  the private key in the `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets. The app only
  installs a package whose signature matches the public key in
  `app/src-tauri/tauri.conf.json`.
- **The key stays away from npm code where it can.** The web UI is built in
  its own job without secrets, npm installs run with `--ignore-scripts`, and
  the installer jobs skip `beforeBuildCommand`. The Tauri CLI and the Rust
  dependencies' build scripts still run with the key, pinned by the lock
  files. Two repository settings narrow this further, and only the owner can
  turn them on: a GitHub Environment (say `release`) with required reviewers
  holding the signing secrets, referenced from the `build` job, and a tag
  ruleset so only maintainers can push `v*` tags.
- **Actions are pinned to commit SHAs.** Dependabot proposes updates; CI fails
  if a `uses:` line isn't pinned.
- **The private key lives at `~/.tauri/tallybook-updater.key`** on the
  machine that created it. Its password is in the macOS Keychain under
  `tallybook-updater-key-password`. **Back up both somewhere safe**, such as
  a password manager. If the key is lost, existing installs can never be
  updated automatically again. Everyone would have to download a new version
  by hand, and you'd have to ship a new public key.
- **The repository must be public** for auto-update to work. The app
  downloads `latest.json` and the update package without logging in, which
  GitHub only allows for public repositories. Until then, the check fails
  quietly. A manual check in Settings → Updates shows the error.

### Building a release locally

Because update packages are signed, a local `tauri build` needs the key:

```sh
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/tallybook-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -a "$USER" -s tallybook-updater-key-password -w)"
cd app && npx tauri build
```

## What gets built

| Platform | Files |
|---|---|
| macOS, Apple silicon | `.dmg`, plus `.app.tar.gz` + `.sig` (update package) |
| macOS, Intel | `.dmg`, plus `.app.tar.gz` + `.sig` |
| Windows | `-setup.exe` (NSIS), `.msi`, plus `.sig` files |
| Linux | `.AppImage`, `.deb`, `.rpm`, plus `.sig` files |
| All | `latest.json`: the update manifest installed copies check |

## Code signing (optional, recommended before a public launch)

Unsigned builds work, but each operating system warns first-time users:

- **macOS:** a downloaded unsigned app is blocked ("can't be opened" or
  "is damaged"). Users can right-click → **Open**, or run
  `xattr -cr /Applications/Tallybook.app`. Signing and notarizing removes
  this. It needs an Apple Developer account ($99/year). Add these repository
  secrets, then uncomment the matching lines in `release.yml`: `APPLE_CERTIFICATE`
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
