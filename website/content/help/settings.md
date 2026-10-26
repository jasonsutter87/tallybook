---
title: "Settings"
description: "Your business profile, invoice defaults, plan, fiscal year, closing the books, appearance, encryption, backups and where your data lives."
type: "help"
group: "Setup & your data"
weight: 31
priority: 0.6
cta: true
css: []
js: []
---

**Settings** is one page with a section for each topic. The list on the left
jumps between them.

{{< shot "settings" "The Settings page showing the business profile" >}}

| Section | What it's for |
|---|---|
| **Profile** | Your business name, email, phone and address, shown on invoices and at the top of printed reports. |
| **Business & household** | One book can keep a business, a household, or both. Each side has its own categories and reports. When business money pays for something at home, or the other way around, Tallybook records it as an owner's draw or contribution. |
| **License** | Your plan and key. **Manage or change my plan**, **Check my plan**, or **Remove from this computer** to free the key for another computer. Your books aren't touched. |
| **Invoices** | Defaults for new invoices: **Next invoice number**, **Payment terms**, **Default sales tax** and a **Footer message**. |
| **Move from Ledger** | Bring people, entries, monthly plans, bills, goals and stores over from the Ledger budgeting app. Ledger's file is only read, never changed. |
| **Fiscal year** | The **First month of the year** for your business. "Year to date" reports count from it. |
| **Close the books** | Set a **Lock date**. Nothing on or before it can be added or changed. Lock the last day of a year once its taxes are filed. |
| **Appearance** | **System**, **Light** or **Dark**. |
| **Encryption** | **Encrypt this book** locks the whole file with a random 256-bit key, kept in your system keychain. You get a recovery key to open it on another computer. Keep that somewhere safe. |
| **Cloud copies** | Encrypted copies kept off this computer. Turn on encryption first. |
| **Backup** | **Back up now** saves a dated full copy in a Backups folder next to your book. To restore, open a backup from the Welcome screen. |
| **Updates** | **Check for updates**. Every update is signed, and your book is backed up before it installs. |
| **About your data** | Where your book file is. It's a standard SQLite file on your computer, with no Tallybook cloud, no account and no tracking. |

**Delete book** is at the end of the list. It closes the book, disconnects
its banks, removes their keys from your keychain, and moves the file to the
Trash.

Household budgeting is free. Yearly plans add bank sync and the phone app,
then a business, then tax prep. If a plan ends, everything it unlocked keeps
working except bank and phone sync.
