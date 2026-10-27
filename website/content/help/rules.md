---
title: "Rules"
description: "Categorize bank transactions automatically: match the description, amount or account, and choose whether to review first."
type: "help"
group: "Bring in transactions"
weight: 30
---

A **rule** categorizes matching transactions for you. Tallybook also learns
from what you accept in [Review](/help/review/), so you only need rules for
things you want handled the same way every time.

{{< shot "rules" "Rules, each showing what it matches, the category, Auto-add or Suggest in review, and its priority" >}}

Rules run **top to bottom**, and the first one that matches wins.

## Make a rule

1. Go to **Rules → New rule**.
2. **When the description…** *contains*, *is exactly* or *starts with* some
   text. It's not case-sensitive.
3. **And the money is…** *either way*, *money in* or *money out*.
4. Optionally limit it to **one account**, or to an amount range with
   **Amount at least** and **Amount at most**.
5. **Then categorize as**: pick the category.
6. Optionally **Rename to** a cleaner name for your books, like "Figma"
   instead of "FIGMA.COM 8823", and give the rule a name for your reference.
7. Click **Create rule**.

{{< shot "new-rule" "New rule: match text, money in or out, account and amount, the category, rename, priority and Add automatically" >}}

## Review first, or add automatically

By default a matching transaction waits in Review with the category
suggested, one click to accept (**Suggest in review**). Turn on **Add
automatically without review** and matching transactions skip Review
altogether (**Auto-add**).

## Priority

When two rules could match, the one with the **lower priority number** runs
first.

Edit or delete any rule from its row on the Rules page.
