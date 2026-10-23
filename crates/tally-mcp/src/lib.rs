//! Tallybook for agents: [`tally_api::App`] as a Model Context Protocol
//! server, so Claude (Desktop, Code, or anything built on the Agent SDK) can
//! read and keep the books.
//!
//! The transport is MCP's stdio one: newline-delimited JSON-RPC 2.0 on
//! stdin/stdout, logs on stderr. Only the handful of protocol methods a tool
//! server needs are implemented (`initialize`, `ping`, `tools/list`,
//! `tools/call`), by hand, so there is nothing to keep in step with an SDK.
//!
//! Every tool ends in [`App::call`], the same dispatcher the desktop app
//! uses, so an agent gets exactly the app's rules: double-entry checks, the
//! lock date, license gates. On top of that:
//!
//! - **Read-only unless asked.** Tools that change the book exist only when
//!   the server is started with `--write`.
//! - **Some things are never an agent's job.** Deleting books, encryption
//!   and recovery keys, licenses, cloud destinations and bank credentials are
//!   refused outright, whatever the mode.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tally_api::App;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// MCP revisions this server speaks, newest first.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub struct Options {
    /// Offer the tools that change the book.
    pub write: bool,
    /// The book to open; the most recently used one when absent.
    pub book: Option<PathBuf>,
}

/// Where the installed desktop app keeps its config (recent books, license):
/// Tauri's app-config dir for the `app.tallybook.desktop` identifier.
pub fn desktop_config_dir() -> PathBuf {
    let id = "app.tallybook.desktop";
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let home = env("HOME").unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support").join(id)
    } else if cfg!(windows) {
        env("APPDATA").unwrap_or(home).join(id)
    } else {
        env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")).join(id)
    }
}

// ------------------------------------------------------------------ policy

/// Methods that only read. Everything else changes something.
const READ: &[&str] = &[
    "book.status", "book.recent", "book.default_dir", "book.open", "book.close",
    "settings.get", "dashboard",
    "reports.pnl", "reports.balance_sheet", "reports.trial_balance", "reports.ar_aging", "reports.ap_aging",
    "accounts.list", "accounts.subtypes", "accounts.register", "entries.get", "entries.list",
    "people.list", "budget.month", "recurring.list", "recurring.due", "recurring.suggestions", "goals.list",
    "household.alerts", "household.opportunities", "household.score", "household.wins",
    "inbox.list", "inbox.reviewed", "inbox.count", "inbox.candidates",
    "import.csv_preview", "import.ofx_preview", "import.ledger_preview",
    "rules.list", "reconcile.active", "reconcile.history", "reconcile.candidates", "reconcile.summary",
    "contacts.list", "contacts.get", "invoices.list", "invoices.get", "bills.list", "bills.get", "payments.list",
    "bank.connections", "bank.feed_accounts", "bank.suggest_opening",
    "tax.years", "tax.lines", "tax.profile", "tax.schedule_c", "tax.estimate", "tax.1099",
    "cloud.status", "license.status",
];

/// Methods that change the book (or the app's settings) and are offered to
/// an agent only with `--write`. Anything not in READ or NEVER is treated as
/// a write; this list exists so that a new method must be sorted
/// into READ, WRITE or NEVER on purpose (the `every_dispatched_method_is_in_exactly_one_list` test).
#[cfg(test)]
const WRITE: &[&str] = &[
    "book.create", "book.create_demo", "book.forget", "book.backup", "book.add_side", "settings.set",
    "accounts.create", "accounts.rename", "accounts.archive", "accounts.set_flex", "accounts.opening_balance",
    "entries.post", "entries.void", "entries.set_person", "entries.set_goal", "people.create",
    "people.update", "people.archive", "budget.set", "budget.remove", "budget.set_available",
    "recurring.create", "recurring.update", "recurring.set_active", "recurring.mark",
    "recurring.dismiss_suggestion", "goals.create", "goals.update", "goals.archive", "goals.contribute",
    "inbox.accept", "inbox.accept_suggested", "inbox.split", "inbox.match", "inbox.exclude", "inbox.undo",
    "import.csv", "import.ofx", "import.ledger", "rules.create", "rules.update", "rules.delete",
    "reconcile.start", "reconcile.toggle", "reconcile.toggle_many", "reconcile.undo_last", "reconcile.finish",
    "reconcile.cancel", "contacts.create", "contacts.update", "contacts.archive", "invoices.create",
    "invoices.update", "invoices.finalize", "invoices.void", "invoices.delete", "bills.create", "bills.void",
    "payments.receive", "payments.pay", "payments.void", "tax.set_profile", "tax.set_line", "tax.confirm_all",
    "tax.record_payment", "tax.void_payment", "tax.set_payee", "tax.set_contact", "tax.export", "bank.sync",
];

