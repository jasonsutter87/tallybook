//! The readable half of the tax package: a summary and a general ledger, laid
//! out with [`crate::pdf`]. The CSVs beside them carry the same numbers for
//! importing.

use crate::pdf::{Col, Doc, Row, Style, color::*};
use crate::tax_api::DISCLAIMER;
use chrono::NaiveDate;
use std::collections::HashMap;
use tally_core::Cents;
use tally_core::tax;
use tally_store::{Entry, Jurisdiction, Report1099, ScheduleCReport, TaxPayment};

fn money(c: Cents) -> String {
    if c.0 < 0 { format!("-${}", -c) } else { format!("${c}") }
}

fn long_date(d: NaiveDate) -> String {
    d.format("%B %-d, %Y").to_string()
}

fn footer(company: &str, what: &str) -> String {
    format!("{company} · {what} · Worksheets prepared from your books by Tallybook, not a filed return")
}

pub(crate) fn summary(company: &str, year: i32, prepared: NaiveDate, sc: &ScheduleCReport, r: &Report1099, payments: &[TaxPayment]) -> Vec<u8> {
    let c = &sc.schedule;
    let what = format!("Tax summary {year}");
    let mut d = Doc::new(&format!("{company} · {what}"), &footer(company, &what));
    d.hero("TAX SUMMARY", company, &format!("Tax year {year} · Prepared {}", long_date(prepared)));

    let paid: Cents = payments.iter().filter(|p| !p.voided).map(|p| p.amount).sum();
    let need_1099 = r.rows.iter().filter(|p| p.over_threshold).count();
    d.tiles(&[
        ("Gross income · line 7", money(c.line7_gross_income), INK),
        ("Total expenses · line 28", money(c.line28_total_expenses), INK),
        ("Net profit · line 31", money(c.line31_net_profit), if c.line31_net_profit.0 < 0 { NEG } else { POS }),
        ("Estimated tax paid", money(paid), INK),
        ("1099-NECs to file", need_1099.to_string(), INK),
        ("Accounts to review", sc.unconfirmed.to_string(), if sc.unconfirmed > 0 { WARN } else { INK }),
    ]);
    d.note("Before you file", DISCLAIMER, WARN_SOFT, WARN);

    // Schedule C, as the form lays it out.
    d.heading(
        "Schedule C worksheet",
        Some("Profit or loss from business, line by line. Meals are already limited to 50%, and expense lines with nothing on them are left out."),
    );
    let line = |n: &str, label: &str, v: Cents, style: Style| Row::new(style, &[n, label, &money(v)]);
    let mut rows = vec![
        line("1", "Gross receipts or sales", c.line1_gross_receipts, Style::Plain),
        line("2", "Returns and allowances", c.line2_returns, Style::Plain),
        line("3", "Subtract line 2 from line 1", c.line3, Style::Strong),
        line("4", "Cost of goods sold", c.line4_cogs, Style::Plain),
        line("5", "Gross profit", c.line5_gross_profit, Style::Strong),
        line("6", "Other income", c.line6_other_income, Style::Plain),
        line("7", "Gross income", c.line7_gross_income, Style::Strong),
    ];
    for e in c.expenses.iter().filter(|e| e.amount != Cents::ZERO) {
        let label = if e.booked != e.amount { format!("{} (50% of {} booked)", e.label, money(e.booked)) } else { e.label.to_string() };
        rows.push(line(e.number, &label, e.amount, Style::Plain));
    }
    rows.extend([
        line("28", "Total expenses", c.line28_total_expenses, Style::Strong),
        line("29", "Tentative profit", c.line29_tentative_profit, Style::Strong),
        line("30", "Home office (simplified method)", c.line30_home_office, Style::Plain),
        line("31", "Net profit", c.line31_net_profit, Style::Total),
    ]);
    if c.meals_disallowed != Cents::ZERO {
        rows.push(line("", "Meals not deductible (the other 50%)", c.meals_disallowed, Style::Muted));
    }
    d.table(&[Col::left("Line", 44.0), Col::left("Description", 340.0), Col::right("Amount", 120.0)], &rows, true);

    // The account behind each number.
    d.heading(
        "Where each number comes from",
        Some("Every business account with activity this year and the Schedule C line it feeds. “Suggested” means Tallybook picked the line and nobody has confirmed it yet."),
    );
    if sc.rows.is_empty() {
        d.para("No business activity yet this year.", 9.0, INK_3);
    } else {
        let rows: Vec<Row> = sc
            .rows
            .iter()
            .map(|m| {
                let info = tax::line_info(m.line);
                let status = if m.confirmed { "Confirmed" } else { "Suggested" };
                let row = Row::new(Style::Plain, &[&m.name, &format!("{} · {}", info.number, info.label), status, &money(m.amount)]);
                if m.confirmed { row } else { row.accent(2, WARN) }
            })
            .collect();
        d.table(
            &[Col::left("Account", 196.0), Col::left("Schedule C line", 172.0), Col::left("Status", 64.0), Col::right("Amount", 72.0)],
            &rows,
            true,
        );
    }

    // Contractors.
    d.heading(
        "1099 summary",
        Some(&format!(
            "Contractors paid this year. Anyone paid {} or more from a bank account needs a 1099-NEC; card payments are reported by the card network, not by you. Only the last four digits of a tax ID are kept.",
            money(r.threshold)
        )),
    );
    if r.rows.is_empty() {
        d.para("No contractor payments this year.", 9.0, INK_3);
    } else {
        let mut rows: Vec<Row> = r
            .rows
            .iter()
            .map(|p| {
                let name = match &p.legal_name {
                    Some(l) if !l.is_empty() && *l != p.name => format!("{} ({l})", p.name),
                    _ => p.name.clone(),
                };
                let tin = p.tin_last4.as_deref().map_or("—".to_string(), |l| format!("•••• {l}"));
                let w9 = if p.w9_on_file { "On file" } else { "Missing" };
                let nec = if p.over_threshold { "Required" } else { "—" };
                let row = Row::new(Style::Plain, &[&name, &tin, w9, &money(p.paid_reportable), &money(p.paid_by_card), nec]);
                match (p.over_threshold, p.w9_on_file) {
                    (true, false) => row.accent(2, WARN),
                    (true, true) => row.accent(5, BRAND_STRONG),
                    _ => row,
                }
            })
            .collect();
        let bank: Cents = r.rows.iter().map(|p| p.paid_reportable).sum();
        let card: Cents = r.rows.iter().map(|p| p.paid_by_card).sum();
        rows.push(Row::new(Style::Total, &["Total", "", "", &money(bank), &money(card), ""]));
        d.table(
            &[
                Col::left("Payee", 160.0),
                Col::left("Tax ID", 66.0),
                Col::left("W-9", 54.0),
                Col::right("By bank", 78.0),
                Col::right("By card", 78.0),
                Col::left("1099-NEC", 68.0),
            ],
            &rows,
            true,
        );
    }

    // Estimated payments.
    d.heading(
        "Estimated tax payments",
        Some(&format!("What you've paid toward {year} estimates. Voided payments are greyed out and left out of the total.")),
    );
    if payments.is_empty() {
        d.para(&format!("No estimated payments recorded for {year}."), 9.0, INK_3);
    } else {
        let mut rows: Vec<Row> = payments
            .iter()
            .map(|p| {
                let to = if p.jurisdiction == Jurisdiction::Federal { "Federal (IRS)" } else { "State" };
                let (status, style) = if p.voided { ("Voided", Style::Muted) } else { ("Paid", Style::Plain) };
                Row::new(style, &[&long_date(p.date), &format!("Q{}", p.quarter), to, status, &money(p.amount)])
            })
            .collect();
        rows.push(Row::new(Style::Total, &["Total paid", "", "", "", &money(paid)]));
        d.table(
            &[Col::left("Date", 124.0), Col::left("Quarter", 70.0), Col::left("Paid to", 120.0), Col::left("Status", 80.0), Col::right("Amount", 110.0)],
            &rows,
            true,
        );
    }
    d.finish()
}

