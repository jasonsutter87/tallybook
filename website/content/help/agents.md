---
title: "Use Tallybook with AI agents"
description: "Connect Claude, OpenAI's Codex and Agents SDK, OpenClaw, or any MCP-capable agent to Tallybook, so it can answer questions about your books and, if you allow it, keep them."
type: "help"
group: "Settings and more"
weight: 20
priority: 0.6
cta: true
css: []
js: []
aliases: ["/help/claude/"]
---

Tallybook can run without its window as an [MCP](https://modelcontextprotocol.io)
server, so an AI agent can read your books and, if you allow it, keep them:
categorize the review inbox, record cash, set a month's budget, check what's
due.

MCP is an open standard, so this isn't tied to one company's AI. It works with
anything that can start a **local** MCP server: Claude, OpenAI's Codex and
Agents SDK, OpenClaw, and most coding tools and agent frameworks.

It runs on your computer, against your book file, like the app does. Nothing
is uploaded anywhere by Tallybook; what an agent does with the answers is up
to the agent you connect, and to the AI service behind it.

## Turn it on

The installed app is the server. Start it with `--mcp`:

| System  | Command |
|---------|---------|
| Mac     | `/Applications/Tallybook.app/Contents/MacOS/tallybook-desktop --mcp` |
| Windows | `"C:\Program Files\Tallybook\tallybook-desktop.exe" --mcp` (typical install path) |
| Linux   | `tallybook-desktop --mcp` |

It opens your most recently used book, read-only. Add:

- `--write` to let the agent change the book (categorize, record entries, budgets).
- `--book /path/to/Your.tallybook` to open a specific book.

It shares the app's recent books, license and keychain, so encrypted books
open as they do in the app. The examples below use the Mac path; swap in
yours.

## Claude

**Claude Code:**

```sh
claude mcp add tallybook -- /Applications/Tallybook.app/Contents/MacOS/tallybook-desktop --mcp
```

**Claude Desktop:** Settings → Developer → Edit Config, then add to
`claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "tallybook": {
      "command": "/Applications/Tallybook.app/Contents/MacOS/tallybook-desktop",
      "args": ["--mcp"]
    }
  }
}
```

**Claude Agent SDK** (Python):

```python
from claude_agent_sdk import ClaudeAgentOptions

options = ClaudeAgentOptions(
    mcp_servers={
        "tallybook": {
            "command": "/Applications/Tallybook.app/Contents/MacOS/tallybook-desktop",
            "args": ["--mcp"],
        }
    },
    allowed_tools=["mcp__tallybook"],
)
```

## OpenAI

**Codex** (the CLI, the IDE extension, and Codex in the ChatGPT desktop app):

```sh
codex mcp add tallybook -- /Applications/Tallybook.app/Contents/MacOS/tallybook-desktop --mcp
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.tallybook]
command = "/Applications/Tallybook.app/Contents/MacOS/tallybook-desktop"
args = ["--mcp"]
```

**OpenAI Agents SDK** (Python):

```python
import asyncio
from agents import Agent, Runner
from agents.mcp import MCPServerStdio

async def main():
    async with MCPServerStdio(
        params={
            "command": "/Applications/Tallybook.app/Contents/MacOS/tallybook-desktop",
            "args": ["--mcp"],
        }
    ) as tallybook:
        agent = Agent(
            name="Bookkeeper",
            instructions="Use Tallybook to answer questions about my books.",
            mcp_servers=[tallybook],
        )
        result = await Runner.run(agent, "What was my net profit this year?")
        print(result.final_output)

asyncio.run(main())
```

**ChatGPT on the web** only connects to MCP servers on the internet, and
Tallybook deliberately doesn't put your books on the internet, so it isn't
supported there.

## OpenClaw

```sh
openclaw mcp add tallybook \
  --command /Applications/Tallybook.app/Contents/MacOS/tallybook-desktop \
  --arg=--mcp
```

Then check the connection with `openclaw mcp doctor tallybook --probe`.

## Anything else

Any agent or app that can launch a local ("stdio") MCP server works the same
way: give it the command and `--mcp` as its argument. That covers Cursor,
VS Code, Windsurf, Gemini CLI, Goose and most agent frameworks; look for
"MCP servers" in their settings.

Agents that run only in the cloud can't start a program on your computer, so
they can't use Tallybook directly.

## What the agent can do

| Tool | What it does | Needs `--write` |
|------|--------------|:---:|
| `book_status` | The open book, its sides, license, inbox count, recent books | |
| `open_book` | Switch to another book | |
| `accounts` | Chart of accounts with balances (and the ids other tools take) | |
| `account_register` | Every line in one account, with a running balance | |
| `entries` | Journal entries in a date range | |
| `report` | Profit & loss, balance sheet, trial balance, A/R and A/P aging | |
| `dashboard` | The home screen's numbers | |
| `review_inbox` | Bank transactions to categorize, with suggestions | |
| `household_month` | The month's budget vs. spending, alerts and wins | |
| `bills_due` | Household bill due dates and whether they're paid | |
| `goals` | Savings goals and when they'll be reached | |
| `records` | Contacts, people, invoices, bills, payments, rules, recurring bills, bank connections | |
| `tax_summary` | Schedule C, estimated-tax plan and 1099s (Business + Tax license) | |
| `categorize` | File an inbox transaction, optionally remembering a rule | ✓ |
| `accept_suggestions` | Accept Tallybook's suggestion for inbox transactions | ✓ |
| `record_entry` | Post a balanced journal entry | ✓ |
| `void_entry` | Reverse an entry (history is kept) | ✓ |
| `set_budget` | Set a line of a month's household plan | ✓ |
| `call` | Any other API method by name, e.g. `invoices.create`, `bank.sync` | for changes |

Money is integer cents (`12345` is $123.45) and dates are `YYYY-MM-DD`.

## Guardrails

- **Read-only by default.** Without `--write`, nothing that changes the book is offered,
  and `call` refuses any method that would.
- **The book's own rules still apply.** Entries must balance, closed periods stay closed,
  and voids reverse rather than delete. License gates are the same as in the app.
- **Never available to an agent,** in any mode: deleting a book, encryption and recovery
  keys, licenses, cloud backup destinations, and connecting or disconnecting banks. Do
  those in the app.
- **The app and an agent can have the same book open.** Each waits briefly for the other's
  write to finish. Screens in the app show an agent's changes the next time they load.
- **Your books go to the agent's AI.** Whatever the agent reads is sent to the AI
  service it uses. Pick one you trust with your finances, and stay read-only unless
  you want it making changes.
