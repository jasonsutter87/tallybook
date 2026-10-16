# Tax Center

Tallybook's Tax Center turns your books into the worksheets and numbers you
need at tax time. It's built for sole proprietors and single-member LLCs
that file **Schedule C** with their Form 1040.

> **What it is:** a preparer's assistant. It organizes, calculates and
> explains, and every figure shows where it came from.
>
> **What it isn't:** tax advice or a filing service. Tallybook never submits
> anything to the IRS or a state. Review its figures with a tax professional,
> or carefully against the IRS instructions, before you file or pay.

## What it does

| Area | What you get |
|---|---|
| **Schedule C** | Each income/expense account is mapped to a Schedule C line. Tallybook suggests the line and you confirm it. You get a line-by-line worksheet with meals limited to 50% and an optional simplified home-office deduction. |
| **Quarterly estimates** | A plan for federal estimated tax (Form 1040-ES) with due dates and payment tracking. Payments are recorded as owner's draws, because estimated income tax is personal, not a business expense. |
| **1099s** | Totals paid to each payee, split into **bank payments** (reportable by you on 1099-NEC) and **card payments** (reported by the card networks, not you). Flags who's over the threshold and whose W-9 is missing. |
| **Accountant package** | A folder with the Schedule C worksheet, the account mapping, the general ledger, a 1099 summary, estimated payments and a README. |

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

### Not included in the projection

- Itemized deductions (it uses the standard deduction)
- Tax credits (child, education, energy…)
- Capital gains and qualified dividends at special rates; net investment income tax
- Retirement contributions and self-employed health insurance deductions
- Newer special deductions (tips, overtime, car-loan interest, seniors)
- QBI above the threshold, where the real deduction depends on facts Tallybook doesn't have. It's left out, which is conservative.
- State and local income tax (a rough state set-aside rate is optional)

## Privacy

Only the **last four digits** of a contractor's tax ID are stored. The full
number belongs on the W-9 and on the IRS filing, not in the book file or in
exports.

## Tax tables and how they're verified

Tables live in `crates/tally-core/src/tax.rs` (`TAX_YEARS`). Each year cites
its sources, and **a year without tables is refused, never guessed.**

| Year | Brackets, standard deduction, QBI | Wage base | 1099-NEC threshold |
|---|---|---|---|
| 2025 | IRS Rev. Proc. 2024-40; standard deduction as amended by Pub. L. 119-21 | SSA: $176,100 | $600 |
| 2026 | IRS Rev. Proc. 2025-32 | SSA: $184,500 | $2,000 (Pub. L. 119-21) |

The test `matches_irs_published_cumulative_tax` checks the bracket engine
against the cumulative-tax figures printed in the Revenue Procedures, e.g.
"$39,207 plus 32% of the excess over $201,750". If a threshold or rate is
mistyped, that test fails.

### Adding a new year (each October/November)

1. The IRS publishes the next year's Revenue Procedure with inflation
   adjustments (brackets for all four statuses, standard deduction, §199A QBI
   thresholds).
2. The SSA announces the next year's Social Security wage base in October.
3. Check the 1099-NEC threshold. It's indexed for inflation starting with 2027.
4. Add a `TaxYear` entry with a `source` string, and add cumulative-tax
   checkpoints from the Revenue Procedure to the test above.
