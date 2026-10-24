//! Bank connections: linking (Plaid Hosted Link, SimpleFIN), syncing, unlinking.
//!
//! Network calls never hold the book lock: read what's needed, drop the
//! lock, await the provider, then lock again to write.

use crate::{ApiError, ApiResult, App, ok, params, today, unknown_method};
use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use tally_core::bank::{FeedAccount, FeedBatch};
use tally_core::{AccountId, AccountSubtype, Cents};
use crate::license::Tier;
use tally_feed::{plaid, simplefin};
use tally_store::*;
use std::result::Result;

const PLAID_CREDS: &str = "plaid.credentials";

/// Tallybook Connect, which holds the Plaid keys for plans that include bank
/// sync (see docs/CONNECT.md). Empty until the service is deployed;
/// `TALLYBOOK_CONNECT_URL` overrides it, for development.
const CONNECT_URL: &str = "";

/// Where Tallybook Connect is, if anywhere: https, or plain http only to this computer.
pub(crate) fn connect_url() -> Option<String> {
    let url = std::env::var("TALLYBOOK_CONNECT_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| CONNECT_URL.to_string());
    let local = ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"].iter().any(|p| url.starts_with(p));
    (url.starts_with("https://") || local).then_some(url)
}

/// Connect's stand-ins for Plaid access tokens start with this; a token
/// without it came from Plaid keys the user brought.
const CONNECT_HANDLE: &str = "tbc-";

/// History fetched when a SimpleFIN connection is first made: only enough to
/// list the accounts and their balances. The real history comes on the first
/// sync, from each linked account's start date.
const SIMPLEFIN_CONNECT_DAYS: u64 = 30;

/// How far back a SimpleFIN sync reaches at most. SimpleFIN Bridge serves
/// about 90 days per request; one day less stays inside that whatever the
/// time zones.
const SIMPLEFIN_MAX_HISTORY_DAYS: u64 = 89;

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

/// The keychain entry holding a connection's credentials, as Tallybook names
/// them: `plaid:<uuid>` or `simplefin:<uuid>`, matching the connection's
/// provider. The name comes from the book file, which may come from someone
/// else, so any other name is treated as no credentials at all: Tallybook
/// never reads or deletes a keychain entry it didn't create for this purpose.
pub(crate) fn secret_key(conn: &BankConnection) -> Option<&str> {
    let key = conn.secret_ref.as_deref()?;
    let prefix = match conn.provider {
        Provider::Plaid => "plaid:",
        Provider::Simplefin => "simplefin:",
        Provider::File => return None,
    };
    let id = key.strip_prefix(prefix)?;
    let canonical = uuid::Uuid::parse_str(id).ok()?.hyphenated().to_string();
    (canonical == id).then_some(key)
}

impl App {
    /// The Plaid client for a new connection: Tallybook Connect on a plan that
    /// includes bank sync, otherwise Plaid keys the user brought (development
    /// and self-hosting).
    fn new_link_client(&self) -> ApiResult<plaid::Client> {
        if let Some(l) = self.load_license().filter(|l| l.tier >= Tier::Personal) {
            if l.active == Some(false) {
                return Err(ApiError::new("license", "Your Tallybook plan has ended, so new bank connections are paused. Renew it in Settings → License."));
            }
            return self.connect_client(l.key);
        }
        // An unreadable keychain (say, Linux without a secret service) means
        // no keys of one's own, not an error: the plan is still the answer.
        if matches!(self.secrets.get(PLAID_CREDS), Ok(Some(_))) {
            return self.own_keys_client();
        }
        Err(ApiError::new(
            "license",
            "Connecting a bank takes a Tallybook plan, from $20 a year. Statement files import for free: Banking → Import a file.",
        ))
    }

    /// The Plaid client that can use `token`: through Connect for its
    /// stand-ins, with the user's own keys otherwise.
    fn client_for(&self, token: &str) -> ApiResult<plaid::Client> {
        if !token.starts_with(CONNECT_HANDLE) {
            return self.own_keys_client();
        }
        // Connect checks the plan is paid for; the app just sends the key.
        let l = self.load_license().ok_or_else(|| ApiError::new("license", "Add your license key in Settings → License to keep this bank syncing."))?;
        self.connect_client(l.key)
    }

    fn connect_client(&self, license_key: String) -> ApiResult<plaid::Client> {
        let url = self.connect_url.as_deref().ok_or_else(|| {
            ApiError::new("connect_unavailable", "Bank sync through Tallybook isn't switched on in this version yet. Statement files import for free: Banking → Import a file.")
        })?;
        Ok(plaid::Client::via_connect(url, license_key))
    }

    fn own_keys_client(&self) -> ApiResult<plaid::Client> {
        let raw = self
            .secrets
            .get(PLAID_CREDS)
            .map_err(|e| ApiError::new("keychain", e))?
            .ok_or_else(|| ApiError::new("plaid_not_configured", "add your Plaid keys first: Bank connections → Connect a bank → Plaid"))?;
        let creds: plaid::Credentials = serde_json::from_str(&raw).map_err(|e| ApiError::new("keychain", e.to_string()))?;
        Ok(plaid::Client::new(creds))
    }

    fn secret(&self, conn: &BankConnection) -> ApiResult<String> {
        let key = secret_key(conn).ok_or_else(|| ApiError::new("rule", "this connection has no saved credentials"))?;
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

            "bank.plaid_config_get" => self.plaid_config_get(),
            "bank.plaid_config_set" => self.plaid_config_set(params(p)?),
            "bank.plaid_start" => {
                let user = self.plaid_user_id()?;
                let link = self.new_link_client()?.create_hosted_link(&user, None).await.map_err(plaid_err)?;
                ok(link)
            }
            "bank.plaid_relink_start" => {
                let p: ConnectionParam = params(p)?;
                let user = self.plaid_user_id()?;
                let conn = self.run(|s| s.connection(p.connection_id))?;
                let token = self.secret(&conn)?;
                let link = self.client_for(&token)?.create_hosted_link(&user, Some(&token)).await.map_err(plaid_err)?;
                ok(link)
            }
            "bank.plaid_poll" => self.plaid_poll(params(p)?).await,
            "bank.simplefin_connect" => self.simplefin_connect(params(p)?).await,
            "bank.simplefin_reconnect" => self.simplefin_reconnect(params(p)?).await,
            "bank.feed_accounts" => self.feed_accounts(params(p)?).await,
            "bank.link" => self.link_account(params(p)?),
            "bank.unlink" => {
                let p: LinkParam = params(p)?;
                self.with(|s| s.unlink_account(p.link_id))
            }
            "bank.disconnect" => self.disconnect(params(p)?).await,
            "bank.sync" => self.sync(params(p)?).await,
            "bank.suggest_opening" => {
                let p: AccountParam = params(p)?;
                self.with_api(|s| suggest_opening(s, p.account_id))
            }
            _ => Err(unknown_method(method)),
        }
    }

    // ------------------------------------------------------------ setup

    fn plaid_config_get(&self) -> ApiResult<Value> {
        let raw = self.secrets.get(PLAID_CREDS).map_err(|e| ApiError::new("keychain", e))?;
        let creds = raw.and_then(|r| serde_json::from_str::<plaid::Credentials>(&r).ok());
        ok(PlaidConfigView {
            configured: creds.is_some(),
            environment: creds.as_ref().map(|c| c.environment),
            client_id_hint: creds.map(|c| client_id_hint(&c.client_id)),
        })
    }

    fn plaid_config_set(&self, creds: plaid::Credentials) -> ApiResult<Value> {
        if creds.client_id.trim().is_empty() || creds.secret.trim().is_empty() {
            return Err(ApiError::new("rule", "both the client ID and secret are required"));
        }
        let json = serde_json::to_string(&creds).map_err(|e| ApiError::new("internal", e.to_string()))?;
        self.secrets.set(PLAID_CREDS, &json).map_err(|e| ApiError::new("keychain", e))?;
        ok(())
    }

    // ------------------------------------------------------------ connecting

    async fn plaid_poll(&self, p: Poll) -> ApiResult<Value> {
        // A repair polls through whatever made the connection; a new one through what will make it.
        let client = match p.connection_id {
            Some(id) => {
                let conn = self.run(|s| s.connection(id))?;
                self.client_for(&self.secret(&conn)?)?
            }
            None => self.new_link_client()?,
        };
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
                let id = self.run(|s| s.create_connection(Provider::Plaid, &institution, Some(&item.item_id), Some(&secret_ref)))?;
                ok(PollView::Linked { connection_id: id, institution, accounts })
            }
        }
    }

    async fn simplefin_connect(&self, p: SetupToken) -> ApiResult<Value> {
        let access_url = simplefin::claim(&p.setup_token).await.map_err(sfin_err)?;
        let since = today() - Days::new(SIMPLEFIN_CONNECT_DAYS);
        let accounts = simplefin::fetch(&access_url, since).await.map_err(sfin_err)?;
        let secret_ref = format!("simplefin:{}", uuid::Uuid::new_v4());
        self.secrets.set(&secret_ref, &access_url).map_err(|e| ApiError::new("keychain", e))?;
        let institution = accounts.iter().find_map(|a| a.institution.clone()).unwrap_or_else(|| "SimpleFIN".into());
        let id = self.run(|s| s.create_connection(Provider::Simplefin, &institution, None, Some(&secret_ref)))?;
        ok(PollView::Linked { connection_id: id, institution, accounts: accounts.into_iter().map(|a| a.account).collect() })
    }

    async fn simplefin_reconnect(&self, p: SimplefinReconnect) -> ApiResult<Value> {
        let conn = self.run(|s| s.connection(p.connection_id))?;
        if conn.provider != Provider::Simplefin {
            return Err(ApiError::new("rule", "that isn't a SimpleFIN connection"));
        }
        let key = secret_key(&conn).ok_or_else(|| ApiError::new("rule", "this connection was disconnected; connect it again instead"))?.to_string();
        let access_url = simplefin::claim(&p.setup_token).await.map_err(sfin_err)?;
        // Same connection, same links: only the credential changes.
        self.secrets.set(&key, &access_url).map_err(|e| ApiError::new("keychain", e))?;
        self.with(|s| s.set_connection_status(p.connection_id, ConnectionStatus::Ok, None))
    }

    // ------------------------------------------------------------ links

    /// The provider's own list of accounts, for the "link accounts" step.
    async fn feed_accounts(&self, p: ConnectionParam) -> ApiResult<Value> {
        let conn = self.run(|s| s.connection(p.connection_id))?;
        let secret = self.secret(&conn)?;
        let accounts = match conn.provider {
            Provider::Plaid => self.client_for(&secret)?.accounts(&secret).await.map_err(plaid_err)?,
            Provider::Simplefin => simplefin::fetch(&secret, today()).await.map_err(sfin_err)?.into_iter().map(|a| a.account).collect(),
            Provider::File => vec![],
        };
        ok(accounts)
    }

    fn link_account(&self, p: Link) -> ApiResult<Value> {
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

    async fn disconnect(&self, p: ConnectionParam) -> ApiResult<Value> {
        let conn = self.run(|s| s.connection(p.connection_id))?;
        if conn.provider == Provider::Plaid
            && let Ok(token) = self.secret(&conn)
            && let Ok(client) = self.client_for(&token)
        {
            // Best effort: revoke at Plaid; disconnect locally regardless.
            let _ = client.remove_item(&token).await;
        }
        if let Some(key) = secret_key(&conn) {
            let _ = self.secrets.delete(key);
        }
        self.with(|s| s.disconnect(p.connection_id))
    }

    // ------------------------------------------------------------ syncing

    /// `bank.sync`: one connection, or every connection that has a provider.
    ///
    /// Syncing everything never stops at a failure: a connection that can't be
    /// synced (a credential missing from the keychain, say) gets a report with
    /// `ok: false` and the reason, and the rest carry on. Naming one connection
    /// returns its error as an error.
    async fn sync(&self, p: MaybeConnection) -> ApiResult<Value> {
        if let Some(id) = p.connection_id {
            return ok(vec![self.sync_one(id).await?]);
        }
        let all: Vec<(ConnectionId, String)> =
            self.run(|s| Ok(s.connections()?.into_iter().filter(|c| c.provider != Provider::File).map(|c| (c.id, c.institution)).collect()))?;
        let mut reports = Vec::new();
        for (id, institution) in all {
            match self.sync_one(id).await {
                Ok(report) => reports.push(report),
                // Nothing else can work without a book; don't report that once per connection.
                Err(e) if e.code == "no_book" => return Err(e),
                Err(e) => reports.push(SyncReport { connection_id: id, institution, ok: false, error: Some(e.message), imported: ImportSummary::default() }),
            }
        }
        ok(reports)
    }

    /// Fetches, then writes: the network call happens between two short locks.
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
        let fetched = match conn.provider {
            Provider::Plaid => self.fetch_plaid(&conn, &secret, &links).await?,
            Provider::Simplefin => fetch_simplefin(&secret, &links).await,
            Provider::File => return Ok(report),
        };

        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        write_fetched(&mut book.store, id, &links, fetched, &mut report)?;
        Ok(report)
    }

    /// Plaid's changes since the saved cursor, grouped by the provider's account id.
    /// The outer error is "can't even start"; the inner one is the provider refusing.
    async fn fetch_plaid(&self, conn: &BankConnection, secret: &str, links: &[BankLink]) -> ApiResult<Result<Fetched, (ConnectionStatus, String)>> {
        let client = self.client_for(secret)?;
        let result = match client.sync(secret, &conn.cursor).await {
            Err(e) => return Ok(Err((if e.needs_relink() { ConnectionStatus::NeedsRelink } else { ConnectionStatus::Error }, e.to_string()))),
            Ok(result) => result,
        };
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
        for l in links {
            per.entry(l.external_account_id.clone()).or_default().removed.extend(result.batch.removed.iter().cloned());
        }
        let balances = client
            .accounts(secret)
            .await
            .map(|accts| accts.into_iter().map(|a| (a.external_id, a.current_balance)).collect())
            .unwrap_or_default();
        Ok(Ok(Fetched { batches: per, balances, cursor: result.next_cursor }))
    }
}

/// SimpleFIN's transactions and balances since the earliest linked account's
/// start date (never further back than the Bridge serves).
async fn fetch_simplefin(secret: &str, links: &[BankLink]) -> Result<Fetched, (ConnectionStatus, String)> {
    let since = links.iter().map(|l| l.start_date).min().unwrap_or_else(today).max(today() - Days::new(SIMPLEFIN_MAX_HISTORY_DAYS));
    match simplefin::fetch(secret, since).await {
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

/// Records the outcome of a fetch: the failure on the connection, or each
/// link's transactions and balance plus the new cursor.
fn write_fetched(
    s: &mut Store,
    id: ConnectionId,
    links: &[BankLink],
    fetched: Result<Fetched, (ConnectionStatus, String)>,
    report: &mut SyncReport,
) -> ApiResult<()> {
    match fetched {
        Err((status, message)) => {
            s.set_connection_status(id, status, Some(&message))?;
            report.ok = false;
            report.error = Some(message);
        }
        Ok(Fetched { batches: mut per, balances, cursor }) => {
            for l in links {
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
    Ok(())
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
