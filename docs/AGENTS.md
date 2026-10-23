# Tallybook for agents

Tallybook can run without its window as an [MCP](https://modelcontextprotocol.io)
server, so Claude, or any agent that speaks MCP, can read your books and,
if you allow it, keep them: categorize the review inbox, record cash, set a
month's budget, check what's due.

It runs on your computer, against your book file, like the app does. Nothing
is uploaded anywhere by Tallybook; what the agent does with the answers is up
to the agent you connect.

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
open as they do in the app.

### Claude Code

```sh
claude mcp add tallybook -- /Applications/Tallybook.app/Contents/MacOS/tallybook-desktop --mcp
# or, to allow changes:
claude mcp add tallybook -- /Applications/Tallybook.app/Contents/MacOS/tallybook-desktop --mcp --write
```

### Claude Desktop

Settings → Developer → Edit Config, then add to `claude_desktop_config.json`:

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

### Your own agent (Claude Agent SDK, or any MCP client)

Point the client at the same command as a stdio server. In the Agent SDK:

```python
from claude_agent_sdk import ClaudeAgentOptions

options = ClaudeAgentOptions(
    mcp_servers={
        "tallybook": {
            "command": "/Applications/Tallybook.app/Contents/MacOS/tallybook-desktop",
            "args": ["--mcp", "--write"],
        }
    },
    allowed_tools=["mcp__tallybook"],
)
```

### From source

`cargo build --release -p tally-mcp` builds a standalone `target/release/tallybook-mcp`
that takes the same flags, plus `--dev` (use the development build's books and
keychain) and `--config-dir <dir>`.

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
