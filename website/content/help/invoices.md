---
title: "Invoices"
description: "Bill a customer, send the invoice, and record the payment when it comes in."
type: "help"
group: "Run the business"
weight: 10
---

**Invoices** shows what customers owe you and who's paid. The cards at the top
total what's **Unpaid**, **Overdue**, **Paid in last 30 days**, and the
**Drafts** you haven't sent. Filter the list with **All**, **Unpaid**,
**Overdue**, **Paid** and **Drafts**.

{{< shot "invoices" "Invoices: unpaid, overdue, paid and draft totals, and the list of invoices with their status" >}}

## Create an invoice

1. Click **New invoice**.
2. Under **Bill to**, choose the customer, or **New customer** to add one.
3. Set the **Invoice date** and **Terms** (*Due on receipt*, *Net 15*, *Net
   30*, *Net 60* or *Custom*). The **Due date** follows from the terms.
4. Add a line for each thing you're billing: description, quantity, rate, the
   income category, and whether it's taxable. **Add line** adds another.
5. Add a **Message on invoice** if you like. It's shown at the bottom of the
   invoice your customer sees.
6. Click **Save & issue**, or **Save draft** to finish later. Drafts stay
   private until you issue them.

{{< shot "new-invoice" "A new invoice: customer, dates and terms, line items, message and totals" >}}

Payment terms, the next invoice number, a default sales tax rate and the
footer message are set in [Settings → Invoices](/help/settings/).

## Send it

Open the invoice and, under **Send it**:

- **Email to…** starts an email to the customer.
- **Copy as text** copies the invoice to paste anywhere.
- **Print / Save PDF** gives you a PDF to attach.

## Record a payment

When a customer pays, click **Receive payment** on the Invoices page, or
**Record payment** on the invoice itself.

1. Choose the **Customer** and enter the **Amount received** and **Date
   received**.
2. **Deposit to**: usually your business checking account.
3. Pick the **Payment method** (*ACH*, *Check*, *Card*, *Cash* or *Other*),
   and add a **Reference** like a check number if you have one.
4. Click **Record payment**. Their invoices are marked paid.

{{< shot "receive-payment" "Receive payment: customer, amount, date, deposit account, method and reference" >}}

When the deposit later shows up from your bank, [Review](/help/review/) offers
to **match** it to this payment, so it isn't counted twice.

## Void an invoice

Open it and click **Void** to cancel an invoice you've issued.

See who's late at a glance in [Reports → Who owes you](/help/reports/).
