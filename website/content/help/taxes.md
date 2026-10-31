---
title: "Tax prep in Tallybook"
description: "What Tallybook's tax center does for sole proprietors and single-member LLCs: Schedule C, quarterly estimates, 1099s and an accountant package, and what it doesn't do."
type: "help"
group: "Reports and taxes"
weight: 20
priority: 0.6
cta: true
css: []
js: []
---

Tallybook's Tax Center turns your books into the worksheets and numbers you
need at tax time. It's built for sole proprietors and single-member LLCs
that file **Schedule C** with their Form 1040.

> **What it is:** a preparer's assistant. It organizes, calculates and
> explains, and every figure shows where it came from.
>
> **What it isn't:** tax advice or a filing service. Tallybook never submits
> anything to the IRS or a state. Review its figures with a tax professional,
> or carefully against the IRS instructions, before you file or pay.

{{< shot "taxes" "The Taxes overview: estimated profit, what to set aside, and a four-step tax checklist" >}}

## What it does

| Area | What you get |
|---|---|
| **Schedule C** | Each income/expense account is mapped to a Schedule C line. Tallybook suggests the line and you confirm it. You get a line-by-line worksheet with meals limited to 50% and an optional simplified home-office deduction. |
| **Quarterly estimates** | A plan for federal estimated tax (Form 1040-ES) with due dates and payment tracking. Payments are recorded as owner's draws, because estimated income tax is personal, not a business expense. |
| **1099s** | Totals paid to each payee, split into **bank payments** (reportable by you on 1099-NEC) and **card payments** (reported by the card networks, not you). Flags who's over the threshold and whose W-9 is missing. |
| **Accountant package** | A folder with a readable tax summary and general ledger as PDFs, plus the Schedule C worksheet, account mapping, general ledger, 1099 summary and estimated payments as spreadsheets, and a README. |

{{< shot "schedule-c" "The Schedule C worksheet, built line by line from your accounts" >}}

## Two ways to plan estimates

1. **Safe harbor (recommended).** Pay last year's total tax in four equal
   parts, or 110% of it if last year's AGI was over $150,000 ($75,000 married
   filing separately). That avoids the federal underpayment penalty no matter
   how much you earn this year. It needs only two numbers from last year's
   return.
2. **This year's projection.** Tallybook projects the year from your books.
   It computes self-employment tax (Schedule SE), additional Medicare tax,
   the standard deduction, a simplified QBI deduction, and federal income tax
   from the brackets. Paying 90% of the current year's tax also avoids the
   penalty, but this number moves as your income changes.

Record each payment under **Taxes → Quarterly estimates → Record a payment**, so
the plan knows what's left.

{{< shot "quarterly-estimates" "Quarterly estimates: the plan per quarter, what's paid, and Record a payment" >}}

### Not included in the projection

- Itemized deductions (it uses the standard deduction)
- Tax credits (child, education, energy…)
- Capital gains and qualified dividends at special rates; net investment income tax
- Retirement contributions and self-employed health insurance deductions
- Newer special deductions (tips, overtime, car-loan interest, seniors)
- QBI above the threshold, where the real deduction depends on facts Tallybook doesn't have. It's left out, which is conservative.
- State and local income tax (a rough state set-aside rate is optional)

## 1099s and the accountant package

**Taxes → 1099s** lists everyone you paid this year, with their W-9 status and
whether they need a 1099-NEC. **Export the 1099 summary** gives you or your
preparer the totals.

{{< shot "1099s" "Who needs a 1099-NEC: payees, W-9 status, and bank versus card totals" >}}

**Taxes → For your accountant → Create tax package** writes the whole folder.
Running it again replaces the files with fresh ones.

## Privacy

Only the **last four digits** of a contractor's tax ID are stored. The full
number belongs on the W-9 and on the IRS filing, not in the book file or in
exports.

## Tax tables

Each tax year's figures come from the IRS and SSA publications listed below.
A year without published tables is refused, never guessed.

| Year | Brackets, standard deduction, QBI | Wage base | 1099-NEC threshold |
|---|---|---|---|
| 2025 | IRS Rev. Proc. 2024-40; standard deduction as amended by Pub. L. 119-21 | SSA: $176,100 | $600 |
| 2026 | IRS Rev. Proc. 2025-32 | SSA: $184,500 | $2,000 (Pub. L. 119-21) |

The tax engine is checked against the cumulative-tax figures printed in the
IRS Revenue Procedures before every release.
