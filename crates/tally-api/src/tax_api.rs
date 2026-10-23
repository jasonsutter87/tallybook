//! Tax center methods (`tax.*`). Everything here prepares and explains;
//! nothing files anything with anyone.

use crate::{ApiError, ApiResult, App, ok, params, today};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::result::Result;
use tally_core::Cents;
use tally_core::report::ProfitAndLoss;
use tally_core::tax::{self, EstimateInputs, FilingStatus, ScheduleCLine, TaxYear};
use tally_store::*;

/// Shown with every tax screen and written into every export.
pub const DISCLAIMER: &str = "Tallybook prepares worksheets and estimates from your books. It is not tax advice, \
it does not file anything, and it can't know everything about your situation. Review these figures with a tax \
professional (or carefully against the IRS instructions) before you file or pay.";

/// The other way to avoid the federal underpayment penalty, besides the
/// prior-year safe harbor: pay at least this share of the current year's tax
/// through estimates (IRC §6654(d)(1)(B)(i); Form 1040-ES instructions).
const CURRENT_YEAR_SAFE_HARBOR_PERCENT: i128 = 90;

/// A quarterly payment whose due date is this close shows as "due soon".
const DUE_SOON_DAYS: i64 = 30;

/// The highest state income-tax rate a profile accepts, in basis points:
/// 20%, well above any US state's top rate, so a bigger number is a typo
/// (900 meant as 9%, say), not a real rate.
const MAX_STATE_RATE_BP: i64 = 2_000;

#[derive(Deserialize)]
struct Year {
    year: Option<i32>,
}

#[derive(Deserialize)]
struct SetLine {
    account_id: i64,
    line: Option<ScheduleCLine>,
}

#[derive(Deserialize)]
struct SetPayee {
    entry_id: i64,
    contact_id: Option<i64>,
}

#[derive(Deserialize)]
struct SetContactTax {
    id: i64,
    #[serde(flatten)]
    tax: ContactTax,
}

#[derive(Deserialize)]
struct VoidPayment {
    id: TaxPaymentId,
}

#[derive(Serialize)]
struct LineView {
    line: ScheduleCLine,
    number: &'static str,
    label: &'static str,
    help: &'static str,
}

#[derive(Serialize)]
struct YearsView {
    supported: Vec<i32>,
    current: i32,
    /// Tax years this copy's license covers; empty without Business + Tax.
    licensed: Vec<i32>,
    disclaimer: &'static str,
}

/// The tax profile, kept as JSON in the book preference `tax.profile`
/// (settings key `pref.tax.profile`), so its field names are part of the
/// file format. Money fields are cents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxProfile {
    pub filing_status: FilingStatus,
    /// "safe_harbor" or "current_year".
    pub method: String,
    pub other_income: Cents,
    pub w2_wages: Cents,
    pub withholding: Cents,
    /// Last year's total tax (Form 1040 "total tax" line) and AGI, for safe harbor.
    pub last_year_tax: Option<Cents>,
    pub last_year_agi: Option<Cents>,
    /// Replaces the annualized projection when set.
    pub projected_profit: Option<Cents>,
    /// Simplified-method home office square footage (0 = none).
    pub home_office_sqft: i64,
    /// Rough state income-tax rate for set-aside tips, basis points.
    pub state_rate_bp: i64,
}

impl Default for TaxProfile {
    fn default() -> Self {
        TaxProfile {
            filing_status: FilingStatus::Single,
            method: "safe_harbor".into(),
            other_income: Cents::ZERO,
            w2_wages: Cents::ZERO,
            withholding: Cents::ZERO,
            last_year_tax: None,
            last_year_agi: None,
            projected_profit: None,
            home_office_sqft: 0,
            state_rate_bp: 0,
        }
    }
}

fn load_profile(s: &Store) -> Result<TaxProfile, StoreError> {
    Ok(s.pref("tax.profile")?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
}

fn require_year(year: i32) -> ApiResult<&'static TaxYear> {
    tax::tax_year(year).ok_or_else(|| {
        ApiError::new(
            "unsupported_year",
            format!("Tallybook doesn't have verified {year} tax tables yet, so it won't guess. Supported: {:?}.", tax::supported_years()),
        )
    })
}

#[derive(Serialize)]
struct QuarterView {
    quarter: u8,
    due: NaiveDate,
    /// What the plan says to pay this quarter (federal).
    planned: Cents,
    paid_federal: Cents,
    paid_state: Cents,
    status: &'static str,
}