/// Methods an agent may never call: they destroy books, handle keys or
/// credentials, spend money, or send data somewhere new.
const NEVER: &[&str] = &[
    "book.delete", "book.unlock", "book.encrypt", "book.decrypt", "book.recovery_key", "book.delete_plain_backups",
    "license.activate", "license.deactivate", "cloud.set", "cloud.backup_now",
    "bank.plaid_config_get", "bank.plaid_config_set", "bank.plaid_start", "bank.plaid_relink_start", "bank.plaid_poll",
    "bank.simplefin_connect", "bank.simplefin_reconnect", "bank.link", "bank.unlink", "bank.disconnect",
];

#[derive(Debug, PartialEq, Eq)]
enum Access {
    Read,
    Write,
    Never,
}

fn access(method: &str) -> Access {
    if NEVER.contains(&method) {
        Access::Never
    } else if READ.contains(&method) {
        Access::Read
    } else {
        Access::Write
    }
}

// ------------------------------------------------------------------ tools

struct Tool {
    name: &'static str,
    write: bool,
    description: &'static str,
    schema: fn() -> Value,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

const SCOPE: &str = "\"business\" or \"household\"; the whole book when absent.";

fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "book_status",
            write: false,
            description: "Which book is open, its sides (business, household), today's date, the license tier, how many bank transactions wait for review, and the recently used books. Start here.",
            schema: || obj(json!({}), &[]),
        },
        Tool {
            name: "open_book",
            write: false,
            description: "Open another .tallybook file, by path (see recent_books from book_status). Encrypted books open only if this computer's keychain holds their key.",
            schema: || obj(json!({ "path": { "type": "string", "description": "Path to a .tallybook file." } }), &["path"]),
        },
        Tool {
            name: "accounts",
            write: false,
            description: "The chart of accounts with balances: banks, cards, income and spending categories. Account ids from here are what other tools take.",
            schema: || obj(json!({ "scope": { "type": "string", "enum": ["business", "household"], "description": SCOPE } }), &[]),
        },
        Tool {
            name: "account_register",
            write: false,
            description: "Every line posted to one account, with a running balance, like a check register.",
            schema: || obj(json!({ "account_id": { "type": "integer" } }), &["account_id"]),
        },
        Tool {
            name: "entries",
            write: false,
            description: "Journal entries dated within a range, each with its lines (account_id, amount in cents: positive is a debit, negative a credit).",
            schema: || {
                obj(
                    json!({ "from": { "type": "string", "format": "date" }, "to": { "type": "string", "format": "date" } }),
                    &["from", "to"],
                )
            },
        },
        Tool {
            name: "report",
            write: false,
            description: "A financial report. pnl (profit & loss) takes from/to and defaults to this year so far; balance_sheet, trial_balance, ar_aging (who owes you) and ap_aging (who you owe) take as_of, default today.",
            schema: || {
                obj(
                    json!({
                        "report": { "type": "string", "enum": ["pnl", "balance_sheet", "trial_balance", "ar_aging", "ap_aging"] },
                        "from": { "type": "string", "format": "date" },
                        "to": { "type": "string", "format": "date" },
                        "as_of": { "type": "string", "format": "date" },
                        "scope": { "type": "string", "enum": ["business", "household"], "description": SCOPE }
                    }),
                    &["report"],
                )
            },
        },
        Tool {
            name: "dashboard",
            write: false,
            description: "The home screen's numbers: cash on hand, money in and out this month, what's owed both ways, and what needs attention.",
            schema: || obj(json!({ "scope": { "type": "string", "enum": ["business", "household"], "description": "Which side; the book's main side when absent." } }), &[]),
        },
        Tool {
            name: "review_inbox",
            write: false,
            description: "Bank transactions waiting to be categorized, each with Tallybook's suggestion (a matching entry, a rule, or what was picked last time) when it has one.",
            schema: || obj(json!({ "account_id": { "type": "integer", "description": "Only this bank account's transactions." } }), &[]),
        },
        Tool {
            name: "household_month",
            write: false,
            description: "The household's month: the budget plan against what's been spent (with pace for the days left), alerts (lines running hot, unpaid bills) and wins. Household side only.",
            schema: || obj(json!({ "month": { "type": "string", "pattern": "^\\d{4}-\\d{2}$", "description": "YYYY-MM; this month when absent." } }), &[]),
        },
        Tool {
            name: "bills_due",
            write: false,
            description: "Household bill due dates in a window (default: a week ago through the end of next month), each paid, overdue, coming up or skipped.",
            schema: || obj(json!({ "from": { "type": "string", "format": "date" }, "to": { "type": "string", "format": "date" } }), &[]),
        },
        Tool {
            name: "goals",
            write: false,
            description: "Savings goals: target, saved so far, and when each will be reached at the current pace.",
            schema: || obj(json!({}), &[]),
        },
        Tool {
            name: "records",
            write: false,
            description: "List one kind of record: contacts (customers and vendors), people (household members), invoices, bills (to pay), payments, rules (auto-categorization), recurring_bills, or bank_connections.",
            schema: || {
                obj(
                    json!({ "kind": { "type": "string", "enum": ["contacts", "people", "invoices", "bills", "payments", "rules", "recurring_bills", "bank_connections"] } }),
                    &["kind"],
                )
            },
        },
        Tool {
            name: "tax_summary",
            write: false,
            description: "The Schedule C worksheet, the quarterly estimated-tax plan and the 1099 summary for a tax year. Worksheets and estimates, not tax advice. Needs a Business + Tax license.",
            schema: || obj(json!({ "year": { "type": "integer", "description": "This year when absent." } }), &[]),
        },
        Tool {
            name: "categorize",
            write: true,
            description: "File one inbox transaction under an account (a category, or a transfer account). Set remember to create a rule so similar transactions file themselves.",
            schema: || {
                obj(
                    json!({
                        "transaction_id": { "type": "integer", "description": "From review_inbox." },
                        "account_id": { "type": "integer", "description": "Where it belongs, from accounts." },
                        "memo": { "type": "string" },
                        "remember": { "type": "boolean" },
                        "person_id": { "type": "integer", "description": "Household: who it was for." },
                        "payee_id": { "type": "integer", "description": "Business: the contact paid or paying (feeds 1099s)." }
                    }),
                    &["transaction_id", "account_id"],
                )
            },
        },
        Tool {
            name: "accept_suggestions",
            write: true,
            description: "Accept Tallybook's own suggestion for each of these inbox transactions, as the Review screen's Accept button does.",
            schema: || obj(json!({ "transaction_ids": { "type": "array", "items": { "type": "integer" }, "minItems": 1 } }), &["transaction_ids"]),
        },
        Tool {
            name: "record_entry",
            write: true,
            description: "Post a journal entry. Lines must balance: amounts in integer cents, positive debits and negative credits summing to zero. E.g. $42.50 of groceries paid from checking: groceries +4250, checking -4250.",
            schema: || {
                obj(
                    json!({
                        "date": { "type": "string", "format": "date" },
                        "memo": { "type": "string" },
                        "lines": {
                            "type": "array",
                            "minItems": 2,
                            "items": obj(json!({ "account_id": { "type": "integer" }, "amount": { "type": "integer" }, "memo": { "type": "string" } }), &["account_id", "amount"])
                        },
                        "person_id": { "type": "integer" },
                        "payee_id": { "type": "integer" }
                    }),
                    &["date", "memo", "lines"],
                )
            },
        },
        Tool {
            name: "void_entry",
            write: true,
            description: "Void an entry. Nothing is deleted: a reversing entry is posted, dated today unless a date is given, so history stays intact.",
            schema: || obj(json!({ "entry_id": { "type": "integer" }, "date": { "type": "string", "format": "date" } }), &["entry_id"]),
        },
        Tool {
            name: "set_budget",
            write: true,
            description: "Set one line of a household month's plan, for a category account or a person, in cents. Plans carry forward to later months that have none of their own. direction: ceiling (spend at most, default) or floor (get at least, e.g. savings or income).",
            schema: || {
                obj(
                    json!({
                        "month": { "type": "string", "pattern": "^\\d{4}-\\d{2}$" },
                        "account_id": { "type": "integer" },
                        "person_id": { "type": "integer" },
                        "amount": { "type": "integer", "minimum": 0 },
                        "direction": { "type": "string", "enum": ["ceiling", "floor"] }
                    }),
                    &["month", "amount"],
                )
            },
        },
        Tool {
            name: "call",
            write: false,
            description: "Any other Tallybook method, by name, with its JSON params: e.g. invoices.create, bills.create, payments.receive, recurring.create, goals.contribute, rules.create, reconcile.start, bank.sync. Read-only servers refuse methods that change the book; deleting books, keys, licenses, cloud destinations and bank credentials are refused always.",
            schema: || obj(json!({ "method": { "type": "string" }, "params": { "type": "object" } }), &["method"]),
        },
    ]
}

