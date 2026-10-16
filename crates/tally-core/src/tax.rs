//! Tax preparation math for sole proprietors: Schedule C, self-employment
//! tax, a federal income-tax projection, safe-harbor estimates, quarterly
//! due dates and the 1099-NEC threshold.
//!
//! **This module estimates; it does not give tax advice or file anything.**
//! Every figure the UI shows carries the form line it came from, and years
//! without verified tables are refused rather than guessed. Tables live in
//! [`TAX_YEARS`] with their sources; update them each fall when the IRS and
//! SSA publish the next year's numbers.

// Amounts are written as dollars_cents (e.g. 250_000_00), hence the grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use crate::invoice::div_round;
use crate::money::Cents;
use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- filing status & tables

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilingStatus {
    Single,
    MarriedJoint,
    MarriedSeparate,
    HeadOfHousehold,
}

/// One rate applying from `from` (taxable income, whole dollars) upward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bracket {
    pub from_dollars: i64,
    /// Basis points: 22% = 2200.
    pub rate_bp: i64,
}

const fn b(from_dollars: i64, rate_bp: i64) -> Bracket {
    Bracket { from_dollars, rate_bp }
}

#[derive(Debug)]
pub struct TaxYear {
    pub year: i32,
    /// Where these numbers come from, shown in the UI next to every estimate.
    pub source: &'static str,
    pub standard_deduction: [(FilingStatus, i64); 4],
    pub brackets: [(FilingStatus, [Bracket; 7]); 4],
    /// Social Security wage base (SSA).
    pub ss_wage_base: i64,
    /// Above this taxable income (before QBI) the simplified QBI deduction isn't applied.
    pub qbi_threshold: [(FilingStatus, i64); 4],
    /// 1099-NEC reporting threshold for payments made in this year.
    pub nec_threshold: i64,
}

impl TaxYear {
    fn lookup<T: Copy>(table: &[(FilingStatus, T); 4], status: FilingStatus) -> T {
        table.iter().find(|(s, _)| *s == status).map(|(_, v)| *v).expect("every status present")
    }

    pub fn standard_deduction(&self, status: FilingStatus) -> Cents {
        Cents(Self::lookup(&self.standard_deduction, status) * 100)
    }

    pub fn brackets(&self, status: FilingStatus) -> [Bracket; 7] {
        Self::lookup(&self.brackets, status)
    }

    pub fn qbi_threshold(&self, status: FilingStatus) -> Cents {
        Cents(Self::lookup(&self.qbi_threshold, status) * 100)
    }
}

use FilingStatus::*;

/// Supported years. Figures as published; verify against irs.gov / ssa.gov.
pub static TAX_YEARS: &[TaxYear] = &[
    TaxYear {
        year: 2025,
        source: "IRS Rev. Proc. 2024-40 as amended by Pub. L. 119-21 (standard deduction); SSA 2025 wage base",
        standard_deduction: [(Single, 15_750), (MarriedJoint, 31_500), (MarriedSeparate, 15_750), (HeadOfHousehold, 23_625)],
        brackets: [
            (Single, [b(0, 1000), b(11_925, 1200), b(48_475, 2200), b(103_350, 2400), b(197_300, 3200), b(250_525, 3500), b(626_350, 3700)]),
            (MarriedJoint, [b(0, 1000), b(23_850, 1200), b(96_950, 2200), b(206_700, 2400), b(394_600, 3200), b(501_050, 3500), b(751_600, 3700)]),
            (MarriedSeparate, [b(0, 1000), b(11_925, 1200), b(48_475, 2200), b(103_350, 2400), b(197_300, 3200), b(250_525, 3500), b(375_800, 3700)]),
            (HeadOfHousehold, [b(0, 1000), b(17_000, 1200), b(64_850, 2200), b(103_350, 2400), b(197_300, 3200), b(250_500, 3500), b(626_350, 3700)]),
        ],
        ss_wage_base: 176_100,
        qbi_threshold: [(Single, 197_300), (MarriedJoint, 394_600), (MarriedSeparate, 197_300), (HeadOfHousehold, 197_300)],
        nec_threshold: 600,
    },
    TaxYear {
        year: 2026,
        source: "IRS Rev. Proc. 2025-32; SSA 2026 wage base; Pub. L. 119-21 (1099-NEC threshold)",
        standard_deduction: [(Single, 16_100), (MarriedJoint, 32_200), (MarriedSeparate, 16_100), (HeadOfHousehold, 24_150)],
        brackets: [
            (Single, [b(0, 1000), b(12_400, 1200), b(50_400, 2200), b(105_700, 2400), b(201_775, 3200), b(256_225, 3500), b(640_600, 3700)]),
            (MarriedJoint, [b(0, 1000), b(24_800, 1200), b(100_800, 2200), b(211_400, 2400), b(403_550, 3200), b(512_450, 3500), b(768_700, 3700)]),
            (MarriedSeparate, [b(0, 1000), b(12_400, 1200), b(50_400, 2200), b(105_700, 2400), b(201_775, 3200), b(256_225, 3500), b(384_350, 3700)]),
            (HeadOfHousehold, [b(0, 1000), b(17_700, 1200), b(67_450, 2200), b(105_700, 2400), b(201_750, 3200), b(256_200, 3500), b(640_600, 3700)]),
        ],
        ss_wage_base: 184_500,
        qbi_threshold: [(Single, 201_750), (MarriedJoint, 403_500), (MarriedSeparate, 201_750), (HeadOfHousehold, 201_750)],
        nec_threshold: 2_000,
    },
];

