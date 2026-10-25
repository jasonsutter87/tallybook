---
title: "Rules: categorize automatically"
description: "Teach Tallybook where transactions go, so they arrive in Review already categorized, or skip Review entirely."
type: "help"
group: "Getting started"
weight: 3
priority: 0.6
cta: true
css: []
js: []
---

Rules categorize bank transactions for you. Tallybook also learns from what
you accept, so you only need rules for the things you want to pin down.

{{< shot "rules" "The Rules screen listing four rules with their categories and priorities" >}}

Rules run top to bottom, and the first one that matches wins. Each rule shows
what it matches, the category it picks, and whether it is **Auto-add** or
**Suggest in review**.

## Make a rule

Click **New rule**.

{{< shot "new-rule" "The New rule window" >}}

1. **When the description…** *contains*, *is exactly*, or *starts with* some
   text. Matching isn't case-sensitive.
2. **And the money is…** *Either way*, *Money in* or *Money out*.
3. Optionally limit it to one account, or to an amount range with **Amount
   at least** and **Amount at most**.
4. **Then categorize as** the category you want. **Rename to** gives the
   transaction a cleaner name in your books, like "Figma".
5. Give it a **Rule name** and a **Priority** if you like. Lower numbers run
   first.
6. Leave **Add automatically without review** off and matching transactions
   wait in Review with this category suggested, one click to accept. Turn it
   on and they skip Review entirely.
7. Click **Create rule**.

Each rule has **Edit rule** and **Delete rule** buttons.
