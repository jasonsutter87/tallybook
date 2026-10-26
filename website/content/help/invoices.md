---
title: "Invoices and getting paid"
description: "Create and issue invoices, track who's paid, and record a customer's payment."
type: "help"
group: "Business"
weight: 21
priority: 0.6
cta: true
css: []
js: []
---

**Invoices** shows what customers owe you and who's paid.

{{< shot "invoices" "The Invoices screen with Unpaid, Overdue, Paid and Drafts totals and a list of invoices" >}}

The tiles across the top show what's **Unpaid**, what's **Overdue**, what
was **Paid in last 30 days**, and how many **Drafts** haven't been sent. Tabs
filter the list by the same groups, and each invoice shows its customer,
dates, total, remaining balance and status (*Draft*, *Open*, *Overdue* or
*Paid*). Overdue invoices say how many days late they are.

## Create an invoice

Click **New invoice**. It's also on the Home screen.

{{< shot "new-invoice" "A new invoice with Bill to, dates, terms, line items and totals" >}}

1. Choose who it's **Bill to**, or add a **New customer**.
2. Set the **Invoice date** and **Terms** (*Due on receipt*, *Net 15*,
   *Net 30*, *Net 60* or *Custom*). The **Due date** follows the terms.
3. Add lines: description, quantity, rate, the income category, and whether
   it's taxable. **Add line** adds another.
4. Optionally set a tax rate, and a **Message on invoice** shown at the
   bottom of what your customer sees.
5. **Save draft** keeps it to yourself. Drafts stay private until you issue
   them with **Save & issue**.

Invoice numbers count up on their own. Defaults for terms, sales tax and the
footer message are in [Settings](/help/settings/).

## Record a payment

When a customer pays, click **Receive payment**.

{{< shot "receive-payment" "The Receive payment window" >}}

Pick the **Customer** and enter the **Amount received**, **Date received**
and the account to **Deposit to** (usually your business checking). Choose
the **Payment method** (*ACH*, *Check*, *Card*, *Cash* or *Other*) and add a
**Reference** such as a check number. Click **Record payment**, and their
invoices are marked paid.

If the deposit later arrives through your bank feed, Review recognizes it as
this payment and won't count it twice.
