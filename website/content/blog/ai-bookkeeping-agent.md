---
title: "Can an AI do my bookkeeping? Yes, on a short leash"
PageTitle: "AI Bookkeeping: Let Claude or OpenAI Agents Keep Your Books, Safely"
description: "What an AI agent is actually good at in your books, the questions worth asking it, and the guardrails that keep it from making a mess: read-only by default, balanced entries, closed periods that stay closed."
slug: "ai-bookkeeping-agent"
date: 2026-10-24
topic: "AI"
priority: 0.7
tags: ["AI bookkeeping", "Claude", "MCP", "automation"]
---

Bookkeeping is mostly sorting. A coffee goes under Meals, a software
subscription goes under Software, the transfer to savings isn't spending at
all. It's repetitive, rule-shaped work, and AI agents have become
surprisingly good at that kind of work.

So yes, an AI can help keep your books. The interesting question is **how
much you let it touch**. Here's how we think about it.

## What an agent is great at

**Answering questions in plain English.** This is the one that surprises
people. Instead of building a report, you just ask:

- "What did I spend on software this year, and which subscriptions are new?"
- "Which clients haven't paid, and how late are they?"
- "Am I on pace with groceries this month?"
- "How much should my next quarterly estimate be?"

The agent reads your books, does the arithmetic, and answers in a sentence.
It's like having a bookkeeper on call who never sighs at you.

**Clearing the review inbox.** New bank transactions land in Tallybook's
inbox, usually with a suggested category. An agent can work through them,
accept the obvious ones, and set aside the ones it isn't sure about for you.

**The boring monthly chores.** Setting next month's budget from this month's,
recording a cash expense you mention in passing, checking which bills are due.

## What an agent should never do

Some things are too important to delegate to software that's guessing,
however clever the guess. In Tallybook these are off-limits to agents
entirely, in every mode:

- deleting a book,
- encryption and recovery keys,
- licenses,
- where your backups go,
- connecting or disconnecting bank accounts, and
- reopening a closed period.

You do those yourself, in the app, with your own eyes on the screen.

## The leash

Tallybook runs as an [MCP](https://modelcontextprotocol.io) server, the open
standard that Claude, OpenAI's Codex and Agents SDK, OpenClaw and most agent
tools speak. The leash has a few links:

1. **Read-only unless you say otherwise.** Out of the box, an agent can look
   at everything and change nothing. Add `--write` when you want it to file
   transactions or record entries.
2. **The books' own rules still apply.** Every entry has to balance, so an
   agent can't conjure money out of nowhere. Closed periods stay closed, so
   it can't quietly rewrite last year. And a mistake is undone with a
   reversing entry, never by erasing history, so you can always see what
   happened and when.
3. **It runs on your computer.** The agent talks to the Tallybook on your
   machine, against your book file. There's no Tallybook server holding your
   books for anyone to reach.

One honest caveat: **whatever the agent reads goes to the AI service behind
it.** If you ask Claude about your profit, your numbers go to Anthropic to
answer. Pick an AI you trust with your finances, and stay read-only until
you're comfortable.

## A good first session

Start read-only and ask questions you already know the answers to:

> "What was my net profit last month?"
>
> "What's my biggest expense category this year?"
>
> "List the transactions waiting in my inbox."

Check the answers against the app. Once you trust what it reads, turn on
`--write` and try the inbox:

> "Categorize my inbox. Accept the suggestions you're confident about, and
> show me the rest with your best guess."

Then look at what it did in the app. Every entry it made is there, dated
and reversible.

## Set it up

Setup is one command for Claude Code or Codex, and a short config for
Claude Desktop, the Agents SDK and others. The
[AI agents guide](/help/agents/) has copy-paste setup for each, and the full
list of what an agent can see and do.

The robots aren't taking over your books. But they're very happy to do the
sorting.
