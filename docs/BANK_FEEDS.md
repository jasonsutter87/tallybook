# Connecting your bank

Tallybook has no server of its own, so bank connections go straight from your
computer to the provider. You pick one of two ways in.

| | SimpleFIN Bridge | File import |
|---|---|---|
| Banks | Many US banks & credit unions | Any bank with CSV/OFX/QFX download |
| Cost | ~$15/year, paid to SimpleFIN | Free |
| Approval | None | None |
| Automatic sync | Yes (daily data) | No — import a file each month |

## SimpleFIN Bridge

1. Create an account at <https://beta-bridge.simplefin.org> and connect your
   banks there.
2. Click **New connection** to get a **setup token**.
3. In Tallybook: **Banking → Connect a bank**, paste the token. It can be used
   only once; Tallybook trades it for a long-lived access URL, which goes in
   your keychain.

## Already connected with Plaid?

Earlier versions could connect through Plaid using your own developer keys.
New Plaid connections are no longer offered, but existing ones keep syncing,
and **Reconnect** still works when a bank asks you to sign in again. Your keys
and access tokens stay in the system keychain.

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
