//! CSV statement import with column mapping and auto-detection.
//!
//! Banks disagree on everything: column names, date formats, whether money
//! out is negative or lives in its own "Debit" column. [`detect`] guesses a
//! [`CsvMapping`] from the header row; the UI lets the user correct it.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tally_core::bank::{FeedTransaction, fnv1a64};
use tally_core::money::Cents;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    /// 2026-01-31
    Iso,
    /// 01/31/2026 or 1/31/26
    MonthDayYear,
    /// 31/01/2026 or 31/1/26
    DayMonthYear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AmountColumns {
    /// One signed column. `negate` flips it for exports where purchases are positive.
    Single { column: usize, negate: bool },
    /// Separate money-out / money-in columns, both written as positive numbers.
    Split { debit: usize, credit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsvMapping {
    pub has_header: bool,
    pub date_column: usize,
    pub date_format: DateFormat,
    pub description_column: usize,
    pub amount: AmountColumns,
}

#[derive(Debug, thiserror::Error)]
pub enum CsvError {
    #[error("couldn't read CSV: {0}")]
    Csv(#[from] csv::Error),
    #[error("row {row}: {message}")]
    Row { row: usize, message: String },
    #[error("the file has no rows")]
    Empty,
}

/// Header row plus the first few data rows, for the mapping UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CsvPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub mapping: Option<CsvMapping>,
}

fn reader(data: &[u8]) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new().has_headers(false).flexible(true).trim(csv::Trim::All).from_reader(data)
}

/// Strip a UTF-8 BOM, which Excel loves to add.
fn strip_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data)
}

pub fn preview(data: &[u8], rows: usize) -> Result<CsvPreview, CsvError> {
    let mut records = reader(strip_bom(data)).into_records();
    let headers: Vec<String> = match records.next() {
        Some(r) => r?.iter().map(str::to_string).collect(),
        None => return Err(CsvError::Empty),
    };
    let sample: Vec<Vec<String>> = records
        .take(rows)
        .map(|r| r.map(|r| r.iter().map(str::to_string).collect()))
        .collect::<Result<_, _>>()?;
    let mapping = detect(&headers, &sample);
    Ok(CsvPreview { headers, rows: sample, mapping })
}

/// Guesses columns from header names and the date format from sample values.
pub fn detect(headers: &[String], sample: &[Vec<String>]) -> Option<CsvMapping> {
    let lower: Vec<String> = headers.iter().map(|h| h.to_lowercase()).collect();
    let find = |names: &[&str]| lower.iter().position(|h| names.iter().any(|n| h.contains(n)));

    let date_column = find(&["posted date", "posting date", "transaction date", "date"])?;
    let description_column = find(&["description", "payee", "name", "memo", "details"])?;
    let amount = match (find(&["debit", "withdrawal", "money out"]), find(&["credit", "deposit", "money in"])) {
        (Some(debit), Some(credit)) if debit != credit => AmountColumns::Split { debit, credit },
        _ => AmountColumns::Single { column: find(&["amount"])?, negate: false },
    };
    let dates: Vec<&str> = sample.iter().filter_map(|r| r.get(date_column).map(String::as_str)).collect();
    Some(CsvMapping { has_header: true, date_column, date_format: detect_date_format(&dates), description_column, amount })
}

/// Picks the first format that parses every sample. US order wins ties.
pub fn detect_date_format(samples: &[&str]) -> DateFormat {
    [DateFormat::Iso, DateFormat::MonthDayYear, DateFormat::DayMonthYear]
        .into_iter()
        .find(|f| !samples.is_empty() && samples.iter().all(|s| parse_date(s, *f).is_some()))
        .unwrap_or(DateFormat::MonthDayYear)
}

pub fn parse_date(s: &str, format: DateFormat) -> Option<NaiveDate> {
    let s = s.trim();
    // Some banks append a time: "01/31/2026 00:00:00".
    let s = s.split_whitespace().next()?;
    let fmts: &[&str] = match format {
        DateFormat::Iso => &["%Y-%m-%d", "%Y/%m/%d"],
        DateFormat::MonthDayYear => &["%m/%d/%Y", "%m/%d/%y", "%m-%d-%Y"],
        DateFormat::DayMonthYear => &["%d/%m/%Y", "%d/%m/%y", "%d-%m-%Y", "%d.%m.%Y"],
    };
    fmts.iter().find_map(|f| {
        NaiveDate::parse_from_str(s, f)
            .ok()
            // %y happily parses 4-digit years as year 20; reject nonsense.
            .filter(|d| (1970..=2200).contains(&chrono::Datelike::year(d)))
    })
}

fn cell<'a>(record: &'a csv::StringRecord, col: usize, row: usize, what: &str) -> Result<&'a str, CsvError> {
    record.get(col).ok_or_else(|| CsvError::Row { row, message: format!("missing {what} column") })
}

fn money(s: &str, row: usize) -> Result<Cents, CsvError> {
    if s.trim().is_empty() {
        return Ok(Cents::ZERO);
    }
    Cents::parse(s).map_err(|e| CsvError::Row { row, message: e.to_string() })
}

