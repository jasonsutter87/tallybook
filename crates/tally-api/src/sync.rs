//! Bank connections: linking (Plaid Hosted Link, SimpleFIN), syncing, unlinking.
//!
//! Network calls never hold the book lock: read what's needed, drop the
//! lock, await the provider, then lock again to write.

use crate::{ApiError, ApiResult, App, ok, params, today};
use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use tally_core::bank::{FeedAccount, FeedBatch};
use tally_core::{AccountId, AccountSubtype, Cents};
use tally_feed::{plaid, simplefin};
use tally_store::*;
use std::result::Result;

const PLAID_CREDS: &str = "plaid.credentials";

#[derive(Deserialize)]
struct ConnectionParam {
    connection_id: ConnectionId,
}

#[derive(Deserialize)]
struct MaybeConnection {
    connection_id: Option<ConnectionId>,
}

#[derive(Deserialize)]
struct Poll {
    link_token: String,
    /// Set when repairing an existing connection.
    connection_id: Option<ConnectionId>,
}

#[derive(Deserialize)]
struct SetupToken {
    setup_token: String,
}

#[derive(Deserialize)]
struct SimplefinReconnect {
    connection_id: ConnectionId,
    setup_token: String,
}

#[derive(Deserialize)]
struct NewLedgerAccount {
    name: String,
    subtype: AccountSubtype,
    /// Defaults to the book's main side.
    #[serde(default)]
    scope: Option<tally_core::Scope>,
}

#[derive(Deserialize)]
struct Link {
    connection_id: ConnectionId,
    external_account_id: String,
    name: String,
    mask: Option<String>,
    /// Link to an existing account...
    account_id: Option<AccountId>,
    /// ...or create one.
    new_account: Option<NewLedgerAccount>,
    start_date: NaiveDate,
}

#[derive(Deserialize)]
struct LinkParam {
    link_id: LinkId,
}

#[derive(Deserialize)]
struct AccountParam {
    account_id: AccountId,
}

#[derive(Serialize)]
struct ConnectionView {
    #[serde(flatten)]
    connection: BankConnection,
    links: Vec<BankLink>,
}

#[derive(Serialize)]
struct PlaidConfigView {
    configured: bool,
    environment: Option<plaid::Environment>,
    client_id_hint: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PollView {
    Waiting,
    Exited,
    Linked { connection_id: ConnectionId, institution: String, accounts: Vec<FeedAccount> },
}

#[derive(Serialize)]
struct SyncReport {
    connection_id: ConnectionId,
    institution: String,
    ok: bool,
    error: Option<String>,
    imported: ImportSummary,
}

#[derive(Serialize)]
struct OpeningSuggestion {
    date: NaiveDate,
    amount: Cents,
    bank_balance: Cents,
}

/// What a provider returned, keyed by the provider's account id.
struct Fetched {
    batches: HashMap<String, FeedBatch>,
    balances: HashMap<String, Option<Cents>>,
    cursor: String,
}

/// `…1234`: the last four characters (not bytes, so a pasted non-ASCII id can't panic).
fn client_id_hint(client_id: &str) -> String {
    let tail: Vec<char> = client_id.chars().rev().take(4).collect();
    format!("…{}", tail.into_iter().rev().collect::<String>())
}

fn plaid_err(e: plaid::PlaidError) -> ApiError {
    ApiError::new("bank", e.to_string())
}

fn sfin_err(e: simplefin::SimpleFinError) -> ApiError {
    ApiError::new("bank", e.to_string())
}

impl App {
    fn plaid_client(&self) -> ApiResult<plaid::Client> {
        let raw = self
            .secrets
            .get(PLAID_CREDS)
            .map_err(|e| ApiError::new("keychain", e))?
            .ok_or_else(|| ApiError::new("plaid_not_configured", "add your Plaid keys first: Bank connections → Connect a bank → Plaid"))?;
        let creds: plaid::Credentials = serde_json::from_str(&raw).map_err(|e| ApiError::new("keychain", e.to_string()))?;
        Ok(plaid::Client::new(creds))
    }

    fn secret(&self, conn: &BankConnection) -> ApiResult<String> {
        let key = conn.secret_ref.as_deref().ok_or_else(|| ApiError::new("rule", "this connection has no saved credentials"))?;
        self.secrets
            .get(key)
            .map_err(|e| ApiError::new("keychain", e))?
            .ok_or_else(|| ApiError::new("rule", "this connection's credentials are missing from the keychain; reconnect it"))
    }

