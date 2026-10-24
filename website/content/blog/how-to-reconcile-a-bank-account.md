---
title: "The 10-minute monthly bank reconciliation (and the detective tricks for when it won't balance)"
PageTitle: "How to Reconcile a Bank Account: A 10-Minute Monthly Routine"
description: "What bank reconciliation is, a step-by-step monthly routine, and the tricks bookkeepers use to find the difference fast: divisible-by-9 transpositions, flipped signs, duplicates and timing."
slug: "how-to-reconcile-a-bank-account"
date: 2026-10-24
topic: "Bookkeeping"
priority: 0.7
tags: ["bank reconciliation", "bookkeeping", "month-end", "small business"]
---

Reconciling sounds like something accountants do in a back room with green
visors. It's actually ten minutes, once a month, and a checklist: does
**your record** of an account match **the bank's record**, to the penny?

If it does, you can trust every report built on top of it. If it doesn't,
something's missing, doubled or wrong, and you just found it while it's
still easy to fix.

## Why bother if the bank feed imports everything?

Bank feeds are great, and they're not perfect. A transaction can import
twice. One can fail to import at all. You can file a refund as an expense, or
type a cash entry with the digits swapped. None of that shows up as an error.
It shows up, months later, as a balance that's "about right."

Reconciling turns "about right" into "right," one month at a time.

## The routine

You'll need your **statement**: the monthly PDF from your bank or card.

1. **Note two numbers from the statement:** the ending date and the ending
   balance. For a credit card, that's the new balance you owed.
2. **Tick off each transaction** that appears on the statement and in your
   books. Tallybook calls these **cleared**.
3. **Watch the difference.** As you tick, Tallybook compares your beginning
   balance plus everything cleared against the statement's ending balance.
4. **When the difference is $0.00, finish.** The month is reconciled, and
   those transactions are locked so they can't drift later.

Most months, the difference hits zero as you tick the last item, and you're
done before your coffee gets cold.

## When it won't balance: detective tricks

Here are the tricks bookkeepers use, in the order they pay off.

### 1. Look for the exact amount

Is the difference exactly the amount of one transaction? Then that
transaction is probably **missing** (on the statement, not in your books) or
**extra** (in your books, not on the statement, often a duplicate import).

### 2. Halve it

Is the difference **twice** some transaction's amount? You've likely got a
**flipped sign**: a refund recorded as a charge, or a deposit as a
withdrawal. A $45 refund entered backwards leaves you $90 off.

### 3. Divide it by 9

This one feels like magic. If the difference divides evenly by 9, look for a
**transposition**: two digits swapped. Type $54 as $45 and you're off by $9.
Type $1,250 as $1,520 and you're off by $270, which is 9 × 30. Swapping two
digits always leaves a difference divisible by 9, so it narrows the search
fast.

### 4. Check the edges of the month

A payment you made on the 31st might not reach the bank until the 2nd. That's
**timing**, not an error. Leave it uncleared; it will show up on next month's
statement, and you'll tick it then.

### 5. Check the starting point

If none of that works, make sure last month's ending balance is this
month's beginning balance. If an already-reconciled transaction was changed,
the mistake is in an earlier month, not this one.

## A few habits that make it painless

- **Reconcile monthly**, not yearly. One month of mistakes is a puzzle. Twelve
  months is an archaeology project.
- **Do every account that has a statement:** checking, savings and every
  credit card, not just the main one.
- **Don't "plug" the difference** with a made-up adjustment to force zero. It
  hides the real problem, and it will come back.
- **Made a mistake on a finished month?** In Tallybook you can undo the most
  recent reconciliation, fix the entry and reconcile again.

## The first one is the hardest

Your first reconciliation sets the starting point, and it might uncover a
little history to clean up. After that, it's a ten-minute monthly ritual
with a satisfying ending: a big, round **$0.00**.

In Tallybook, open any bank or card account and choose **Reconcile**. It
walks you through the steps above, with the difference updating as you go.