/// One tool call, mapped onto API methods.
async fn run_tool(app: &App, write: bool, name: &str, a: &Value) -> Result<Value, String> {
    let call = |method: &'static str, params: Value| async move { api(app, write, method, params).await };
    let arg = |k: &str| a.get(k).cloned().unwrap_or(Value::Null);
    match name {
        "book_status" => {
            let status = call("book.status", Value::Null).await?;
            let license = call("license.status", Value::Null).await?;
            let inbox = if status["open"] == true { call("inbox.count", Value::Null).await.ok() } else { None };
            let recent = call("book.recent", Value::Null).await?;
            Ok(json!({ "book": status, "license": license["name"], "inbox_waiting": inbox, "recent_books": recent }))
        }
        "open_book" => call("book.open", json!({ "path": arg("path") })).await,
        "accounts" => {
            let all = call("accounts.list", Value::Null).await?;
            Ok(match a.get("scope").and_then(Value::as_str) {
                Some(scope) => Value::Array(all.as_array().into_iter().flatten().filter(|x| x["scope"] == scope).cloned().collect()),
                None => all,
            })
        }
        "account_register" => call("accounts.register", json!({ "id": arg("account_id") })).await,
        "entries" => call("entries.list", json!({ "from": arg("from"), "to": arg("to") })).await,
        "report" => {
            let scope = arg("scope");
            match a.get("report").and_then(Value::as_str) {
                Some("pnl") => {
                    let to = a.get("to").cloned().unwrap_or_else(|| json!(tally_api::today()));
                    let from = a.get("from").cloned().unwrap_or_else(|| json!(tally_api::year_start()));
                    call("reports.pnl", json!({ "from": from, "to": to, "scope": scope })).await
                }
                Some(r @ ("balance_sheet" | "trial_balance" | "ar_aging" | "ap_aging")) => {
                    let method = match r {
                        "balance_sheet" => "reports.balance_sheet",
                        "trial_balance" => "reports.trial_balance",
                        "ar_aging" => "reports.ar_aging",
                        _ => "reports.ap_aging",
                    };
                    call(method, json!({ "as_of": arg("as_of"), "scope": scope })).await
                }
                _ => Err("report must be pnl, balance_sheet, trial_balance, ar_aging or ap_aging".into()),
            }
        }
        "dashboard" => call("dashboard", json!({ "scope": arg("scope") })).await,
        "review_inbox" => call("inbox.list", json!({ "account_id": arg("account_id") })).await,
        "household_month" => {
            let p = json!({ "month": arg("month") });
            Ok(json!({
                "budget": call("budget.month", p.clone()).await?,
                "alerts": call("household.alerts", p.clone()).await?,
                "wins": call("household.wins", p).await?,
            }))
        }
        "bills_due" => call("recurring.due", json!({ "from": arg("from"), "to": arg("to") })).await,
        "goals" => call("goals.list", Value::Null).await,
        "records" => {
            let method = match a.get("kind").and_then(Value::as_str) {
                Some("contacts") => "contacts.list",
                Some("people") => "people.list",
                Some("invoices") => "invoices.list",
                Some("bills") => "bills.list",
                Some("payments") => "payments.list",
                Some("rules") => "rules.list",
                Some("recurring_bills") => "recurring.list",
                Some("bank_connections") => "bank.connections",
                _ => return Err("unknown kind".into()),
            };
            call(method, json!({})).await
        }
        "tax_summary" => {
            let p = json!({ "year": arg("year") });
            Ok(json!({
                "schedule_c": call("tax.schedule_c", p.clone()).await?,
                "estimate": call("tax.estimate", p.clone()).await?,
                "form_1099": call("tax.1099", p).await?,
            }))
        }
        "categorize" => {
            let p = json!({
                "id": arg("transaction_id"), "account_id": arg("account_id"), "memo": arg("memo"),
                "remember": a.get("remember").cloned().unwrap_or(json!(false)),
                "person_id": arg("person_id"), "payee_id": arg("payee_id"),
            });
            call("inbox.accept", p).await
        }
        "accept_suggestions" => call("inbox.accept_suggested", json!({ "ids": arg("transaction_ids") })).await,
        "record_entry" => {
            let p = json!({ "date": arg("date"), "memo": arg("memo"), "lines": arg("lines"), "person_id": arg("person_id"), "payee_id": arg("payee_id") });
            call("entries.post", p).await
        }
        "void_entry" => call("entries.void", json!({ "id": arg("entry_id"), "date": arg("date") })).await,
        "set_budget" => {
            let target = match (a.get("account_id"), a.get("person_id")) {
                (Some(id), None) => json!({ "kind": "account", "id": id }),
                (None, Some(id)) => json!({ "kind": "person", "id": id }),
                _ => return Err("give exactly one of account_id or person_id".into()),
            };
            let line = json!({ "target": target, "amount": arg("amount"), "direction": a.get("direction").cloned().unwrap_or(json!("ceiling")) });
            call("budget.set", json!({ "month": arg("month"), "line": line })).await
        }
        "call" => {
            let method = a.get("method").and_then(Value::as_str).ok_or("method is required")?;
            api(app, write, method, a.get("params").cloned().unwrap_or(Value::Null)).await
        }
        _ => Err(format!("no such tool: {name}")),
    }
}

