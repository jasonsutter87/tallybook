//! The chart of accounts and the journal.
//!
//! Methods: `accounts.*` (list, create, rename, archive, register, opening
//! balance, flex) and `entries.get`, `entries.list`, `entries.post`,
//! `entries.set_person`, `entries.void`.

use crate::{ApiResult, App, Archive, Id, IdDate, main_side, ok, params, today, unknown_method};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tally_core::{AccountId, AccountSubtype, Cents, EntryLine, Flex, NewEntry, Scope};
use tally_store::*;
// The glob above brings in the store's one-arg `Result`; keep std's.
use std::result::Result;

#[derive(Deserialize)]
struct Range {
    from: NaiveDate,
    to: NaiveDate,
}

/// `accounts.set_flex`: how far a household category can bend
/// ("fixed", "usage", "lifestyle"); null clears it. Household spending only.
#[derive(Deserialize)]
struct SetFlex {
    id: AccountId,
    flex: Option<Flex>,
}

#[derive(Deserialize)]
struct CreateAccount {
    code: Option<String>,
    name: String,
    subtype: AccountSubtype,
    parent_id: Option<AccountId>,
    /// Defaults to the book's main side.
    scope: Option<Scope>,
    flex: Option<Flex>,
    opening_balance: Option<Cents>,
    opening_date: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct Rename {
    id: AccountId,
    name: String,
}

#[derive(Deserialize)]
struct OpeningBalance {
    id: AccountId,
    date: NaiveDate,
    amount: Cents,
}

#[derive(Deserialize)]
struct PostEntry {
    date: NaiveDate,
    memo: String,
    lines: Vec<EntryLine>,
    /// Who the money was for (household books).
    #[serde(default)]
    person_id: Option<PersonId>,
    /// Who was paid, or who paid (business books; feeds 1099s).
    #[serde(default)]
    payee_id: Option<i64>,
}

/// `entries.set_person`: tag an entry as being for someone; null untags it.
#[derive(Deserialize)]
struct SetPerson {
    id: tally_core::journal::EntryId,
    person_id: Option<PersonId>,
}

#[derive(Serialize)]
struct AccountView {
    #[serde(flatten)]
    account: tally_core::Account,
    kind: tally_core::AccountKind,
    balance: Cents,
    link: Option<BankLink>,
    reconcilable: bool,
}

#[derive(Serialize)]
struct SubtypeInfo {
    value: AccountSubtype,
    kind: tally_core::AccountKind,
}

impl App {
    pub(crate) fn accounts(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "accounts.list" => self.with(|s| {
                let balances = s.balances(None, today())?;
                s.accounts()?
                    .into_iter()
                    .map(|a| {
                        Ok(AccountView {
                            kind: a.kind(),
                            balance: a.kind().natural(balances.get(&a.id).copied().unwrap_or_default()),
                            link: s.link_for_account(a.id)?,
                            reconcilable: a.subtype.is_reconcilable(),
                            account: a,
                        })
                    })
                    .collect::<Result<Vec<_>, StoreError>>()
            }),
            "accounts.subtypes" => ok(AccountSubtype::ALL.iter().map(|t| SubtypeInfo { value: *t, kind: t.kind() }).collect::<Vec<_>>()),
            "accounts.create" => {
                let p: CreateAccount = params(p)?;
                self.with(|s| {
                    let scope = match p.scope {
                        Some(scope) => scope,
                        None => main_side(s)?,
                    };
                    let id = s.create_account(&NewAccount { code: p.code, name: p.name, subtype: p.subtype, parent_id: p.parent_id, scope, flex: p.flex })?;
                    if let Some(amount) = p.opening_balance.filter(|a| !a.is_zero()) {
                        s.post_opening_balance(id, p.opening_date.unwrap_or_else(today), amount)?;
                    }
                    Ok(id)
                })
            }
            "accounts.rename" => {
                let p: Rename = params(p)?;
                self.with(|s| s.rename_account(p.id, &p.name))
            }
            // {id, flex} -> null.
            "accounts.set_flex" => {
                let p: SetFlex = params(p)?;
                self.with(|s| s.set_flex(p.id, p.flex))
            }
            "accounts.archive" => {
                let p: Archive = params(p)?;
                self.with(|s| s.set_archived(p.id, p.archived))
            }
            "accounts.register" => {
                let p: Id = params(p)?;
                self.with(|s| s.register(p.id))
            }
            "accounts.opening_balance" => {
                let p: OpeningBalance = params(p)?;
                self.with(|s| s.post_opening_balance(p.id, p.date, p.amount))
            }
            "entries.get" => {
                let p: Id = params(p)?;
                self.with(|s| s.entry(p.id))
            }
            "entries.list" => {
                let p: Range = params(p)?;
                self.with(|s| s.entries(p.from, p.to))
            }
            "entries.post" => {
                let p: PostEntry = params(p)?;
                self.with(|s| {
                    let id = s.post_entry(&NewEntry { date: p.date, memo: p.memo, lines: p.lines })?;
                    if p.person_id.is_some() {
                        s.set_entry_person(id, p.person_id)?;
                    }
                    if p.payee_id.is_some() {
                        s.set_entry_payee(id, p.payee_id)?;
                    }
                    Ok(id)
                })
            }
            // {id, person_id} -> null.
            "entries.set_person" => {
                let p: SetPerson = params(p)?;
                self.with(|s| s.set_entry_person(p.id, p.person_id))
            }
            "entries.void" => {
                let p: IdDate = params(p)?;
                self.with(|s| s.void_entry(p.id, p.date.unwrap_or_else(today)))
            }
            _ => Err(unknown_method(method)),
        }
    }
}