pub fn tax_year(year: i32) -> Option<&'static TaxYear> {
    TAX_YEARS.iter().find(|t| t.year == year)
}

pub fn supported_years() -> Vec<i32> {
    TAX_YEARS.iter().map(|t| t.year).collect()
}

fn pct(amount: Cents, bp: i64) -> Cents {
    Cents(div_round(i128::from(amount.0) * i128::from(bp), 10_000) as i64)
}

// ---------------------------------------------------------------- Schedule C

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleCLine {
    L1GrossReceipts,
    L2Returns,
    L4CostOfGoods,
    L6OtherIncome,
    L8Advertising,
    L9CarTruck,
    L10Commissions,
    L11ContractLabor,
    L13Depreciation,
    L14EmployeeBenefits,
    L15Insurance,
    L16aMortgageInterest,
    L16bOtherInterest,
    L17Legal,
    L18Office,
    L19Pension,
    L20aRentVehicles,
    L20bRentOther,
    L21Repairs,
    L22Supplies,
    L23Taxes,
    L24aTravel,
    L24bMeals,
    L25Utilities,
    L26Wages,
    L27aOther,
    /// Not a business deduction (owner draws, transfers, personal).
    NotDeductible,
}

pub struct LineInfo {
    pub line: ScheduleCLine,
    pub number: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

const fn li(line: ScheduleCLine, number: &'static str, label: &'static str, help: &'static str) -> LineInfo {
    LineInfo { line, number, label, help }
}

pub const SCHEDULE_C_LINES: &[LineInfo] = {
    use ScheduleCLine::*;
    &[
        li(L1GrossReceipts, "1", "Gross receipts or sales", "Sales and fees you earned from the business."),
        li(L2Returns, "2", "Returns and allowances", "Refunds and discounts you gave customers."),
        li(L4CostOfGoods, "4", "Cost of goods sold", "Inventory, materials and direct product costs (Part III)."),
        li(L6OtherIncome, "6", "Other income", "Interest on business accounts, other business income."),
        li(L8Advertising, "8", "Advertising", "Ads, marketing, promotion."),
        li(L9CarTruck, "9", "Car and truck expenses", "Business use of a vehicle (needs mileage or actual-cost records)."),
        li(L10Commissions, "10", "Commissions and fees", "Platform fees, payment processing, commissions."),
        li(L11ContractLabor, "11", "Contract labor", "Freelancers and contractors (who may need a 1099-NEC)."),
        li(L13Depreciation, "13", "Depreciation and section 179", "Equipment write-offs (Form 4562). Ask a pro."),
        li(L14EmployeeBenefits, "14", "Employee benefit programs", "Benefits for employees, not for yourself."),
        li(L15Insurance, "15", "Insurance (other than health)", "Liability, property, business insurance."),
        li(L16aMortgageInterest, "16a", "Mortgage interest", "Mortgage interest paid to banks on business property."),
        li(L16bOtherInterest, "16b", "Other interest", "Interest on business loans and cards."),
        li(L17Legal, "17", "Legal and professional services", "Lawyers, accountants, bookkeepers."),
        li(L18Office, "18", "Office expense", "Software, postage, small office costs."),
        li(L19Pension, "19", "Pension and profit-sharing plans", "Plans for employees."),
        li(L20aRentVehicles, "20a", "Rent: vehicles, machinery, equipment", "Leased equipment."),
        li(L20bRentOther, "20b", "Rent: other business property", "Office or studio rent."),
        li(L21Repairs, "21", "Repairs and maintenance", "Fixing equipment or business property."),
        li(L22Supplies, "22", "Supplies", "Materials and supplies used up in the business."),
        li(L23Taxes, "23", "Taxes and licenses", "Business licenses, payroll taxes, sales tax you paid. Not income tax."),
        li(L24aTravel, "24a", "Travel", "Airfare, lodging, transport while traveling for business."),
        li(L24bMeals, "24b", "Deductible meals", "Business meals. Generally only 50% is deductible."),
        li(L25Utilities, "25", "Utilities", "Power, internet, phone for the business."),
        li(L26Wages, "26", "Wages", "Wages paid to employees (not to yourself)."),
        li(L27aOther, "27a", "Other expenses", "Anything else ordinary and necessary. List it in Part V."),
        li(NotDeductible, "—", "Not on Schedule C", "Owner draws, transfers, personal or non-deductible items."),
    ]
};

pub fn line_info(line: ScheduleCLine) -> &'static LineInfo {
    SCHEDULE_C_LINES.iter().find(|l| l.line == line).expect("every line listed")
}