#[derive(Serialize)]
struct EstimateView {
    year: i32,
    profile: TaxProfile,
    /// Schedule C line 31 so far this year.
    profit_to_date: Cents,
    /// Straight-line projection of the year from profit so far.
    profit_annualized: Cents,
    /// What the estimate actually used.
    profit_used: Cents,
    profit_basis: &'static str,
    estimate: tax::Estimate,
    /// Prior-year safe harbor (needs last year's tax), annual.
    safe_harbor: Option<Cents>,
    /// 90% of the current-year projection: the other way to avoid the penalty.
    ninety_percent_current: Cents,
    /// The annual amount the plan targets for estimated payments.
    plan_annual: Cents,
    plan_method: &'static str,
    quarters: Vec<QuarterView>,
    state_set_aside: Cents,
    payments: Vec<TaxPayment>,
    disclaimer: &'static str,
}

fn build_estimate(s: &Store, year: i32, ty: &'static TaxYear) -> ApiResult<EstimateView> {
    let profile = load_profile(s)?;
    let home_office_probe = s.schedule_c(year, Cents::ZERO)?;
    let home_office = tax::simplified_home_office(profile.home_office_sqft, home_office_probe.schedule.line29_tentative_profit);
    let profit_to_date = home_office_probe.schedule.line29_tentative_profit - home_office;

    let now = today();
    let (start, end) = year_range(year)?;
    let profit_annualized = if now < start {
        Cents::ZERO
    } else if now >= end {
        profit_to_date
    } else {
        let elapsed = (now - start).num_days() + 1;
        let total = (end - start).num_days() + 1;
        Cents(tally_core::invoice::div_round(i128::from(profit_to_date.0) * i128::from(total), i128::from(elapsed)) as i64)
    };
    let (profit_used, profit_basis) = match profile.projected_profit {
        Some(p) => (p, "your projection"),
        None if now >= end => (profit_to_date, "the full year's books"),
        None => (profit_annualized, "profit so far, extended to a full year"),
    };

    let estimate = tax::estimate(
        &EstimateInputs {
            status: profile.filing_status,
            business_profit: profit_used,
            other_income: profile.other_income,
            w2_wages: profile.w2_wages,
            withholding: profile.withholding,
        },
        ty,
    );
    let safe_harbor = profile.last_year_tax.map(|t| tax::safe_harbor(t, profile.last_year_agi.unwrap_or_default(), profile.filing_status));
    let ninety = Cents(tally_core::invoice::div_round(i128::from(estimate.total_tax.0) * CURRENT_YEAR_SAFE_HARBOR_PERCENT, 100) as i64);
    let (plan_total, plan_method) = match (profile.method.as_str(), safe_harbor) {
        ("safe_harbor", Some(sh)) => (sh, "safe_harbor"),
        _ => (estimate.total_tax, "current_year"),
    };
    let plan_annual = (plan_total - profile.withholding).max(Cents::ZERO);
    let per_quarter = tax::installments(plan_annual);

    let payments = s.tax_payments(year)?;
    let dues = tax::quarterly_due_dates(year);
    let mut quarters = Vec::new();
    for (i, due) in dues.iter().enumerate() {
        let q = i as u8 + 1;
        let paid = |j: Jurisdiction| -> Cents {
            payments.iter().filter(|p| !p.voided && p.quarter == q && p.jurisdiction == j).map(|p| p.amount).sum()
        };
        let paid_federal = paid(Jurisdiction::Federal);
        let status = if per_quarter[i].0 > 0 && paid_federal >= per_quarter[i] {
            "paid"
        } else if per_quarter[i].is_zero() {
            "nothing_due"
        } else if now > *due {
            "late"
        } else if (*due - now).num_days() <= DUE_SOON_DAYS {
            "due_soon"
        } else {
            "upcoming"
        };
        quarters.push(QuarterView { quarter: q, due: *due, planned: per_quarter[i], paid_federal, paid_state: paid(Jurisdiction::State), status });
    }
    let state_set_aside = Cents(tally_core::invoice::div_round(i128::from(profit_used.max(Cents::ZERO).0) * i128::from(profile.state_rate_bp), 10_000) as i64);

    Ok(EstimateView {
        year,
        profile,
        profit_to_date,
        profit_annualized,
        profit_used,
        profit_basis,
        estimate,
        safe_harbor,
        ninety_percent_current: ninety,
        plan_annual,
        plan_method,
        quarters,
        state_set_aside,
        payments,
        disclaimer: DISCLAIMER,
    })
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// Cents as plain dollars for the CSV files: `1234.50`, `-3.07`. No `$`,
/// no thousands separators, a leading `-` when negative, so any spreadsheet
/// reads it as a number.
fn dollars(c: Cents) -> String {
    let sign = if c.0 < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", c.0.abs() / 100, c.0.abs() % 100)
}

// ---- the files in an accountant package (see `App::export`)

/// Schedule C worksheet: the form's lines in order, one row each.
fn schedule_c_worksheet_csv(c: &tax::ScheduleC) -> String {
    let mut out = String::from("line,description,amount\n");
    for (n, label, v) in [
        ("1", "Gross receipts or sales", c.line1_gross_receipts),
        ("2", "Returns and allowances", c.line2_returns),
        ("3", "Subtract line 2 from line 1", c.line3),
        ("4", "Cost of goods sold", c.line4_cogs),
        ("5", "Gross profit", c.line5_gross_profit),
        ("6", "Other income", c.line6_other_income),
        ("7", "Gross income", c.line7_gross_income),
    ] {
        let _ = writeln!(out, "{n},{},{}", csv_cell(label), dollars(v));
    }
    for e in &c.expenses {
        let _ = writeln!(out, "{},{},{}", e.number, csv_cell(e.label), dollars(e.amount));
    }
    for (n, label, v) in [
        ("28", "Total expenses", c.line28_total_expenses),
        ("29", "Tentative profit", c.line29_tentative_profit),
        ("30", "Home office (simplified method)", c.line30_home_office),
        ("31", "Net profit", c.line31_net_profit),
    ] {
        let _ = writeln!(out, "{n},{},{}", csv_cell(label), dollars(v));
    }
    let _ = writeln!(out, ",Meals not deductible (50%),{}", dollars(c.meals_disallowed));
    out
}

/// Account to line mapping, so a preparer can audit every number.
fn schedule_c_mapping_csv(sc: &ScheduleCReport) -> String {
    let mut out = String::from("account,line,confirmed,amount\n");
    for r in &sc.rows {
        let _ = writeln!(out, "{},{},{},{}", csv_cell(&r.name), tax::line_info(r.line).number, r.confirmed, dollars(r.amount));
    }
    out
}

/// The year's business journal lines, one row per line.
fn general_ledger_csv(entries: &[Entry], names: &std::collections::HashMap<i64, String>) -> String {
    let mut out = String::from("entry,date,memo,account,debit,credit,line_memo\n");
    for e in entries {
        // Business lines only; a crossing entry's household half is left out.
        for l in e.lines.iter().filter(|l| names.contains_key(&l.account_id)) {
            let (dr, cr) = if l.amount.0 > 0 { (dollars(l.amount), String::new()) } else { (String::new(), dollars(-l.amount)) };
            let _ = writeln!(
                out,
                "{},{},{},{},{dr},{cr},{}",
                e.id,
                e.date,
                csv_cell(&e.memo),
                csv_cell(names.get(&l.account_id).map_or("", |n| n.as_str())),
                csv_cell(l.memo.as_deref().unwrap_or(""))
            );
        }
    }
    out
}

/// 1099 summary. Full TINs are deliberately not stored or exported.
fn form_1099_csv(r: &Report1099) -> String {
    let mut out = String::from("name,legal_name,tin_last4,w9_on_file,marked_1099,paid_from_bank,paid_by_card,needs_1099_nec\n");
    for p in &r.rows {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{}",
            csv_cell(&p.name),
            csv_cell(p.legal_name.as_deref().unwrap_or("")),
            p.tin_last4.as_deref().unwrap_or(""),
            p.w9_on_file,
            p.is_1099,
            dollars(p.paid_reportable),
            dollars(p.paid_by_card),
            p.over_threshold
        );
    }
    out
}

