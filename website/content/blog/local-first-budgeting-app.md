---
title: "Why I keep my budget in a file, not in someone else's cloud"
PageTitle: "A Private Budgeting App: Why Your Money Data Should Live on Your Computer"
description: "Budgeting apps come and go, and they take your history with them. The case for local-first money software: one file you own, encrypted, backed up where you choose, and still there in ten years."
slug: "local-first-budgeting-app"
date: 2026-10-22
topic: "Privacy"
priority: 0.7
tags: ["privacy", "local-first", "budgeting app", "Mint alternative"]
---

When Intuit shut down Mint in 2024, millions of people found out at the same
moment that years of their financial history lived somewhere they didn't
control. They could export a CSV. They couldn't keep the thing they'd built:
the categories, the rules, the habits, the history in one place.

That's not a story about one company. It's how most money software works: your
data lives on the vendor's servers, in the vendor's format, for as long as the
vendor wants to keep the product going. There's another way to build it.

## What "local-first" means

Local-first software keeps your data **on your own device**, as the primary
copy. The app works offline, opens instantly, and doesn't need an account. If
you want copies elsewhere, you send them there, on your terms.

For something as personal as your finances, that changes a lot.

### It's private by default

Your transactions describe your life: where you live, what you buy, who you pay,
what you earn, when you're struggling. A local app has no reason to collect any
of it. There's no analytics on your spending, no "anonymized" data sold to
partners, and nothing for a breach to leak.

### It doesn't disappear

A file on your computer doesn't get sunset, acquired, or moved to a new pricing
plan. If the developer stopped tomorrow, the app on your machine would keep
working, and your books would still be in your Documents folder.

### You can see it, copy it and hand it over

Your books are a thing you can point to. Back it up like any other file. Copy it
to a new computer. Hand it to your accountant at tax time.

## "But what about my bank feed?"

Local-first doesn't mean manual-only. A local app can still connect to your bank
through services like **SimpleFIN** or **Plaid**, directly from your computer,
with the access token kept in your system's keychain rather than on a server.
Your bank password only ever goes to your bank's own sign-in page.

And when you'd rather not connect at all, nearly every bank lets you download a
**CSV or OFX/QFX** statement. A good local app imports those safely, skipping
duplicates when the dates overlap.

## "And backups?"

This is the honest trade-off: if the data is yours, so is the backup. The answer
is to make it easy, not to give the data away:

- **Encrypt the file.** With a strong key held in your system keychain, the file
  is unreadable to anyone who copies it, and you keep a recovery key for
  anywhere else.
- **Send encrypted copies wherever you like:** iCloud Drive, Dropbox, Google
  Drive, OneDrive, or a private Git repository. The cloud stores a sealed box it
  can't open.
- **Back up before every update,** automatically.

## What to look for in a private budgeting app

If you're shopping for one, a few questions sort the local-first apps from the
ones that merely say "secure":

1. **Does it work without an account?** If you have to sign up, your data is
   going somewhere.
2. **Where is the file?** You should be able to find it, copy it and open it on
   another computer.
3. **Is it encrypted at rest,** with a key you control?
4. **What does it send home?** The right answer is close to nothing: an update
   check, maybe a one-time license check.
5. **What happens if the company disappears?** The app should keep working.

## Why Tallybook works this way

We built [Tallybook](/) around these answers. Your household budget, bills and
goals, and if you run a business, your books and tax prep, live in **one
`.tallybook` file** on your computer. There's no account. Encryption is one
click. Cloud copies are only ever sent encrypted. The household budget is free
for good, and the business features are bought outright, not rented as a subscription
that holds your history hostage.

Your money, in one file you own. [Download it free](/download/).
