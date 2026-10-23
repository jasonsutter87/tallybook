//! The bank-feed inbox (Review): accepting, splitting, matching and excluding
//! imported transactions.
//!
//! Methods: `inbox.*`.

use crate::{ApiResult, App, Id, params, unknown_method};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tally_core::AccountId;
use tally_store::*;
// The glob above brings in the store's one-arg `Result`; keep std's.
use std::result::Result;

/// How many already-reviewed bank transactions Review lists, newest first:
/// enough to find and undo a recent decision without loading years of them.
const REVIEWED_SHOWN: u32 = 200;

#[derive(Deserialize)]
struct MaybeAccount {
    account_id: Option<AccountId>,
}

#[derive(Deserialize)]
struct Accept {
    id: BankTxnId,
    account_id: AccountId,
    memo: Option<String>,
    /// Also create a rule so future transactions like this auto-categorize.
    #[serde(default)]
    remember: bool,
    /// Who was paid (for 1099s) or who paid you.
    #[serde(default)]
    payee_id: Option<i64>,
    /// Who the money was for (household books).
    #[serde(default)]
    person_id: Option<PersonId>,
}

#[derive(Deserialize)]
struct AcceptSplit {
    id: BankTxnId,
    splits: Vec<Split>,
    memo: Option<String>,
}

#[derive(Deserialize)]
struct MatchLine {
    id: BankTxnId,
    line_id: LineId,
}

#[derive(Deserialize)]
struct Ids {
    ids: Vec<i64>,
}

#[derive(Serialize)]
struct ReviewedView {
    #[serde(flatten)]
    txn: BankTxn,
    categories: Vec<AccountId>,
}

#[derive(Serialize)]
struct BatchResult {
    accepted: usize,
    skipped: usize,
}

impl App {
    pub(crate) fn inbox(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "inbox.list" => {
                let p: MaybeAccount = params(p)?;
                self.with(|s| s.inbox(p.account_id))
            }
            "inbox.reviewed" => {
                let p: MaybeAccount = params(p)?;
                self.with(|s| {
                    s.reviewed(p.account_id, REVIEWED_SHOWN)?
                        .into_iter()
                        .map(|txn| {
                            // Where a posted transaction was filed: every line but the bank's own.
                            let categories = match (txn.status, txn.entry_id) {
                                (BankTxnStatus::Posted, Some(entry)) => {
                                    s.entry(entry)?.lines.iter().filter(|l| Some(l.id) != txn.line_id).map(|l| l.account_id).collect()
                                }
                                _ => vec![],
                            };
                            Ok(ReviewedView { txn, categories })
                        })
                        .collect::<Result<Vec<_>, StoreError>>()
                })
            }
            "inbox.count" => self.with(|s| s.inbox_count()),
            "inbox.candidates" => {
                let p: Id = params(p)?;
                self.with(|s| {
                    let t = s.bank_txn(p.id)?;
                    s.match_candidates(&t)
                })
            }
            "inbox.accept" => {
                let p: Accept = params(p)?;
                self.with(|s| {
                    let t = s.bank_txn(p.id)?;
                    let entry = s.accept(p.id, p.account_id, p.memo)?;
                    if p.payee_id.is_some() {
                        s.set_entry_payee(entry, p.payee_id)?;
                    }
                    if p.person_id.is_some() {
                        s.set_entry_person(entry, p.person_id)?;
                    }
                    if p.remember {
                        remember_rule(s, &t, p.account_id, p.person_id)?;
                    }
                    Ok(entry)
                })
            }
            "inbox.split" => {
                let p: AcceptSplit = params(p)?;
                self.with(|s| s.accept_split(p.id, &p.splits, p.memo))
            }
            "inbox.match" => {
                let p: MatchLine = params(p)?;
                self.with(|s| s.match_txn(p.id, p.line_id))
            }
            "inbox.exclude" => {
                let p: Ids = params(p)?;
                self.with(|s| p.ids.iter().try_for_each(|id| s.exclude(*id)))
            }
            "inbox.undo" => {
                let p: Id = params(p)?;
                self.with(|s| s.undo_review(p.id))
            }
            "inbox.accept_suggested" => {
                let p: Ids = params(p)?;
                self.with(|s| accept_suggested(s, &p.ids))
            }
            _ => Err(unknown_method(method)),
        }
    }
}

/// After the user categorizes a transaction with "remember", create a
/// contains-rule keyed on the payee so next month's lands pre-categorized.
fn remember_rule(s: &mut Store, t: &BankTxn, account_id: AccountId, person_id: Option<PersonId>) -> Result<(), StoreError> {
    let pattern = t.merchant.clone().unwrap_or_else(|| tally_core::bank::normalize_description(&t.description));
    if pattern.trim().len() < 3 {
        return Ok(());
    }
    let exists = s.rules()?.iter().any(|r| r.pattern.eq_ignore_ascii_case(pattern.trim()));
    if exists {
        return Ok(());
    }
    s.create_rule(&NewRule {
        name: pattern.trim().to_string(),
        priority: tally_core::rules::DEFAULT_PRIORITY,
        op: tally_core::rules::MatchOp::Contains,
        pattern: pattern.trim().to_string(),
        direction: if t.amount.0 > 0 { tally_core::rules::Direction::Inflow } else { tally_core::rules::Direction::Outflow },
        amount_min: None,
        amount_max: None,
        bank_account_id: None,
        target_account_id: account_id,
        rename_to: None,
        auto_post: false,
        person_id,
    })?;
    Ok(())
}

/// "Accept all": applies each transaction's own suggestion, skipping ones without.
fn accept_suggested(s: &mut Store, ids: &[BankTxnId]) -> Result<BatchResult, StoreError> {
    let inbox = s.inbox(None)?;
    let mut r = BatchResult { accepted: 0, skipped: 0 };
    for id in ids {
        let Some(item) = inbox.iter().find(|i| i.txn.id == *id) else {
            r.skipped += 1;
            continue;
        };
        let done = match &item.suggestion {
            Some(Suggestion::Match { line_id, .. }) => s.match_txn(*id, *line_id).is_ok(),
            Some(Suggestion::Rule { account_id, rename_to, .. }) => accept_for(s, *id, *account_id, rename_to.clone(), item.person_id)?,
            Some(Suggestion::History { account_id }) => accept_for(s, *id, *account_id, None, item.person_id)?,
            None => false,
        };
        if done { r.accepted += 1 } else { r.skipped += 1 }
    }
    Ok(r)
}

/// Accepts into a category and tags who it was for; `false` if it didn't post.
fn accept_for(s: &mut Store, id: BankTxnId, account_id: AccountId, memo: Option<String>, person: Option<PersonId>) -> Result<bool, StoreError> {
    let Ok(entry) = s.accept(id, account_id, memo) else { return Ok(false) };
    if person.is_some() {
        s.set_entry_person(entry, person)?;
    }
    Ok(true)
}