    fn plaid_user_id(&self) -> ApiResult<String> {
        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        if let Some(id) = book.store.pref("plaid.user_id")? {
            return Ok(id);
        }
        let id = uuid::Uuid::new_v4().to_string();
        book.store.set_pref("plaid.user_id", Some(&id))?;
        Ok(id)
    }

    pub(crate) async fn bank(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "bank.connections" => self.with(|s| {
                let links = s.links()?;
                Ok(s.connections()?
                    .into_iter()
                    .map(|c| ConnectionView { links: links.iter().filter(|l| l.connection_id == c.id).cloned().collect(), connection: c })
                    .collect::<Vec<_>>())
            }),

            "bank.plaid_config_get" => {
                let raw = self.secrets.get(PLAID_CREDS).map_err(|e| ApiError::new("keychain", e))?;
                let creds = raw.and_then(|r| serde_json::from_str::<plaid::Credentials>(&r).ok());
                ok(PlaidConfigView {
                    configured: creds.is_some(),
                    environment: creds.as_ref().map(|c| c.environment),
                    client_id_hint: creds.map(|c| client_id_hint(&c.client_id)),
                })
            }
            "bank.plaid_config_set" => {
                let creds: plaid::Credentials = params(p)?;
                if creds.client_id.trim().is_empty() || creds.secret.trim().is_empty() {
                    return Err(ApiError::new("rule", "both the client ID and secret are required"));
                }
                let json = serde_json::to_string(&creds).map_err(|e| ApiError::new("internal", e.to_string()))?;
                self.secrets.set(PLAID_CREDS, &json).map_err(|e| ApiError::new("keychain", e))?;
                ok(())
            }
            "bank.plaid_start" => {
                let user = self.plaid_user_id()?;
                let link = self.plaid_client()?.create_hosted_link(&user, None).await.map_err(plaid_err)?;
                ok(link)
            }
            "bank.plaid_relink_start" => {
                let p: ConnectionParam = params(p)?;
                let user = self.plaid_user_id()?;
                let conn = self.run(|s| s.connection(p.connection_id))?;
                let token = self.secret(&conn)?;
                let link = self.plaid_client()?.create_hosted_link(&user, Some(&token)).await.map_err(plaid_err)?;
                ok(link)
            }
            "bank.plaid_poll" => {
                let p: Poll = params(p)?;
                let client = self.plaid_client()?;
                match client.link_status(&p.link_token).await.map_err(plaid_err)? {
                    plaid::LinkStatus::Waiting => ok(PollView::Waiting),
                    plaid::LinkStatus::Exited => ok(PollView::Exited),
                    plaid::LinkStatus::Done(result) => {
                        if let Some(id) = p.connection_id {
                            // Update mode: same item, fresh login. Nothing to exchange.
                            self.with(|s| s.set_connection_status(id, ConnectionStatus::Ok, None))?;
                            return ok(PollView::Linked { connection_id: id, institution: String::new(), accounts: vec![] });
                        }
                        let item = client.exchange_public_token(&result.public_token).await.map_err(plaid_err)?;
                        let accounts = client.accounts(&item.access_token).await.map_err(plaid_err)?;
                        let secret_ref = format!("plaid:{}", uuid::Uuid::new_v4());
                        self.secrets.set(&secret_ref, &item.access_token).map_err(|e| ApiError::new("keychain", e))?;
                        let institution = result.institution_name.unwrap_or_else(|| "Bank".into());
                        let id = self
                            .run(|s| s.create_connection(Provider::Plaid, &institution, Some(&item.item_id), Some(&secret_ref)))?;
                        ok(PollView::Linked { connection_id: id, institution, accounts })
                    }
                }
            }
            "bank.simplefin_connect" => {
                let p: SetupToken = params(p)?;
                let access_url = simplefin::claim(&p.setup_token).await.map_err(sfin_err)?;
                let since = today() - Days::new(30);
                let accounts = simplefin::fetch(&access_url, since).await.map_err(sfin_err)?;
                let secret_ref = format!("simplefin:{}", uuid::Uuid::new_v4());
                self.secrets.set(&secret_ref, &access_url).map_err(|e| ApiError::new("keychain", e))?;
                let institution = accounts.iter().find_map(|a| a.institution.clone()).unwrap_or_else(|| "SimpleFIN".into());
                let id = self
                    .run(|s| s.create_connection(Provider::Simplefin, &institution, None, Some(&secret_ref)))?;
                ok(PollView::Linked { connection_id: id, institution, accounts: accounts.into_iter().map(|a| a.account).collect() })
            }
            "bank.simplefin_reconnect" => {
                let p: SimplefinReconnect = params(p)?;
                let conn = self.run(|s| s.connection(p.connection_id))?;
                if conn.provider != Provider::Simplefin {
                    return Err(ApiError::new("rule", "that isn't a SimpleFIN connection"));
                }
                let key = conn.secret_ref.clone().ok_or_else(|| ApiError::new("rule", "this connection was disconnected; connect it again instead"))?;
                let access_url = simplefin::claim(&p.setup_token).await.map_err(sfin_err)?;
                // Same connection, same links: only the credential changes.
                self.secrets.set(&key, &access_url).map_err(|e| ApiError::new("keychain", e))?;
                self.with(|s| s.set_connection_status(p.connection_id, ConnectionStatus::Ok, None))
            }
            "bank.feed_accounts" => {
                let p: ConnectionParam = params(p)?;
                let conn = self.run(|s| s.connection(p.connection_id))?;
                let secret = self.secret(&conn)?;
                let accounts = match conn.provider {
                    Provider::Plaid => self.plaid_client()?.accounts(&secret).await.map_err(plaid_err)?,
                    Provider::Simplefin => simplefin::fetch(&secret, today()).await.map_err(sfin_err)?.into_iter().map(|a| a.account).collect(),
                    Provider::File => vec![],
                };
                ok(accounts)
            }
            "bank.link" => {
                let p: Link = params(p)?;
                self.with_api(|s| {
                    let account_id = match (p.account_id, p.new_account) {
                        (Some(id), _) => id,
                        (None, Some(n)) => {
                            let scope = match n.scope {
                                Some(scope) => scope,
                                None => crate::main_side(s)?,
                            };
                            s.create_account(&NewAccount { code: None, name: n.name, subtype: n.subtype, parent_id: None, scope, flex: None })?
                        }
                        (None, None) => return Err(ApiError::new("bad_params", "choose an account or create a new one")),
                    };
                    let link = s.link_account(p.connection_id, &p.external_account_id, account_id, &p.name, p.mask.as_deref(), p.start_date)?;
                    // Re-pull history so the newly linked account gets its transactions
                    // (import is idempotent, so already-seen ones are skipped).
                    s.record_sync(p.connection_id, "")?;
                    Ok(json!({ "link_id": link, "account_id": account_id }))
                })
            }
            "bank.unlink" => {
                let p: LinkParam = params(p)?;
                self.with(|s| s.unlink_account(p.link_id))
            }
            "bank.disconnect" => {
                let p: ConnectionParam = params(p)?;
                let conn = self.run(|s| s.connection(p.connection_id))?;
                if conn.provider == Provider::Plaid
                    && let Ok(token) = self.secret(&conn)
                    && let Ok(client) = self.plaid_client()
                {
                    // Best effort: revoke at Plaid; disconnect locally regardless.
                    let _ = client.remove_item(&token).await;
                }
                if let Some(key) = &conn.secret_ref {
                    let _ = self.secrets.delete(key);
                }
                self.with(|s| s.disconnect(p.connection_id))
            }
            "bank.sync" => {
                let p: MaybeConnection = params(p)?;
                let ids: Vec<ConnectionId> = match p.connection_id {
                    Some(id) => vec![id],
                    None => self
                        .run(|s| Ok(s.connections()?.into_iter().filter(|c| c.provider != Provider::File).map(|c| c.id).collect()))?,
                };
                let mut reports = Vec::new();
                for id in ids {
                    reports.push(self.sync_one(id).await?);
                }
                ok(reports)
            }
            "bank.suggest_opening" => {
                let p: AccountParam = params(p)?;
                self.with_api(|s| suggest_opening(s, p.account_id))
            }
            _ => Err(ApiError::new("unknown_method", format!("no such method: {method}"))),
        }
    }

