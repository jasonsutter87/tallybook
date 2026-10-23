//! How Tallybook holds up in a book years deep: about ten thousand entries
//! across two years and two thousand bank transactions waiting in Review.
//! Every screen's main call must answer well within what feels instant.
//!
//! Ignored by default because building the book takes a while. Run it with
//! `cargo test --release -p tally-api --test big_book -- --ignored --nocapture`.

use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tally_api::{App, MemorySecrets};

const ENTRIES: usize = 10_000;
const INBOX: usize = 2_000;

async fn call(app: &App, method: &str, params: Value) -> Value {
    app.call(method, params).await.unwrap_or_else(|e| panic!("{method}: {} ({})", e.message, e.code))
}

fn ofx(count: usize) -> String {
    let mut txns = String::new();
    for i in 0..count {
        let day = 1 + i % 28;
        txns.push_str(&format!(
            "<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>202610{day:02}<TRNAMT>-{}.{:02}<FITID>BIG{i}<NAME>MERCHANT {}</STMTTRN>\n",
            3 + i % 90,
            i % 100,
            i % 250
        ));
    }
    format!(
        "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\n\n<OFX>\n<BANKMSGSRSV1><STMTTRNRS><STMTRS>\n<CURDEF>USD\n\
         <BANKACCTFROM><BANKID>1<ACCTID>2<ACCTTYPE>CHECKING</BANKACCTFROM>\n<BANKTRANLIST>\n{txns}</BANKTRANLIST>\n\
         <LEDGERBAL><BALAMT>0<DTASOF>20261031</LEDGERBAL>\n</STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>"
    )
}

#[tokio::test]
#[ignore = "builds a 10,000-entry book; run with --ignored"]
async fn a_book_years_deep_stays_fast() {
    let dir = std::env::temp_dir().join(format!("tallybook-big-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = App::new(Arc::new(MemorySecrets::default()), dir.join("config")).with_file_remover(|p| std::fs::remove_file(p));
    call(&app, "book.create", json!({ "path": dir.join("big.tallybook"), "company_name": "Big", "sides": ["household"] })).await;
    let checking = call(&app, "accounts.create", json!({ "name": "Checking", "subtype": "bank", "opening_balance": 50_000_000, "opening_date": "2024-12-31" })).await;
    let accounts = call(&app, "accounts.list", Value::Null).await;
    let spending: Vec<Value> = accounts.as_array().unwrap().iter().filter(|a| a["kind"] == "expense").map(|a| a["id"].clone()).collect();

    // Two years of everyday spending.
    let built = Instant::now();
    for i in 0..ENTRIES {
        let date = chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + chrono::Days::new((i * 730 / ENTRIES) as u64);
        let cents = 100 + (i as i64 * 37) % 20_000;
        call(
            &app,
            "entries.post",
            json!({ "date": date, "memo": format!("Purchase {i}"), "lines": [
                { "account_id": spending[i % spending.len()], "amount": cents },
                { "account_id": checking, "amount": -cents }
            ] }),
        )
        .await;
    }
    call(&app, "import.ofx", json!({ "content": ofx(INBOX), "index": 0, "account_id": checking })).await;
    println!("built {ENTRIES} entries + {INBOX} inbox items in {:.1?}", built.elapsed());

    // Each screen's main call, timed.
    let checks: Vec<(&str, &str, Value)> = vec![
        ("Home", "dashboard", json!({})),
        ("Accounts", "accounts.list", Value::Null),
        ("Checking register", "accounts.register", json!({ "id": checking })),
        ("Review", "inbox.list", json!({})),
        ("Reviewed", "inbox.reviewed", json!({})),
        ("Month", "budget.month", json!({ "month": "2026-10" })),
        ("Alerts", "household.alerts", json!({ "month": "2026-10" })),
        ("Bills spotted", "recurring.suggestions", Value::Null),
        ("P&L, two years", "reports.pnl", json!({ "from": "2025-01-01", "to": "2026-12-31" })),
        ("Balance sheet", "reports.balance_sheet", json!({ "as_of": "2026-12-31" })),
        ("Trial balance", "reports.trial_balance", json!({ "as_of": "2026-12-31" })),
        ("Entries, one year", "entries.list", json!({ "from": "2026-01-01", "to": "2026-12-31" })),
    ];
    let budget = Duration::from_millis(if cfg!(debug_assertions) { 3000 } else { 500 });
    let mut slow = Vec::new();
    for (screen, method, params) in checks {
        let t = Instant::now();
        call(&app, method, params).await;
        let took = t.elapsed();
        println!("{screen:<20} {method:<24} {took:>8.1?}");
        if took > budget {
            slow.push(format!("{screen} ({method}) took {took:.1?}"));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(slow.is_empty(), "slower than {budget:?}: {slow:#?}");
}
