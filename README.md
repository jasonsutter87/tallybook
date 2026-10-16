<div align="center">

<img src="app/src-tauri/icons/128x128@2x.png" width="112" alt="Tallybook icon" />

# Tallybook

**Bookkeeping for small businesses, on your own computer.**

Bank feeds, invoices, bills, reports and tax prep, with your books in one
file that you own. Free and open source.

<!-- TODO: replace with the release link once v0.1.0 is published -->
[**Download**](https://github.com/jasonsutter87/tallybook/releases/latest) · [See it in action](#a-quick-tour) · [Questions](#questions)

<br />

![Tallybook home screen](docs/screenshots/hero.png)

</div>

---

## Why Tallybook?

**Your books stay with you.** Everything lives in a single file on your
computer: no account to create, no monthly fee, nothing uploaded to anyone's
cloud. Copy it, back it up, or hand it to your accountant.

**It does the boring part for you.** Connect your bank and transactions flow
in. Tallybook suggests where each one goes, learns from your choices, and
spots payments you've already recorded so nothing gets counted twice.

**Real bookkeeping, plain English.** Underneath it's proper double-entry
accounting your accountant will recognize. On screen it says "Money in,"
"Owed to you," and "Where did this go?"

**Tax time without the dread.** A Schedule C worksheet built from your books,
a quarterly estimated-tax plan, and 1099 tracking for your contractors.

---

## A quick tour

### Home: your money at a glance
Cash on hand, card balances, what's coming in and going out, and a short list
of things that need you.

![Home dashboard](docs/screenshots/home.png)

### Review: categorize in seconds
New bank transactions land here with a suggestion: a match to something you
already entered, a rule you set up, or what you picked last time. Accept one
at a time or all at once. Every action can be undone.

![Review inbox](docs/screenshots/review.png)

### Invoices: get paid
Create a clean, professional invoice, print it or save it as a PDF, and record
payments as they arrive. When the deposit shows up in your bank feed,
Tallybook matches it automatically.

![An invoice](docs/screenshots/invoice.png)

### Bills: stay on top of what you owe
Enter bills as they come in, see what's due this week, and pay from checking
or a credit card.

![Bills](docs/screenshots/bills.png)

### Reports: the numbers that matter
Profit & loss, balance sheet, and who owes you (and who you owe), for any
date range, ready to print.

![Profit and loss report](docs/screenshots/reports.png)

### Taxes: prepared, not guessed
For sole proprietors and single-member LLCs:

- **Schedule C worksheet.** Each of your categories is mapped to the right
  line, with meals limited to 50% and the home-office deduction.
- **Quarterly estimates.** How much to pay and when, using the IRS "safe
  harbor" rule so you avoid underpayment penalties.
- **1099s.** Who you paid enough to need one, with card payments correctly
  left out.
- **Accountant package.** Everything your preparer needs, in one folder.

![Tax center](docs/screenshots/taxes.png)

> Tallybook prepares worksheets and estimates; it isn't tax advice and never
> files anything for you. Have a tax professional review before you file.
> [What it does and doesn't cover →](docs/TAXES.md)

### Light or dark
Follows your system, or pick one in Settings.

![Dark mode](docs/screenshots/dark-mode.png)

---

## Getting started

1. **Download** the installer for your computer from the
   [latest release](https://github.com/jasonsutter87/tallybook/releases/latest).
2. **Open Tallybook** and create a book for your business. Or click
   **Explore with sample data** to look around a demo company first.
3. **Bring in your transactions**, either way:
   - **Connect your bank** through Plaid or SimpleFIN. See the
     [bank connection guide](docs/BANK_FEEDS.md).
   - **Import a statement**: download a CSV or OFX/QFX file from your bank's
     website and drop it into Review.
4. **Review** your transactions, and you're bookkeeping.

<details>
<summary><b>"Tallybook can't be opened" on Mac, or "Windows protected your PC"</b></summary>

<br />

Early versions of Tallybook aren't signed by Apple or Microsoft yet, so your
computer asks before opening it the first time.

- **Mac:** right-click Tallybook in Applications, choose **Open**, then
  **Open** again. If it says the app is damaged, run
  `xattr -cr /Applications/Tallybook.app` in Terminal and try again.
- **Windows:** click **More info**, then **Run anyway**.

You only need to do this once.

</details>

---

## Your data, your rules

- **One file.** Your books are a single `.tallybook` file, in
  `Documents/Tallybook` unless you choose somewhere else.
- **Backups.** **Settings → Back up now** saves a complete copy next to your
  book. Copying the file works too.
- **Bank logins stay secret.** Bank access is stored in your computer's
  keychain, never in the book file. Your bank password is only ever typed
  into your bank's (or Plaid's) own page.
- **Nothing is quietly changed.** Mistakes are fixed by voiding, which keeps
  a record, rather than by deleting history. Lock past periods once you've
  filed taxes.
- **No tracking.** Tallybook doesn't phone home.

---

## Questions

**Is it really free?**
Yes. Tallybook is open source under the [AGPL-3.0](LICENSE) license. Bank
connections use your own Plaid or SimpleFIN account; SimpleFIN costs about
$15/year, Plaid is free for personal use, and file import is always free.

**Can I move from QuickBooks?**
Start fresh with your current balances: add your accounts with an opening
balance, then import recent bank statements. A direct QuickBooks import is on
the wishlist.

**Does it work for my accountant?**
Yes. It's standard double-entry bookkeeping, and **Taxes → For your
accountant** exports your Schedule C worksheet, general ledger and more as
spreadsheets.

**Does it do payroll or file my taxes?**
No. Tallybook keeps your books and prepares your tax numbers. It doesn't run
payroll or submit anything to the IRS.

**Mac, Windows, Linux?**
There are installers for all three. Mac is the most tested so far, so
please [report anything odd](https://github.com/jasonsutter87/tallybook/issues)
on Windows or Linux.

---

<div align="center">

Built with care for freelancers and small businesses.
Want to contribute? See the [developer guide](docs/DEVELOPMENT.md).

</div>
