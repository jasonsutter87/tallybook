//! Journal entries and the invariants every posted entry must satisfy.

use crate::account::AccountId;
use crate::money::Cents;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

pub type EntryId = i64;

/// One leg of an entry. `amount` > 0 is a debit, < 0 is a credit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryLine {
    pub account_id: AccountId,
    pub amount: Cents,
    pub memo: Option<String>,
}

impl EntryLine {
    pub fn debit(account_id: AccountId, amount: Cents) -> Self {
        EntryLine { account_id, amount: amount.abs(), memo: None }
    }

    pub fn credit(account_id: AccountId, amount: Cents) -> Self {
        EntryLine { account_id, amount: -amount.abs(), memo: None }
    }

    pub fn with_memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEntry {
    pub date: NaiveDate,
    pub memo: String,
    pub lines: Vec<EntryLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntryError {
    #[error("an entry needs at least two lines")]
    TooFewLines,
    #[error("line {0} has a zero amount")]
    ZeroLine(usize),
    #[error("entry does not balance: debits {debits}, credits {credits}")]
    Unbalanced { debits: Cents, credits: Cents },
    #[error("entry amounts overflow")]
    Overflow,
}

impl NewEntry {
    pub fn new(date: NaiveDate, memo: impl Into<String>) -> Self {
        NewEntry { date, memo: memo.into(), lines: Vec::new() }
    }

    pub fn line(mut self, line: EntryLine) -> Self {
        self.lines.push(line);
        self
    }

    pub fn debits(&self) -> Cents {
        self.lines.iter().filter(|l| l.amount.0 > 0).map(|l| l.amount).sum()
    }

    pub fn credits(&self) -> Cents {
        -self.lines.iter().filter(|l| l.amount.0 < 0).map(|l| l.amount).sum::<Cents>()
    }

    /// Structural checks that need no outside knowledge. The store adds the
    /// rest (accounts exist, aren't archived, date is after the lock date).
    pub fn validate(&self) -> Result<(), EntryError> {
        if self.lines.len() < 2 {
            return Err(EntryError::TooFewLines);
        }
        let mut debits = Cents::ZERO;
        let mut credits = Cents::ZERO;
        for (i, line) in self.lines.iter().enumerate() {
            if line.amount.is_zero() {
                return Err(EntryError::ZeroLine(i));
            }
            let side = if line.amount.0 > 0 { &mut debits } else { &mut credits };
            *side = side.checked_add(line.amount.abs()).ok_or(EntryError::Overflow)?;
        }
        if debits != credits {
            return Err(EntryError::Unbalanced { debits, credits });
        }
        Ok(())
    }

    /// The entry that exactly cancels this one, used to void without deleting history.
    pub fn reversal(&self, date: NaiveDate, memo: impl Into<String>) -> NewEntry {
        NewEntry {
            date,
            memo: memo.into(),
            lines: self
                .lines
                .iter()
                .map(|l| EntryLine { account_id: l.account_id, amount: -l.amount, memo: l.memo.clone() })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
    }

    #[test]
    fn balanced_entry_validates() {
        let e = NewEntry::new(d(), "rent")
            .line(EntryLine::debit(1, Cents(150000)))
            .line(EntryLine::credit(2, Cents(150000)));
        assert_eq!(e.validate(), Ok(()));
        assert_eq!(e.debits(), Cents(150000));
        assert_eq!(e.credits(), Cents(150000));
    }

    #[test]
    fn split_entry_validates() {
        let e = NewEntry::new(d(), "split")
            .line(EntryLine::debit(1, Cents(700)))
            .line(EntryLine::debit(2, Cents(300)))
            .line(EntryLine::credit(3, Cents(1000)));
        assert_eq!(e.validate(), Ok(()));
    }

    #[test]
    fn rejects_bad_entries() {
        let one = NewEntry::new(d(), "").line(EntryLine::debit(1, Cents(1)));
        assert_eq!(one.validate(), Err(EntryError::TooFewLines));

        let zero = NewEntry::new(d(), "")
            .line(EntryLine::debit(1, Cents(0)))
            .line(EntryLine::credit(2, Cents(0)));
        assert_eq!(zero.validate(), Err(EntryError::ZeroLine(0)));

        let off = NewEntry::new(d(), "")
            .line(EntryLine::debit(1, Cents(100)))
            .line(EntryLine::credit(2, Cents(99)));
        assert!(matches!(off.validate(), Err(EntryError::Unbalanced { .. })));

        let huge = NewEntry::new(d(), "")
            .line(EntryLine::debit(1, Cents(i64::MAX)))
            .line(EntryLine::debit(1, Cents(i64::MAX)))
            .line(EntryLine::credit(2, Cents(1)));
        assert_eq!(huge.validate(), Err(EntryError::Overflow));
    }

    #[test]
    fn reversal_cancels() {
        let e = NewEntry::new(d(), "x")
            .line(EntryLine::debit(1, Cents(100)))
            .line(EntryLine::credit(2, Cents(100)));
        let r = e.reversal(d(), "void");
        assert_eq!(r.validate(), Ok(()));
        for (a, b) in e.lines.iter().zip(&r.lines) {
            assert_eq!(a.amount + b.amount, Cents::ZERO);
        }
    }
}