    async fn sync_one(&self, id: ConnectionId) -> ApiResult<SyncReport> {
        let (conn, links) = {
            let guard = self.lock();
            let book = guard.as_ref().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
            let conn = book.store.connection(id)?;
            let links: Vec<BankLink> = book.store.links()?.into_iter().filter(|l| l.connection_id == id).collect();
            (conn, links)
        };
        let mut report = SyncReport { connection_id: id, institution: conn.institution.clone(), ok: true, error: None, imported: ImportSummary::default() };
        if links.is_empty() {
            return Ok(report);
        }
        let secret = self.secret(&conn)?;

        // Fetch outside the lock.
        let fetched: Result<Fetched, (ConnectionStatus, String)> = match conn.provider {
            Provider::Plaid => {
                let client = self.plaid_client()?;
                match client.sync(&secret, &conn.cursor).await {
                    Err(e) => Err((if e.needs_relink() { ConnectionStatus::NeedsRelink } else { ConnectionStatus::Error }, e.to_string())),
                    Ok(result) => {
                        let mut per: HashMap<String, FeedBatch> = HashMap::new();
                        for t in result.batch.added {
                            let acct = result.account_of.get(&t.external_id).cloned().unwrap_or_default();
                            per.entry(acct).or_default().added.push(t);
                        }
                        for t in result.batch.modified {
                            let acct = result.account_of.get(&t.external_id).cloned().unwrap_or_default();
                            per.entry(acct).or_default().modified.push(t);
                        }
                        // Removed ids carry no account in our batch; apply to every link (ids are unique).
                        for l in &links {
                            per.entry(l.external_account_id.clone()).or_default().removed.extend(result.batch.removed.iter().cloned());
                        }
                        let balances = client
                            .accounts(&secret)
                            .await
                            .map(|accts| accts.into_iter().map(|a| (a.external_id, a.current_balance)).collect())
                            .unwrap_or_default();
                        Ok(Fetched { batches: per, balances, cursor: result.next_cursor })
                    }
                }
            }
            Provider::Simplefin => {
                let since = links.iter().map(|l| l.start_date).min().unwrap_or_else(today).max(today() - Days::new(89));
                match simplefin::fetch(&secret, since).await {
                    Err(e) => Err((
                        if matches!(e, simplefin::SimpleFinError::Unauthorized(_)) { ConnectionStatus::NeedsRelink } else { ConnectionStatus::Error },
                        e.to_string(),
                    )),
                    Ok(accounts) => {
                        let mut per = HashMap::new();
                        let mut balances = HashMap::new();
                        for a in accounts {
                            balances.insert(a.account.external_id.clone(), a.account.current_balance);
                            per.insert(a.account.external_id, FeedBatch { added: a.transactions, ..Default::default() });
                        }
                        Ok(Fetched { batches: per, balances, cursor: String::new() })
                    }
                }
            }
            Provider::File => return Ok(report),
        };

        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        let s = &mut book.store;
        match fetched {
            Err((status, message)) => {
                s.set_connection_status(id, status, Some(&message))?;
                report.ok = false;
                report.error = Some(message);
            }
            Ok(Fetched { batches: mut per, balances, cursor }) => {
                for l in &links {
                    if let Some(batch) = per.remove(&l.external_account_id) {
                        let r = s.import_feed(l.ledger_account_id, &batch)?;
                        report.imported.added += r.added;
                        report.imported.updated += r.updated;
                        report.imported.removed += r.removed;
                        report.imported.skipped += r.skipped;
                        report.imported.auto_posted += r.auto_posted;
                    }
                    if let Some(b) = balances.get(&l.external_account_id) {
                        s.set_link_balance(l.id, *b)?;
                    }
                }
                s.record_sync(id, &cursor)?;
            }
        }
        Ok(report)
    }
}