fn estimated_payments_csv(payments: &[TaxPayment]) -> String {
    let mut out = String::from("date,quarter,jurisdiction,amount,voided\n");
    for p in payments {
        let j = if p.jurisdiction == Jurisdiction::Federal { "federal" } else { "state" };
        let _ = writeln!(out, "{},Q{},{j},{},{}", p.date, p.quarter, dollars(p.amount), p.voided);
    }
    out
}

/// README.txt: what each file is, how to read the CSVs, and the headline figures.
fn export_readme(year: i32, ty: &TaxYear, book_file: &str, (from, to): (NaiveDate, NaiveDate), pnl: &ProfitAndLoss, sc: &ScheduleCReport) -> String {
    format!(
        "Tallybook tax package · {year}\n\
         Made {made} by Tallybook {version} from the book \"{book_file}\".\n\n{DISCLAIMER}\n\nFiles\n- Tax summary.pdf: a readable overview with the Schedule C worksheet, account mapping, 1099s and estimated payments.\n\
         - General ledger.pdf: every entry for the year, ready to read or print.\n\
         - Schedule C worksheet.csv: line-by-line figures (meals already limited to 50%).\n\
         - Schedule C mapping.csv: which account feeds which line, and whether you confirmed it.\n\
         - General ledger.csv: every entry dated {from} to {to}.\n\
         - 1099 summary.csv: payees, amounts paid from bank vs. card, and who crosses the ${} threshold.\n\
         - Estimated tax payments.csv: what you paid toward {year} estimates.\n\n\
         The .csv files are UTF-8 text, comma-separated, with a header row and quotes around any cell \
         that has a comma, quote or line break. Amounts are US dollars with two decimals and a leading \
         minus when negative, with no $ or thousands separators. Dates are YYYY-MM-DD. The general \
         ledger's entry numbers are the entry ids in the book, so any line can be traced back.\n\n\
         From the books: income {}, expenses {}, net income {}.\nUnconfirmed Schedule C mappings: {}.\nTax tables: {}.\n",
        ty.nec_threshold,
        dollars(pnl.total_income),
        dollars(pnl.total_cost_of_goods_sold + pnl.total_expenses),
        dollars(pnl.net_income),
        sc.unconfirmed,
        ty.source,
        made = today(),
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// The year a request is about: the current one when absent.
fn year_of(p: Value) -> ApiResult<i32> {
    Ok(params::<Year>(p)?.year.unwrap_or_else(|| today().year()))
}

impl App {
    pub(crate) async fn tax(&self, method: &str, p: Value) -> ApiResult<Value> {
        self.check_license(method, &p)?;
        match method {
            "tax.years" => ok(YearsView {
                supported: tax::supported_years(),
                current: today().year(),
                licensed: self.tax_years_licensed(),
                disclaimer: DISCLAIMER,
            }),
            "tax.lines" => ok(tax::SCHEDULE_C_LINES
                .iter()
                .map(|l| LineView { line: l.line, number: l.number, label: l.label, help: l.help })
                .collect::<Vec<_>>()),
            "tax.profile" => self.with(|s| load_profile(s)),
            "tax.set_profile" => self.set_profile(params(p)?),
            "tax.schedule_c" => self.schedule_c(year_of(p)?),
            "tax.set_line" => {
                let p: SetLine = params(p)?;
                self.with(|s| s.set_tax_line(p.account_id, p.line))
            }
            "tax.confirm_all" => self.confirm_all(year_of(p)?),
            "tax.estimate" => {
                let year = year_of(p)?;
                let ty = require_year(year)?;
                self.with_api(|s| build_estimate(s, year, ty))
            }
            "tax.record_payment" => {
                let p: NewTaxPayment = params(p)?;
                self.with(|s| s.record_tax_payment(&p))
            }
            "tax.void_payment" => {
                let p: VoidPayment = params(p)?;
                self.with(|s| s.void_tax_payment(p.id, today()))
            }
            "tax.1099" => {
                let year = year_of(p)?;
                require_year(year)?;
                self.with(|s| s.report_1099(year))
            }
            "tax.set_payee" => {
                let p: SetPayee = params(p)?;
                self.with(|s| s.set_entry_payee(p.entry_id, p.contact_id))
            }
            "tax.set_contact" => {
                let p: SetContactTax = params(p)?;
                self.with(|s| s.set_contact_tax(p.id, &p.tax))
            }
            "tax.export" => {
                let year = year_of(p)?;
                let ty = require_year(year)?;
                self.export(year, ty)
            }
            _ => Err(ApiError::new("unknown_method", format!("no such method: {method}"))),
        }
    }

    /// Tax prep needs the Tax license, and a year's figures need that year
    /// licensed too.
    fn check_license(&self, method: &str, p: &Value) -> ApiResult<()> {
        // Contractor details and payees are everyday bookkeeping; the rest is tax prep.
        if !matches!(method, "tax.years" | "tax.lines" | "tax.set_payee" | "tax.set_contact") {
            self.require(crate::license::Tier::Tax)?;
        }
        // Business + Tax is bought per tax season, so a year's figures need that year
        // licensed. A year with no tables at all gets that (truer) error from the method.
        let year = match method {
            "tax.schedule_c" | "tax.confirm_all" | "tax.estimate" | "tax.1099" | "tax.export" => Some(year_of(p.clone())?),
            "tax.record_payment" => p.get("tax_year").and_then(Value::as_i64).map(|y| y as i32),
            _ => None,
        };
        if let Some(year) = year.filter(|y| tax::tax_year(*y).is_some()) {
            self.require_tax_year(year)?;
        }
        Ok(())
    }

    fn set_profile(&self, profile: TaxProfile) -> ApiResult<Value> {
        if !matches!(profile.method.as_str(), "safe_harbor" | "current_year") {
            return Err(ApiError::new("bad_params", "method must be safe_harbor or current_year"));
        }
        if !(0..=MAX_STATE_RATE_BP).contains(&profile.state_rate_bp) {
            return Err(ApiError::new("rule", "state rate looks wrong (0–20%)"));
        }
        let json = serde_json::to_string(&profile).map_err(|e| ApiError::new("internal", e.to_string()))?;
        self.with(|s| s.set_pref("tax.profile", Some(&json)))
    }

    /// The Schedule C worksheet, with the simplified home-office deduction worked out from the profile.
    fn schedule_c(&self, year: i32) -> ApiResult<Value> {
        require_year(year)?;
        self.with_api(|s| {
            let profile = load_profile(s)?;
            let probe = s.schedule_c(year, Cents::ZERO)?;
            let home = tax::simplified_home_office(profile.home_office_sqft, probe.schedule.line29_tentative_profit);
            let report = s.schedule_c(year, home)?;
            Ok(json!({ "report": report, "home_office_sqft": profile.home_office_sqft, "disclaimer": DISCLAIMER }))
        })
    }

    /// Accepts every current suggestion as confirmed.
    fn confirm_all(&self, year: i32) -> ApiResult<Value> {
        require_year(year)?;
        self.with(|s| {
            let r = s.schedule_c(year, Cents::ZERO)?;
            for row in r.rows.iter().filter(|r| !r.confirmed) {
                s.set_tax_line(row.account_id, Some(row.line))?;
            }
            Ok(())
        })
    }

    /// Writes an accountant package to "<book folder>/Tax <year>/".
    fn export(&self, year: i32, ty: &'static TaxYear) -> ApiResult<Value> {
        let mut guard = self.lock();
        let book = guard.as_mut().ok_or_else(|| ApiError::new("no_book", "no book is open"))?;
        let dir: PathBuf = book.path.parent().unwrap_or(std::path::Path::new(".")).join(format!("Tax {year}"));
        std::fs::create_dir_all(&dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        let s = &book.store;
        // Business side only: the household's spending is nobody's tax business, and
        // since each side balances on its own the business lines still balance.
        let names: std::collections::HashMap<i64, String> =
            s.accounts_in(Some(tally_core::Scope::Business))?.into_iter().map(|a| (a.id, a.name)).collect();
        let (from, to) = year_range(year)?;
        let write = |name: &str, body: &[u8]| std::fs::write(dir.join(name), body).map_err(|e| ApiError::new("io", e.to_string()));

        let est = build_estimate(s, year, ty)?;
        let probe = s.schedule_c(year, Cents::ZERO)?;
        let home = tax::simplified_home_office(est.profile.home_office_sqft, probe.schedule.line29_tentative_profit);
        let sc = s.schedule_c(year, home)?;
        write("Schedule C worksheet.csv", schedule_c_worksheet_csv(&sc.schedule).as_bytes())?;
        write("Schedule C mapping.csv", schedule_c_mapping_csv(&sc).as_bytes())?;

        let entries = s.entries(from, to)?;
        write("General ledger.csv", general_ledger_csv(&entries, &names).as_bytes())?;

        let r = s.report_1099(year)?;
        write("1099 summary.csv", form_1099_csv(&r).as_bytes())?;
        write("Estimated tax payments.csv", estimated_payments_csv(&est.payments).as_bytes())?;

        // The same figures, laid out for people to read.
        let company = s.pref("company.name")?.unwrap_or_else(|| "Your business".into());
        write("Tax summary.pdf", &crate::tax_pdf::summary(&company, year, today(), &sc, &r, &est.payments))?;
        write("General ledger.pdf", &crate::tax_pdf::ledger(&company, year, (from, to), today(), &entries, &names))?;

        let pnl = s.profit_and_loss_in(from, to, Some(tally_core::Scope::Business))?;
        let book_file = book.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        write("README.txt", export_readme(year, ty, &book_file, (from, to), &pnl, &sc).as_bytes())?;
        ok(dir)
    }
}
