# Connecting your bank

Tallybook has no server of its own, so bank connections go straight from your
computer to the provider. You pick one of three ways in.

| | Plaid | SimpleFIN Bridge | File import |
|---|---|---|---|
| Banks | ~12,000 US/CA | Many US banks & credit unions | Any bank with CSV/OFX/QFX download |
| Cost | Free for personal use (Limited Production) | ~$15/year | Free |
| Approval | Plaid reviews your application | None | None |
| Automatic sync | Yes | Yes (daily data) | No — import a file each month |

## Plaid

Plaid requires every app, including a self-hosted one, to have its own API
keys. You're the app's developer here, so you get your own keys.

1. Sign up at <https://dashboard.plaid.com/signup>.
2. Go to **Developers → Keys**. Copy your `client_id` and the **Sandbox** secret.
3. In Tallybook: **Bank connections → Connect a bank → Plaid**, paste the keys,
   choose *Sandbox*.
4. Try it with Plaid's test bank: pick any institution, user `user_good`,
   password `pass_good`.
5. When it works, request **Limited Production** access in the Plaid dashboard
   (free, a short form). Once approved, swap in your Production secret and
   choose *Production* to link real accounts.

### Why does it open my browser?

Many big banks (Chase, Bank of America, Wells Fargo…) use OAuth sign-in,
which needs to redirect back to a web page. A desktop app can't receive that
redirect, so Tallybook uses Plaid **Hosted Link**: you finish signing in on a
Plaid-hosted page in your normal browser, and Tallybook picks up the result.

### Where are my credentials?

* Your Plaid keys and each bank's access token are stored in the **system
  keychain** (macOS Keychain, Windows Credential Manager, or the Linux Secret
  Service), never in the `.tallybook` file. Sharing or backing up a book never
  exposes bank access.
* Your bank username and password are only ever entered on Plaid's or your
  bank's own page. Tallybook never sees them.

### "Needs sign-in"

Banks expire connections periodically (password changes, MFA renewals).
Tallybook shows **Needs sign-in** on that connection; click **Reconnect** to
re-authenticate the same connection. Your history and links are kept.

## SimpleFIN Bridge

1. Create an account at <https://beta-bridge.simplefin.org> and connect your
   banks there.
2. Click **New connection** to get a **setup token**.
3. In Tallybook: **Bank connections → Connect a bank → SimpleFIN**, paste the
   token. It can be used only once; Tallybook trades it for a long-lived
   access URL, which goes in your keychain.

## File import (CSV, OFX, QFX)

Download a statement from your bank's website, then **Review → Import file**.

* **OFX/QFX** ("Quicken" or "Money" format) is the best choice when offered:
  it carries unique transaction ids, so re-importing is always safe.
* **CSV** works with any bank. Tallybook guesses which columns hold the date,
  description and amount, and shows a preview so you can confirm that money
  *out* comes through negative. If your bank lists purchases as positive
  numbers, switch on **Flip signs**.
* Re-importing an overlapping date range is safe. Duplicates are recognized
  and skipped.

## Start dates and opening balances

When you link an account you choose a **start date**. Transactions before it
are ignored. Then you set the account's **opening balance** as of the day
before. Tallybook suggests one by working backward from the bank's current
balance. Your first reconciliation confirms it.