/// Best-guess line from an account's name, for first-time setup. The user confirms.
pub fn suggest_line(account_name: &str, is_income: bool, is_cogs: bool) -> ScheduleCLine {
    use ScheduleCLine::*;
    let n = account_name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if is_cogs {
        return L4CostOfGoods;
    }
    if is_income {
        return if has(&["interest", "other income"]) { L6OtherIncome } else if has(&["refund", "return", "discount"]) { L2Returns } else { L1GrossReceipts };
    }
    if has(&["meal", "dining", "restaurant", "food"]) {
        L24bMeals
    } else if has(&["travel", "airfare", "hotel", "lodging"]) {
        L24aTravel
    } else if has(&["advertis", "marketing", "promotion"]) {
        L8Advertising
    } else if has(&["contract", "freelanc", "subcontract"]) {
        L11ContractLabor
    } else if has(&["bank fee", "merchant", "processing", "commission", "stripe", "paypal fee"]) {
        L10Commissions
    } else if has(&["insurance"]) {
        L15Insurance
    } else if has(&["legal", "professional", "accounting", "bookkeep"]) {
        L17Legal
    } else if has(&["software", "subscription", "office", "postage"]) {
        L18Office
    } else if has(&["rent", "lease"]) {
        if has(&["equipment", "vehicle", "machine"]) { L20aRentVehicles } else { L20bRentOther }
    } else if has(&["repair", "maintenance"]) {
        L21Repairs
    } else if has(&["supplies", "materials"]) {
        L22Supplies
    } else if has(&["tax", "license", "permit"]) {
        L23Taxes
    } else if has(&["utilit", "internet", "phone", "electric"]) {
        L25Utilities
    } else if has(&["wage", "payroll", "salar"]) {
        L26Wages
    } else if has(&["interest"]) {
        L16bOtherInterest
    } else if has(&["car", "vehicle", "mileage", "fuel", "gas"]) {
        L9CarTruck
    } else if has(&["depreciation"]) {
        L13Depreciation
    } else {
        L27aOther
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleCLineTotal {
    pub line: ScheduleCLine,
    pub number: &'static str,
    pub label: &'static str,
    /// As booked (meals at 100%).
    pub booked: Cents,
    /// What goes on the form (meals at 50%).
    pub amount: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleC {
    pub line1_gross_receipts: Cents,
    pub line2_returns: Cents,
    pub line3: Cents,
    pub line4_cogs: Cents,
    pub line5_gross_profit: Cents,
    pub line6_other_income: Cents,
    pub line7_gross_income: Cents,
    pub expenses: Vec<ScheduleCLineTotal>,
    pub line28_total_expenses: Cents,
    pub line29_tentative_profit: Cents,
    pub line30_home_office: Cents,
    pub line31_net_profit: Cents,
    /// The half of meals that isn't deductible, shown so the gap from the P&L is explained.
    pub meals_disallowed: Cents,
}

/// `amounts` are natural-sign totals per line for the year (income positive,
/// expenses positive). `home_office` is the Form 8829 / simplified-method figure.
pub fn schedule_c(amounts: &[(ScheduleCLine, Cents)], home_office: Cents) -> ScheduleC {
    use ScheduleCLine::*;
    let sum = |l: ScheduleCLine| amounts.iter().filter(|(x, _)| *x == l).map(|(_, a)| *a).sum::<Cents>();
    let line1 = sum(L1GrossReceipts);
    let line2 = sum(L2Returns);
    let line3 = line1 - line2;
    let line4 = sum(L4CostOfGoods);
    let line5 = line3 - line4;
    let line6 = sum(L6OtherIncome);
    let line7 = line5 + line6;

    let mut expenses = Vec::new();
    let mut meals_disallowed = Cents::ZERO;
    for info in SCHEDULE_C_LINES {
        if matches!(info.line, L1GrossReceipts | L2Returns | L4CostOfGoods | L6OtherIncome | NotDeductible) {
            continue;
        }
        let booked = sum(info.line);
        if booked.is_zero() {
            continue;
        }
        let amount = if info.line == L24bMeals {
            let half = pct(booked, 5000);
            meals_disallowed = booked - half;
            half
        } else {
            booked
        };
        expenses.push(ScheduleCLineTotal { line: info.line, number: info.number, label: info.label, booked, amount });
    }
    let line28: Cents = expenses.iter().map(|e| e.amount).sum();
    let line29 = line7 - line28;
    ScheduleC {
        line1_gross_receipts: line1,
        line2_returns: line2,
        line3,
        line4_cogs: line4,
        line5_gross_profit: line5,
        line6_other_income: line6,
        line7_gross_income: line7,
        expenses,
        line28_total_expenses: line28,
        line29_tentative_profit: line29,
        line30_home_office: home_office,
        line31_net_profit: line29 - home_office,
        meals_disallowed,
    }
}

/// IRS simplified home-office method: $5 per square foot, up to 300 sq ft,
/// limited to the tentative profit.
pub fn simplified_home_office(square_feet: i64, tentative_profit: Cents) -> Cents {
    let allowed = Cents(square_feet.clamp(0, 300) * 500);
    allowed.min(tentative_profit.max(Cents::ZERO))
}

// ---------------------------------------------------------------- self-employment tax (Schedule SE)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SeTax {
    /// Net profit × 92.35%.
    pub net_earnings: Cents,
    pub social_security: Cents,
    pub medicare: Cents,
    pub total: Cents,
    /// Half of SE tax, deducted on Schedule 1.
    pub deductible_half: Cents,
}

/// Schedule SE. `w2_ss_wages` are Social Security wages from any job, which
/// use up part of the wage base.
pub fn se_tax(net_profit: Cents, w2_ss_wages: Cents, ty: &TaxYear) -> SeTax {
    let net_earnings = pct(net_profit, 9235);
    if net_earnings < Cents(400_00) {
        return SeTax { net_earnings, social_security: Cents::ZERO, medicare: Cents::ZERO, total: Cents::ZERO, deductible_half: Cents::ZERO };
    }
    let base_left = (Cents(ty.ss_wage_base * 100) - w2_ss_wages).max(Cents::ZERO);
    let social_security = pct(net_earnings.min(base_left), 1240);
    let medicare = pct(net_earnings, 290);
    let total = social_security + medicare;
    SeTax { net_earnings, social_security, medicare, total, deductible_half: pct(total, 5000) }
}

/// Additional Medicare tax (Form 8959) on self-employment earnings above the threshold.
pub fn additional_medicare(se_net_earnings: Cents, w2_medicare_wages: Cents, status: FilingStatus) -> Cents {
    let threshold = Cents(match status {
        MarriedJoint => 250_000_00,
        MarriedSeparate => 125_000_00,
        _ => 200_000_00,
    });
    let over = se_net_earnings + w2_medicare_wages - threshold.max(w2_medicare_wages);
    pct(over.max(Cents::ZERO).min(se_net_earnings), 90)
}

// ---------------------------------------------------------------- income tax projection

/// Regular federal income tax on `taxable` for `status`.
pub fn income_tax(taxable: Cents, status: FilingStatus, ty: &TaxYear) -> Cents {
    let brackets = ty.brackets(status);
    let mut tax = Cents::ZERO;
    for (i, br) in brackets.iter().enumerate() {
        let lo = Cents(br.from_dollars * 100);
        if taxable <= lo {
            break;
        }
        let hi = brackets.get(i + 1).map(|n| Cents(n.from_dollars * 100));
        let top = hi.map_or(taxable, |h| taxable.min(h));
        tax += pct(top - lo, br.rate_bp);
    }
    tax
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstimateInputs {
    pub status: FilingStatus,
    /// Business net profit (Schedule C line 31), projected for the full year.
    pub business_profit: Cents,
    /// W-2 wages, interest, a spouse's income, etc.
    pub other_income: Cents,
    /// Social Security / Medicare wages from W-2 jobs (usually equal to W-2 wages).
    pub w2_wages: Cents,
    /// Federal tax already withheld from paychecks.
    pub withholding: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Estimate {
    pub year: i32,
    pub source: &'static str,
    pub se: SeTax,
    pub additional_medicare: Cents,
    pub agi: Cents,
    pub standard_deduction: Cents,
    pub qbi_deduction: Cents,
    /// True when income is above the QBI threshold, where the real deduction
    /// depends on facts we don't have; we leave it out (conservative).
    pub qbi_limited: bool,
    pub taxable_income: Cents,
    pub income_tax: Cents,
    pub total_tax: Cents,
    pub withholding: Cents,
    /// Total tax minus withholding: roughly what estimated payments should cover.
    pub to_pay: Cents,
    /// Average rate on business profit, for a simple "set aside X%" tip.
    pub set_aside_bp: i64,
    /// Things this estimate deliberately ignores.
    pub not_included: &'static [&'static str],
}

pub const NOT_INCLUDED: &[&str] = &[
    "Itemized deductions (it uses the standard deduction)",
    "Tax credits (child, education, energy, etc.)",
    "Capital gains, dividends at special rates, and investment income tax",
    "Retirement contributions and health-insurance deductions",
    "Newer special deductions (tips, overtime, car-loan interest, seniors)",
    "State and local income tax",
];

pub fn estimate(inputs: &EstimateInputs, ty: &TaxYear) -> Estimate {
    let se = se_tax(inputs.business_profit.max(Cents::ZERO), inputs.w2_wages, ty);
    let addl = additional_medicare(se.net_earnings, inputs.w2_wages, inputs.status);
    let agi = (inputs.business_profit + inputs.other_income - se.deductible_half).max(Cents::ZERO);
    let std = ty.standard_deduction(inputs.status);
    let before_qbi = (agi - std).max(Cents::ZERO);
    let qbi_limited = before_qbi > ty.qbi_threshold(inputs.status);
    let qbi = if qbi_limited || inputs.business_profit.0 <= 0 {
        Cents::ZERO
    } else {
        // Simplified 20% QBI below the threshold: lesser of 20% of QBI or 20% of taxable income.
        let qbi_base = inputs.business_profit - se.deductible_half;
        pct(qbi_base.max(Cents::ZERO), 2000).min(pct(before_qbi, 2000))
    };
    let taxable = (before_qbi - qbi).max(Cents::ZERO);
    let income = income_tax(taxable, inputs.status, ty);
    let total = income + se.total + addl;
    // Share of total tax attributable to the business: total minus tax on other income alone.
    let without_business = {
        let agi_o = inputs.other_income.max(Cents::ZERO);
        income_tax((agi_o - std).max(Cents::ZERO), inputs.status, ty)
    };
    let business_tax = total - without_business;
    let set_aside_bp = if inputs.business_profit.0 > 0 {
        div_round(i128::from(business_tax.0) * 10_000, i128::from(inputs.business_profit.0)) as i64
    } else {
        0
    };
    Estimate {
        year: ty.year,
        source: ty.source,
        se,
        additional_medicare: addl,
        agi,
        standard_deduction: std,
        qbi_deduction: qbi,
        qbi_limited,
        taxable_income: taxable,
        income_tax: income,
        total_tax: total,
        withholding: inputs.withholding,
        to_pay: (total - inputs.withholding).max(Cents::ZERO),
        set_aside_bp,
        not_included: NOT_INCLUDED,
    }
}

/// Prior-year safe harbor: paying this much during the year (in four equal
/// installments, counting withholding) avoids the federal underpayment
/// penalty regardless of this year's income. 110% applies when last year's
/// AGI was over $150,000 ($75,000 married filing separately).
pub fn safe_harbor(last_year_total_tax: Cents, last_year_agi: Cents, status: FilingStatus) -> Cents {
    let limit = Cents(if status == MarriedSeparate { 75_000_00 } else { 150_000_00 });
    if last_year_agi > limit { pct(last_year_total_tax, 11_000) } else { last_year_total_tax }
}

/// Splits an annual amount into four installments (any odd cents go to the first).
pub fn installments(annual: Cents) -> [Cents; 4] {
    let q = Cents(annual.0 / 4);
    let rem = annual - Cents(q.0 * 4);
    [q + rem, q, q, q]
}

// ---------------------------------------------------------------- due dates

/// The four federal estimated-tax due dates for tax year `year`
/// (Apr 15, Jun 15, Sep 15, Jan 15 of the next year), moved past weekends.
/// Federal holidays can push a date one more business day; the UI says so.
pub fn quarterly_due_dates(year: i32) -> [NaiveDate; 4] {
    let roll = |d: NaiveDate| match d.weekday() {
        Weekday::Sat => d + chrono::Days::new(2),
        Weekday::Sun => d + chrono::Days::new(1),
        _ => d,
    };
    let d = |y, m, day| roll(NaiveDate::from_ymd_opt(y, m, day).expect("valid date"));
    [d(year, 4, 15), d(year, 6, 15), d(year, 9, 15), d(year + 1, 1, 15)]
}

/// Payment periods the installments cover (for "income so far" displays).
pub fn quarter_periods(year: i32) -> [(NaiveDate, NaiveDate); 4] {
    let d = |m, day| NaiveDate::from_ymd_opt(year, m, day).expect("valid date");
    [(d(1, 1), d(3, 31)), (d(4, 1), d(5, 31)), (d(6, 1), d(8, 31)), (d(9, 1), d(12, 31))]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(d: i64) -> Cents {
        Cents(d * 100)
    }

    #[test]
    fn every_year_has_ascending_brackets() {
        for ty in TAX_YEARS {
            for (status, brackets) in &ty.brackets {
                assert!(brackets.windows(2).all(|w| w[0].from_dollars < w[1].from_dollars && w[0].rate_bp < w[1].rate_bp), "{} {status:?}", ty.year);
                assert_eq!(brackets[0].from_dollars, 0);
            }
        }
        assert!(tax_year(2019).is_none(), "unknown years are refused, not guessed");
    }

    #[test]
    fn income_tax_by_hand_2026_single() {
        let ty = tax_year(2026).unwrap();
        // $60,000 taxable: 10% of 12,400 + 12% of 38,000 + 22% of 9,600
        // = 1,240 + 4,560 + 2,112 = 7,912
        assert_eq!(income_tax(usd(60_000), Single, ty), usd(7_912));
        assert_eq!(income_tax(Cents::ZERO, Single, ty), Cents::ZERO);
        // Exactly at a bracket edge.
        assert_eq!(income_tax(usd(12_400), Single, ty), usd(1_240));
    }

    #[test]
    fn se_tax_by_hand() {
        let ty = tax_year(2026).unwrap();
        // $50,000 profit → 46,175 net earnings; SS 12.4% = 5,725.70; Medicare 2.9% = 1,339.08
        let se = se_tax(usd(50_000), Cents::ZERO, ty);
        assert_eq!(se.net_earnings, usd(46_175));
        assert_eq!(se.social_security, Cents(5_725_70));
        assert_eq!(se.medicare, Cents(1_339_08));
        assert_eq!(se.total, Cents(7_064_78));
        assert_eq!(se.deductible_half, Cents(3_532_39));
        // Under $400 of net earnings: no SE tax.
        assert_eq!(se_tax(usd(400), Cents::ZERO, ty).total, Cents::ZERO);
    }

    #[test]
    fn wage_base_caps_social_security() {
        let ty = tax_year(2026).unwrap();
        // A W-2 job already used the whole base: only Medicare applies.
        let se = se_tax(usd(50_000), usd(200_000), ty);
        assert_eq!(se.social_security, Cents::ZERO);
        assert_eq!(se.medicare, Cents(1_339_08));
    }

    #[test]
    fn schedule_c_halves_meals_and_skips_non_deductible() {
        use ScheduleCLine::*;
        let c = schedule_c(
            &[
                (L1GrossReceipts, usd(80_000)),
                (L2Returns, usd(1_000)),
                (L24bMeals, Cents(1_001)),
                (L20bRentOther, usd(12_000)),
                (L18Office, usd(500)),
                (NotDeductible, usd(30_000)),
            ],
            usd(1_500),
        );
        assert_eq!(c.line3, usd(79_000));
        assert_eq!(c.line7_gross_income, usd(79_000));
        let meals = c.expenses.iter().find(|e| e.line == L24bMeals).unwrap();
        assert_eq!(meals.amount, Cents(501), "half of $10.01, rounded half away from zero");
        assert_eq!(c.meals_disallowed, Cents(500));
        assert_eq!(c.line28_total_expenses, usd(12_500) + Cents(501));
        assert_eq!(c.line31_net_profit, c.line29_tentative_profit - usd(1_500));
        assert!(c.expenses.iter().all(|e| e.line != NotDeductible));
        // Lines come out in form order.
        let order: Vec<_> = c.expenses.iter().map(|e| e.number).collect();
        assert_eq!(order, vec!["18", "20b", "24b"]);
    }

    #[test]
    fn simplified_home_office_caps() {
        assert_eq!(simplified_home_office(200, usd(50_000)), usd(1_000));
        assert_eq!(simplified_home_office(500, usd(50_000)), usd(1_500));
        assert_eq!(simplified_home_office(200, usd(300)), usd(300));
        assert_eq!(simplified_home_office(200, usd(-10)), Cents::ZERO);
    }

    #[test]
    fn full_estimate_is_internally_consistent() {
        let ty = tax_year(2026).unwrap();
        let e = estimate(
            &EstimateInputs { status: Single, business_profit: usd(80_000), other_income: Cents::ZERO, w2_wages: Cents::ZERO, withholding: Cents::ZERO },
            ty,
        );
        assert_eq!(e.agi, usd(80_000) - e.se.deductible_half);
        assert_eq!(e.taxable_income, e.agi - e.standard_deduction - e.qbi_deduction);
        assert_eq!(e.total_tax, e.income_tax + e.se.total + e.additional_medicare);
        assert!(!e.qbi_limited);
        assert!(e.qbi_deduction.0 > 0);
        // A plausible set-aside rate for $80k of profit: somewhere between 15% and 30%.
        assert!((1500..3000).contains(&e.set_aside_bp), "{}", e.set_aside_bp);
    }

    #[test]
    fn safe_harbor_and_installments() {
        assert_eq!(safe_harbor(usd(10_000), usd(90_000), Single), usd(10_000));
        assert_eq!(safe_harbor(usd(10_000), usd(200_000), Single), usd(11_000));
        assert_eq!(safe_harbor(usd(10_000), usd(80_000), MarriedSeparate), usd(11_000));
        assert_eq!(installments(Cents(10_003)), [Cents(2_503), Cents(2_500), Cents(2_500), Cents(2_500)]);
    }

    #[test]
    fn due_dates_skip_weekends() {
        // 2026: Apr 15 Wed, Jun 15 Mon, Sep 15 Tue, Jan 15 2027 Fri.
        let d = quarterly_due_dates(2026);
        assert_eq!(d[0], NaiveDate::from_ymd_opt(2026, 4, 15).unwrap());
        assert_eq!(d[3], NaiveDate::from_ymd_opt(2027, 1, 15).unwrap());
        // 2028: Apr 15 is a Saturday → Monday Apr 17.
        assert_eq!(quarterly_due_dates(2028)[0], NaiveDate::from_ymd_opt(2028, 4, 17).unwrap());
    }

    #[test]
    fn suggestions_cover_the_default_chart() {
        use ScheduleCLine::*;
        assert_eq!(suggest_line("Meals", false, false), L24bMeals);
        assert_eq!(suggest_line("Rent", false, false), L20bRentOther);
        assert_eq!(suggest_line("Software & Subscriptions", false, false), L18Office);
        assert_eq!(suggest_line("Contractors", false, false), L11ContractLabor);
        assert_eq!(suggest_line("Bank Fees", false, false), L10Commissions);
        assert_eq!(suggest_line("Service Revenue", true, false), L1GrossReceipts);
        assert_eq!(suggest_line("Interest Income", true, false), L6OtherIncome);
        assert_eq!(suggest_line("Mystery", false, false), L27aOther);
    }
}
