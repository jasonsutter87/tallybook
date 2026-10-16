//! OFX / QFX statement import (Quicken & Money downloads).
//!
//! Handles both OFX 1.x SGML (unclosed leaf tags) and OFX 2.x XML with one
//! lenient scanner: a leaf's value runs from its tag to the next `<`.

use chrono::NaiveDate;
use tally_core::bank::{FeedAccount, FeedAccountKind, FeedTransaction};
use tally_core::money::Cents;

#[derive(Debug, thiserror::Error)]
pub enum OfxError {
    #[error("not an OFX file (no <OFX> element)")]
    NotOfx,
    #[error("transaction {index}: {message}")]
    Transaction { index: usize, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfxStatement {
    pub account: FeedAccount,
    pub transactions: Vec<FeedTransaction>,
}

/// Text between `<TAG>` and `</TAG>` (case-insensitive), for each occurrence.
fn blocks<'a>(src: &'a str, upper: &str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = upper[from..].find(&open) {
        let body_start = from + start + open.len();
        let end = upper[body_start..].find(&close).map(|e| body_start + e).unwrap_or(src.len());
        out.push(&src[body_start..end]);
        from = end;
        if from >= src.len() {
            break;
        }
    }
    out
}

/// Value of the first leaf `<TAG>value` in `src`.
fn leaf(src: &str, tag: &str) -> Option<String> {
    let upper = src.to_ascii_uppercase();
    let open = format!("<{tag}>");
    let start = upper.find(&open)? + open.len();
    let rest = &src[start..];
    let value = rest[..rest.find('<').unwrap_or(rest.len())].trim();
    (!value.is_empty()).then(|| decode_entities(value))
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

/// `20260131`, `20260131120000`, `20260131120000.000[-5:EST]` → date.
fn ofx_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..8)?, "%Y%m%d").ok()
}

pub fn parse(data: &[u8]) -> Result<Vec<OfxStatement>, OfxError> {
    let src = String::from_utf8_lossy(data);
    let upper = src.to_ascii_uppercase();
    if !upper.contains("<OFX>") {
        return Err(OfxError::NotOfx);
    }
    let mut statements = Vec::new();
    for (tag, kind) in [("STMTRS", FeedAccountKind::Depository), ("CCSTMTRS", FeedAccountKind::Credit)] {
        for stmt in blocks(&src, &upper, tag) {
            statements.push(parse_statement(stmt, kind)?);
        }
    }
    Ok(statements)
}

fn parse_statement(stmt: &str, kind: FeedAccountKind) -> Result<OfxStatement, OfxError> {
    let acct_id = leaf(stmt, "ACCTID").unwrap_or_else(|| "unknown".into());
    let mask = (acct_id.len() >= 4).then(|| acct_id[acct_id.len() - 4..].to_string());
    let balance = blocks(stmt, &stmt.to_ascii_uppercase(), "LEDGERBAL")
        .first()
        .and_then(|b| leaf(b, "BALAMT"))
        .and_then(|b| Cents::parse(&b).ok())
        // OFX reports card balances as negative (you owe); FeedAccount uses natural sign.
        .map(|b| if kind == FeedAccountKind::Credit { -b } else { b });
    let account = FeedAccount {
        external_id: acct_id.clone(),
        name: match kind {
            FeedAccountKind::Credit => format!("Card ···{}", mask.as_deref().unwrap_or("")),
            _ => format!("Account ···{}", mask.as_deref().unwrap_or("")),
        },
        mask,
        kind,
        current_balance: balance,
        currency: leaf(stmt, "CURDEF"),
    };

    let upper = stmt.to_ascii_uppercase();
    let mut transactions = Vec::new();
    for (index, t) in blocks(stmt, &upper, "STMTTRN").into_iter().enumerate() {
        let err = |message: &str| OfxError::Transaction { index, message: message.into() };
        let date = leaf(t, "DTPOSTED").and_then(|d| ofx_date(&d)).ok_or_else(|| err("missing or bad DTPOSTED"))?;
        let amount = leaf(t, "TRNAMT")
            .and_then(|a| Cents::parse(&a).ok())
            .ok_or_else(|| err("missing or bad TRNAMT"))?;
        let name = leaf(t, "NAME");
        let memo = leaf(t, "MEMO");
        let description = match (&name, &memo) {
            (Some(n), Some(m)) if !n.eq_ignore_ascii_case(m) && !n.contains(m.as_str()) => format!("{n} {m}"),
            (Some(n), _) => n.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => leaf(t, "TRNTYPE").unwrap_or_else(|| "Transaction".into()),
        };
        let fitid = leaf(t, "FITID").ok_or_else(|| err("missing FITID"))?;
        transactions.push(FeedTransaction {
            external_id: format!("ofx:{fitid}"),
            date,
            amount,
            description,
            merchant: name,
            pending: false,
            category_hint: None,
        });
    }
    Ok(OfxStatement { account, transactions })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SGML: &str = "OFXHEADER:100
DATA:OFXSGML
VERSION:102

<OFX>
<BANKMSGSRSV1><STMTTRNRS><STMTRS>
<CURDEF>USD
<BANKACCTFROM><BANKID>121000248<ACCTID>000123456789<ACCTTYPE>CHECKING</BANKACCTFROM>
<BANKTRANLIST>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260115120000[-8:PST]<TRNAMT>-42.17<FITID>2026011501<NAME>PG&amp;E<MEMO>AUTOPAY</STMTTRN>
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20260116<TRNAMT>1500.00<FITID>2026011602<NAME>ACME PAYROLL</STMTTRN>
</BANKTRANLIST>
<LEDGERBAL><BALAMT>3210.55<DTASOF>20260131</LEDGERBAL>
</STMTRS></STMTTRNRS></BANKMSGSRSV1>
</OFX>";

    const XML_CARD: &str = r#"<?xml version="1.0"?><OFX><CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS>
<CURDEF>USD</CURDEF><CCACCTFROM><ACCTID>4111111111111111</ACCTID></CCACCTFROM>
<BANKTRANLIST><STMTTRN><TRNTYPE>DEBIT</TRNTYPE><DTPOSTED>20260203</DTPOSTED><TRNAMT>-19.99</TRNAMT><FITID>abc</FITID><NAME>NETFLIX.COM</NAME></STMTTRN></BANKTRANLIST>
<LEDGERBAL><BALAMT>-250.00</BALAMT></LEDGERBAL>
</CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1></OFX>"#;

    #[test]
    fn parses_sgml_bank_statement() {
        let s = parse(SGML.as_bytes()).unwrap();
        assert_eq!(s.len(), 1);
        let st = &s[0];
        assert_eq!(st.account.mask.as_deref(), Some("6789"));
        assert_eq!(st.account.current_balance, Some(Cents(321055)));
        assert_eq!(st.transactions.len(), 2);
        assert_eq!(st.transactions[0].amount, Cents(-4217));
        assert_eq!(st.transactions[0].description, "PG&E AUTOPAY");
        assert_eq!(st.transactions[0].date, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());
        assert_eq!(st.transactions[1].external_id, "ofx:2026011602");
    }

    #[test]
    fn parses_xml_card_statement_with_natural_balance() {
        let s = parse(XML_CARD.as_bytes()).unwrap();
        assert_eq!(s[0].account.kind, FeedAccountKind::Credit);
        assert_eq!(s[0].account.current_balance, Some(Cents(25000)));
        assert_eq!(s[0].transactions[0].amount, Cents(-1999));
    }

    #[test]
    fn rejects_non_ofx() {
        assert!(matches!(parse(b"date,amount"), Err(OfxError::NotOfx)));
    }
}