/// Parses every data row. Ids are synthesized from content plus an
/// occurrence counter, so re-importing an overlapping file is a no-op while
/// two genuinely identical same-day charges are both kept.
pub fn parse(data: &[u8], mapping: &CsvMapping) -> Result<Vec<FeedTransaction>, CsvError> {
    let mut seen: HashMap<u64, u32> = HashMap::new();
    let mut out = Vec::new();
    for (i, record) in reader(strip_bom(data)).into_records().enumerate() {
        let record = record?;
        let row = i + 1;
        if (mapping.has_header && i == 0) || record.iter().all(|c| c.is_empty()) {
            continue;
        }
        let raw_date = cell(&record, mapping.date_column, row, "date")?;
        let date = parse_date(raw_date, mapping.date_format)
            .ok_or_else(|| CsvError::Row { row, message: format!("unrecognized date {raw_date:?}") })?;
        let description = cell(&record, mapping.description_column, row, "description")?.to_string();
        let amount = match mapping.amount {
            AmountColumns::Single { column, negate } => {
                let a = money(cell(&record, column, row, "amount")?, row)?;
                if negate { -a } else { a }
            }
            AmountColumns::Split { debit, credit } => {
                let out = money(cell(&record, debit, row, "debit").unwrap_or(""), row)?.abs();
                let inn = money(cell(&record, credit, row, "credit").unwrap_or(""), row)?.abs();
                inn - out
            }
        };
        if amount.is_zero() {
            continue;
        }
        let key = fnv1a64(format!("{date}|{}|{}", amount.0, description.to_lowercase()).as_bytes());
        let n = seen.entry(key).or_insert(0);
        *n += 1;
        out.push(FeedTransaction {
            external_id: format!("csv:{key:016x}:{n}"),
            date,
            amount,
            description,
            merchant: None,
            pending: false,
            category_hint: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHASE: &str = "\u{feff}Transaction Date,Post Date,Description,Category,Type,Amount,Memo
01/28/2026,01/29/2026,FIGMA MONTHLY,Software,Sale,-45.00,
01/29/2026,01/29/2026,STARBUCKS 0423,Food & Drink,Sale,-6.75,
01/29/2026,01/29/2026,STARBUCKS 0423,Food & Drink,Sale,-6.75,
01/30/2026,01/30/2026,Payment Thank You,,Payment,500.00,
";

    const SPLIT: &str = "Date,Description,Withdrawals,Deposits,Balance
2026-02-01,Rent,1200.00,,3800.00
2026-02-03,Client ACH,,2,500.00,6300.00
";

    #[test]
    fn detects_and_parses_a_signed_amount_export() {
        let p = preview(CHASE.as_bytes(), 5).unwrap();
        let m = p.mapping.unwrap();
        assert_eq!(m.date_column, 0);
        assert_eq!(m.description_column, 2);
        assert_eq!(m.amount, AmountColumns::Single { column: 5, negate: false });
        assert_eq!(m.date_format, DateFormat::MonthDayYear);

        let txns = parse(CHASE.as_bytes(), &m).unwrap();
        assert_eq!(txns.len(), 4);
        assert_eq!(txns[0].amount, Cents(-4500));
        assert_eq!(txns[3].amount, Cents(50000));
        // The two identical coffees get distinct, stable ids.
        assert_ne!(txns[1].external_id, txns[2].external_id);
        assert_eq!(parse(CHASE.as_bytes(), &m).unwrap()[2].external_id, txns[2].external_id);
    }

    #[test]
    fn detects_split_debit_credit_columns() {
        // Note the unquoted thousands separator in row 2 shifts columns; flexible
        // parsing must not silently mis-read it, so quote it like real banks do.
        let data = SPLIT.replace("2,500.00", "\"2,500.00\"");
        let p = preview(data.as_bytes(), 5).unwrap();
        let m = p.mapping.unwrap();
        assert_eq!(m.amount, AmountColumns::Split { debit: 2, credit: 3 });
        assert_eq!(m.date_format, DateFormat::Iso);
        let txns = parse(data.as_bytes(), &m).unwrap();
        assert_eq!(txns.iter().map(|t| t.amount).collect::<Vec<_>>(), vec![Cents(-120000), Cents(250000)]);
    }

    #[test]
    fn day_first_dates_are_detected_when_unambiguous() {
        assert_eq!(detect_date_format(&["31/01/2026", "15/02/2026"]), DateFormat::DayMonthYear);
        assert_eq!(detect_date_format(&["01/02/2026"]), DateFormat::MonthDayYear);
        assert_eq!(parse_date("1/31/26", DateFormat::MonthDayYear), NaiveDate::from_ymd_opt(2026, 1, 31));
        assert_eq!(parse_date("01/31/2026 00:00:00", DateFormat::MonthDayYear), NaiveDate::from_ymd_opt(2026, 1, 31));
    }

    #[test]
    fn bad_rows_report_their_line() {
        let m = CsvMapping {
            has_header: false,
            date_column: 0,
            date_format: DateFormat::Iso,
            description_column: 1,
            amount: AmountColumns::Single { column: 2, negate: false },
        };
        let err = parse(b"2026-01-01,ok,1.00\nnot-a-date,bad,2.00\n", &m).unwrap_err();
        assert!(err.to_string().starts_with("row 2"), "{err}");
    }
}