/// [`App::call`], behind the access policy.
async fn api(app: &App, write: bool, method: &str, params: Value) -> Result<Value, String> {
    match access(method) {
        Access::Never => return Err(format!("{method} isn't available to agents; do it in the Tallybook app")),
        Access::Write if !write => {
            return Err(format!("{method} changes the book, and this server is read-only (start it with --write to allow changes)"));
        }
        _ => {}
    }
    app.call(method, params).await.map_err(|e| format!("{} ({})", e.message, e.code))
}

// ------------------------------------------------------------------ protocol

const INSTRUCTIONS: &str = "Tallybook is double-entry bookkeeping and household budgeting kept in a local file. \
Money is integer cents (12345 = $123.45); dates are YYYY-MM-DD; months are YYYY-MM. A book has a business side, \
a household side, or both. Call book_status first, then accounts to learn account ids. Prefer the review_inbox → \
categorize flow for bank transactions; use record_entry only for things the bank won't show (cash, adjustments). \
Tax figures are worksheets and estimates, not tax advice.";

fn tool_list(write: bool) -> Value {
    let list: Vec<Value> = tools()
        .into_iter()
        .filter(|t| write || !t.write)
        .map(|t| {
            // `call` can change the book exactly when the server allows writes.
            let read_only = if t.name == "call" { !write } else { !t.write };
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": { "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false }
            })
        })
        .collect();
    json!({ "tools": list })
}