/// Estimates what the account held the day before its feed starts:
/// today's bank balance minus everything the feed has delivered since.
fn suggest_opening(s: &mut Store, account_id: AccountId) -> ApiResult<OpeningSuggestion> {
    let link = s.link_for_account(account_id)?.ok_or_else(|| ApiError::new("rule", "this account isn't linked to a bank"))?;
    let bank_balance = link.bank_balance.ok_or_else(|| ApiError::new("rule", "sync once so we know the bank's balance"))?;
    let kind = s.account(account_id)?.kind();
    let inflows: Cents = s
        .inbox(Some(account_id))?
        .iter()
        .map(|i| &i.txn)
        .chain(s.reviewed(Some(account_id), u32::MAX)?.iter())
        .filter(|t| !t.pending && t.date >= link.start_date)
        .map(|t| t.amount)
        .sum();
    let raw_now = kind.raw(bank_balance);
    Ok(OpeningSuggestion {
        date: link.start_date.pred_opt().unwrap_or(link.start_date),
        amount: kind.natural(raw_now - inflows),
        bank_balance,
    })
}

#[cfg(test)]
mod tests {
    use super::client_id_hint;

    #[test]
    fn client_id_hint_takes_characters_not_bytes() {
        assert_eq!(client_id_hint("abcdef1234"), "…1234");
        assert_eq!(client_id_hint("ab"), "…ab");
        assert_eq!(client_id_hint("12éé3"), "…2éé3");
    }
}
