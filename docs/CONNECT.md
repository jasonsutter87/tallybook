# Tallybook Connect

Tallybook Connect (`crates/tally-connect`, binary `tally-connect`) is a small
self-hosted Rust service that lets people on a paid plan connect their bank
through Plaid **without bringing their own Plaid keys**. We hold the Plaid keys
on this server; the app sends it a Lemon Squeezy license key and it relays a
fixed set of Plaid calls.

Everything else in Tallybook stays local-first. Books, transactions and
categories never leave the user's machine for Connect, and the server never
stores them.

## How it works

```
Tallybook app  --HTTPS, Bearer <license key>-->  tally-connect  --HTTPS, our Plaid keys-->  Plaid
                                                      |
                                                      +--> Lemon Squeezy License API (is this key active?)
                                                      +--> encrypted SQLite file (items, licenses)
```

The app uses the same Plaid client as before (`tally-feed`'s `plaid::Client`),
built with `Client::via_connect(base_url, license_key)`. Every call goes to
`{base}/v1/plaid/<path>` with the bearer header and no credentials in the body.

**Auth.** Each request's license key is checked with Lemon Squeezy
(`POST /v1/licenses/validate`). It is accepted only if `valid` is true,
`license_key.status` is `active`, `meta.store_id` is ours and `meta.product_id`
is one of ours. Results are cached in memory, keyed by the SHA-256 of the key:
one hour when valid, five minutes when not. A reply that can't be understood, or
a network error, is a 503 and is not cached.

**Allowed Plaid calls.** Only `link/token/create`, `link/token/get`,
`item/public_token/exchange`, `accounts/get`, `transactions/sync` and
`item/remove`. Anything else is a 404 and never reaches Plaid.

**What the server does to a request**

- Removes any `client_id` or `secret` from the body and adds ours.
- On `link/token/create`, sets `user.client_user_id` to an id derived from the
  license (so one user can't act as another), forces `products` to
  `["transactions"]` for a new item, and drops `webhook` and the optional
  product lists.
- Refuses to start a link for a **new** item once the license has
  `MAX_ITEMS_PER_LICENSE` items (403). Update mode, which carries an existing
  handle, is always allowed. The cap is enforced again at exchange time, and an
  Item over the cap is removed at Plaid immediately.
- **Never gives clients real Plaid access tokens.** On `item/public_token/exchange`
  it stores the real token and returns Plaid's response with `access_token`
  replaced by a handle (`tbc-<uuid>`). The app keeps the handle where it used to
  keep the token.
- Translates any `access_token` in a request from a handle owned by **that**
  license to the real token. An unknown handle, or another license's handle, is
  a 404.
- After a successful `item/remove`, deletes the row.
- Returns Plaid's status and JSON as-is otherwise, including Plaid's own error
  bodies. Errors that the service itself raises use the same shape
  (`error_code`, `error_message`, `display_message`) so the app can show them as
  written, for example "Your Tallybook plan has ended. Renew to keep your banks
  syncing."

## What it stores, and what it doesn't

One SQLCipher-encrypted SQLite database (WAL mode, mode 0600):

| Table | Contents |
|---|---|
| `licenses` | SHA-256 of the license key, **the license key itself**, last time it was seen valid, last status, and when it first looked lapsed |
| `items` | handle, license hash, Plaid `item_id`, **the real Plaid access token**, created time |

The license key is stored because the daily sweep needs to re-check it with Lemon
Squeezy after the user's app is gone. The access token is stored because Plaid
calls need it. Both are protected by the database encryption at rest and by file
permissions; they are readable by anyone who has both the database file and
`CONNECT_DB_KEY`, and by the running process.

What it does **not** store: no books, no accounts, no balances, no
transactions, no bank credentials (the user types those into Plaid's hosted
page, never to us). Plaid responses pass through memory and are not written
down.

**Logs** contain the method, the path (only our fixed routes; anything else is
logged as `<other>`), the status, the latency, and the first 8 characters of
the license hash. Never license keys, access tokens, handles, or request or
response bodies, which contain bank data. Tests assert this.

## Environment variables

Configuration comes from environment variables only.

| Variable | Required | Default | Meaning |
|---|---|---|---|
| `CONNECT_DB` | yes | | Path of the encrypted database file |
| `CONNECT_DB_KEY` | yes | | SQLCipher key (a long random string). The service refuses to start without it |
| `PLAID_CLIENT_ID` | yes | | Our Plaid client id |
| `PLAID_SECRET` | yes | | Our Plaid secret for the chosen environment |
| `PLAID_ENV` | yes | | `sandbox` or `production` |
| `LS_STORE_ID` | yes | | Our Lemon Squeezy store id; a license from another store is rejected |
| `LS_PRODUCT_IDS` | yes | | Comma list of the paid product ids that include banks |
| `BIND` | no | `127.0.0.1:8787` | Listen address. Keep it on localhost and put a TLS-terminating tunnel or proxy in front |
| `MAX_ITEMS_PER_LICENSE` | no | `10` | Connected banks per license |
| `LAPSE_GRACE_DAYS` | no | `30` | Days a plan may stay expired or disabled before its Items are removed |
| `RATE_LIMIT_PER_MIN` | no | `120` | Requests per license per minute |
| `CLIENT_IP_HEADER` | no | peer address | Header carrying the real client address, for the per-address failed-auth limit. `CF-Connecting-IP` behind Cloudflare; `X-Forwarded-For` behind Caddy (the last entry is used). Set it only when a proxy you control always sets it |
| `PLAID_BASE_URL` | tests | | Overrides the Plaid URL. For tests against a fake only |
| `LS_BASE_URL` | tests | | Overrides the Lemon Squeezy URL. For tests against a fake only |

Failed authentication is limited to 20 per client address per minute (429), and
request bodies are limited to 64 KB.

## Commands

```
tally-connect          # serve (same as `tally-connect serve`)
tally-connect sweep    # run the lapse sweeper once and exit
```

### The lapse sweeper

Plaid bills us monthly per Item until the Item is removed, so Items of plans
that have really ended must not linger. Every 24 hours (first run ten minutes
after start), and on demand with `tally-connect sweep`, the service re-validates
every license that has Items:

- Still valid: nothing happens, and any earlier lapse is forgotten.
- Lemon Squeezy says **expired or disabled**: the first sighting starts the
  grace clock. Once it has been true for more than `LAPSE_GRACE_DAYS`, each Item
  is removed at Plaid and the row is deleted only after Plaid confirms (or says
  the Item is already gone). A Plaid failure leaves the row for tomorrow.
- Anything else (network error, rate limit, unknown key, another store's key, a
  reply we don't understand): skipped. **Nothing is ever removed on an error.**

## Running it at home behind a Cloudflare Tunnel

Good for development and the sandbox beta: no router ports are opened and your
home IP stays hidden.

1. Build and install: `cargo build --release -p tally-connect`, then copy
   `target/release/tally-connect` to somewhere like `~/tally-connect/`.
2. Create an env file (outside the repo, mode 0600), based on
   `deploy/connect/tally-connect.env.example`. Use `PLAID_ENV=sandbox`. Generate
   the database key with `openssl rand -base64 32`.
3. Install `cloudflared`, run `cloudflared tunnel login`, then
   `cloudflared tunnel create tally-connect`.
4. Point a hostname at the tunnel:
   `cloudflared tunnel route dns tally-connect connect.example.com`.
5. Write `~/.cloudflared/config.yml` from `deploy/connect/cloudflared.example.yml`
   (the tunnel forwards to `http://127.0.0.1:8787`).
6. Run `tally-connect` (with the env file loaded) and `cloudflared tunnel run
   tally-connect`. Check `https://connect.example.com/healthz` answers `ok`.
7. Set `CLIENT_IP_HEADER=CF-Connecting-IP` so the failed-auth limit sees real
   client addresses.

A laptop that sleeps means banks stop syncing, so this is a beta setup, not
production.

## Running it on a small VPS (for example Hetzner) behind Caddy

1. Create a small Debian or Ubuntu server, a non-root user, and a firewall that
   allows only 22, 80 and 443. Point a DNS `A` record for
   `connect.example.com` at it.
2. Create a service user and directories:
   `useradd --system --home /var/lib/tally-connect --shell /usr/sbin/nologin tally-connect`,
   `install -d -o tally-connect -g tally-connect -m 700 /var/lib/tally-connect`,
   `install -d -m 755 /etc/tally-connect`.
3. Build with `cargo build --release -p tally-connect` (on the server, or
   cross-build), and install it to `/usr/local/bin/tally-connect`.
4. Put the settings in `/etc/tally-connect/env` (root-owned, mode 0600), from
   `deploy/connect/tally-connect.env.example`, with `PLAID_ENV=production` once
   Plaid has approved production access, and
   `CLIENT_IP_HEADER=X-Forwarded-For`.
5. Install `deploy/connect/tally-connect.service` to
   `/etc/systemd/system/`, then
   `systemctl daemon-reload && systemctl enable --now tally-connect`.
6. Install Caddy and use `deploy/connect/Caddyfile` as `/etc/caddy/Caddyfile`;
   Caddy gets and renews the TLS certificate on its own. `systemctl reload caddy`.
7. Optionally run `tally-connect sweep` from the timer in
   `deploy/connect/tally-connect-sweep.timer` as a belt-and-braces second sweep;
   the server also sweeps by itself every 24 hours.

## Backups

The database and its key are **both** needed. A backup of the file without
`CONNECT_DB_KEY` is unreadable, and the key without the file is useless.

- Back up the database with SQLite's online backup so you get a consistent copy
  while the service runs: `sqlcipher` users can use `.backup`, or stop the
  service briefly and copy `connect.db` (and `connect.db-wal` if present). The
  copy stays encrypted with the same key.
- Store the key separately from the file, in a password manager and one other
  safe place. Never in the repository.
- If the key is lost, the stored access tokens cannot be recovered. Users would
  have to reconnect their banks, and the orphaned Items keep billing until they
  are removed from the Plaid dashboard by hand.
- If the database is lost but the key is not, the same applies: Items still
  exist at Plaid and must be cleaned up there.

## Rotating the Plaid secret

1. In the Plaid dashboard, generate a new secret for the environment. Plaid keeps
   the old one valid until you rotate it out, so there is no outage window.
2. Update `PLAID_SECRET` in the env file and restart the service
   (`systemctl restart tally-connect`). Stored access tokens stay valid; they
   don't depend on the secret.
3. Check a sync from the app, then deactivate the old secret in the dashboard.

Rotate `CONNECT_DB_KEY` separately by re-keying a stopped copy of the database
(`PRAGMA rekey`) and updating the env file together, keeping the old backup and
old key until the new setup is proven.

## Developing

`cargo test -p tally-connect` starts a fake Plaid and a fake Lemon Squeezy on
local ports and drives the server through the real `tally-feed` client in
Connect mode. No network access or real keys are needed.