/// Answers one JSON-RPC message; `None` for notifications.
async fn handle(app: &App, opts: &Options, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOLS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOLS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "tallybook", "title": "Tallybook", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tool_list(opts.write)),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let known = tools().into_iter().find(|t| t.name == name);
            let outcome = match known {
                Some(t) if t.write && !opts.write => Err(format!("{name} changes the book, and this server is read-only")),
                Some(_) => run_tool(app, opts.write, name, &args).await,
                None => Err(format!("no such tool: {name}")),
            };
            // Tool failures are results the model should see, not protocol errors.
            Ok(match outcome {
                Ok(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }], "isError": false }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            })
        }
        m => Err((-32601, format!("method not found: {m}"))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    })
}

/// Opens the requested (or most recent) book, then serves MCP on `input`/`output`
/// until the input closes.
pub async fn serve<R, W>(app: Arc<App>, opts: Options, input: R, mut output: W) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let path = match &opts.book {
        Some(p) => Some(json!(p)),
        None => app.call("book.recent", Value::Null).await.ok().and_then(|r| r.get(0).map(|b| b["path"].clone())),
    };
    if let Some(path) = path
        && let Err(e) = app.call("book.open", json!({ "path": path })).await
    {
        eprintln!("tallybook-mcp: couldn't open {path}: {}", e.message);
    }

    let mut lines = input.lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&app, &opts, &msg).await,
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(reply) = reply {
            output.write_all(reply.to_string().as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}

/// [`serve`] on this process's stdin and stdout.
pub async fn serve_stdio(app: Arc<App>, opts: Options) -> std::io::Result<()> {
    serve(app, opts, tokio::io::BufReader::new(tokio::io::stdin()), tokio::io::stdout()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tally_api::MemorySecrets;

    async fn session(write: bool, messages: &[Value]) -> Vec<Value> {
        let dir = std::env::temp_dir().join(format!("tally-mcp-{}-{write}-{}", std::process::id(), messages.len()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = Arc::new(App::new(Arc::new(MemorySecrets::default()), dir.join("config")));
        app.call("book.create_demo", json!({ "path": dir.join("demo.tallybook") })).await.unwrap();
        let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
        let mut out = Vec::new();
        serve(app, Options { write, book: None }, input.as_bytes(), &mut out).await.unwrap();
        String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    fn req(id: i64, method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    fn tool(id: i64, name: &str, args: Value) -> Value {
        req(id, "tools/call", json!({ "name": name, "arguments": args }))
    }

    #[tokio::test]
    async fn handshake_and_read_only_tools() {
        let r = session(
            false,
            &[
                req(1, "initialize", json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "test" } })),
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                req(2, "tools/list", json!({})),
                tool(3, "book_status", json!({})),
                tool(4, "report", json!({ "report": "pnl" })),
                tool(5, "categorize", json!({ "transaction_id": 1, "account_id": 1 })),
                tool(6, "call", json!({ "method": "entries.void", "params": { "id": 1 } })),
                tool(7, "call", json!({ "method": "book.recovery_key" })),
                req(8, "nope", json!({})),
            ],
        )
        .await;
        assert_eq!(r.len(), 8, "the notification gets no reply");
        assert_eq!(r[0]["result"]["protocolVersion"], "2025-03-26");
        let names: Vec<&str> = r[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"review_inbox") && !names.contains(&"categorize"), "{names:?}");

        let status: Value = serde_json::from_str(r[2]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(status["book"]["company_name"], "Northwind Studio", "the most recent book opened by itself");
        assert!(status["inbox_waiting"].as_i64().unwrap() > 0);

        assert_eq!(r[3]["result"]["isError"], false);
        assert!(r[3]["result"]["content"][0]["text"].as_str().unwrap().contains("total_income"));
        assert_eq!(r[4]["result"]["isError"], true, "write tools don't exist when read-only");
        assert!(r[5]["result"]["content"][0]["text"].as_str().unwrap().contains("read-only"));
        assert!(r[6]["result"]["content"][0]["text"].as_str().unwrap().contains("isn't available to agents"));
        assert_eq!(r[7]["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn writes_go_through_the_books_rules() {
        let r = session(
            true,
            &[
                tool(1, "review_inbox", json!({})),
                tool(2, "record_entry", json!({ "date": "2026-01-02", "memo": "Unbalanced", "lines": [
                    { "account_id": 1, "amount": 500 }, { "account_id": 2, "amount": -400 }
                ] })),
                req(3, "tools/list", json!({})),
            ],
        )
        .await;
        let inbox: Value = serde_json::from_str(r[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(!inbox.as_array().unwrap().is_empty());
        assert_eq!(r[1]["result"]["isError"], true, "double-entry still has to balance: {}", r[1]);
        let names: Vec<&str> = r[2]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"categorize") && names.contains(&"record_entry"));
    }

    #[test]
    fn policy_sorts_every_method() {
        assert_eq!(access("reports.pnl"), Access::Read);
        assert_eq!(access("inbox.accept"), Access::Write);
        assert_eq!(access("book.delete"), Access::Never);
        assert!(READ.iter().all(|m| !NEVER.contains(m)), "a method is either readable or forbidden, not both");
    }

    /// Every method the API dispatches is sorted on purpose into exactly one of
    /// READ, WRITE and NEVER, so a new method can't become agent-writable (or
    /// agent-readable) by default; and the lists name no method that doesn't exist.
    #[test]
    fn every_dispatched_method_is_in_exactly_one_list() {
        for m in tally_api::METHODS {
            let n = [READ.contains(m), WRITE.contains(m), NEVER.contains(m)].into_iter().filter(|b| *b).count();
            assert_eq!(n, 1, "{m} must be in exactly one of READ, WRITE, NEVER (in {n})");
        }
        for m in READ.iter().chain(WRITE).chain(NEVER) {
            assert!(tally_api::METHODS.contains(m), "{m} is in an access list but is not an API method");
        }
    }
}