/// Every business-side entry in the period, each followed by its lines.
pub(crate) fn ledger(company: &str, year: i32, (from, to): (NaiveDate, NaiveDate), prepared: NaiveDate, entries: &[Entry], names: &HashMap<i64, String>) -> Vec<u8> {
    let what = format!("General ledger {year}");
    let mut d = Doc::new(&format!("{company} · {what}"), &footer(company, &what));
    d.hero(
        "GENERAL LEDGER",
        company,
        &format!("Business side · {} to {} · Prepared {}", from.format("%b %-d, %Y"), to.format("%b %-d, %Y"), long_date(prepared)),
    );
    d.para(
        "Every entry for the year with the business accounts it touched, so nothing has to be taken on faith. Debits and credits balance entry by entry.",
        9.0,
        INK_2,
    );
    d.gap(6.0);

    let (mut dr_total, mut cr_total) = (Cents::ZERO, Cents::ZERO);
    let mut rows = Vec::new();
    for e in entries {
        let lines: Vec<_> = e.lines.iter().filter(|l| names.contains_key(&l.account_id)).collect();
        if lines.is_empty() {
            continue;
        }
        let memo = if e.voided_by_id.is_some() { format!("{} · voided", e.memo) } else { e.memo.clone() };
        rows.push(Row::new(Style::Group, &[&e.date.format("%b %-d").to_string(), &format!("#{}", e.id), &memo]).keep(lines.len()));
        for l in lines {
            let (dr, cr) = if l.amount.0 > 0 {
                dr_total += l.amount;
                (money(l.amount), String::new())
            } else {
                cr_total -= l.amount;
                (String::new(), money(-l.amount))
            };
            let account = names.get(&l.account_id).map_or("", |n| n.as_str());
            rows.push(Row::new(Style::Plain, &["", "", account, l.memo.as_deref().unwrap_or(""), &dr, &cr]));
        }
    }
    if rows.is_empty() {
        d.para("No entries in this period.", 9.0, INK_3);
    } else {
        rows.push(Row::new(Style::Total, &["Total", "", "", "", &money(dr_total), &money(cr_total)]));
        d.table(
            &[
                Col::left("Date", 52.0),
                Col::left("Entry", 44.0),
                Col::left("Account", 168.0),
                Col::left("Memo", 96.0),
                Col::right("Debit", 72.0),
                Col::right("Credit", 72.0),
            ],
            &rows,
            false,
        );
    }
    d.finish()
}
