# Connecting your bank

There are two ways in.

| | Bank sync | File import |
|---|---|---|
| Banks | Most US banks, credit unions and cards, through Plaid | Any bank with CSV/OFX/QFX download |
| Cost | Included in every paid plan, from $20/year | Free, on every plan |
| Setup | Sign in to your bank once | None |
| Automatic sync | Yes | No: import a file each month |

## Bank sync

1. **Banking → Connect a bank → Continue to my bank.**
2. Your browser opens Plaid's secure page. Pick your bank and sign in there.
   Tallybook never sees your bank password.
3. Back in Tallybook, choose which accounts to bring in and the date to start
   from.

Behind the scenes, Tallybook talks to Plaid through **Tallybook Connect**, a
small service that holds Tallybook's Plaid keys and checks that your plan
includes bank sync ([how it works](CONNECT.md)). It passes transactions
straight to your computer without keeping them. Your computer's keychain holds
only Connect's stand-in for the bank's access token, which is useless without
your license key.

If your plan ends, syncing stops and nothing else changes: your books, your
accounts and everything already imported stay as they are.

## Already connected with SimpleFIN, or your own Plaid keys?

Earlier versions connected through SimpleFIN Bridge, or through Plaid with
developer keys you brought. Those connections keep syncing, and **Reconnect**
still works when a bank asks you to sign in again; new connections use bank
sync. Developers can still add their own Plaid keys through the API
(`bank.plaid_config_set`), and they're used when this copy has no plan with
bank sync.

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
