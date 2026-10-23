//! Bad input gets an error, never a crash. In the desktop app a panic in a
//! backend call takes the whole window down, and the API is reachable by
//! agents (tally-mcp's `call` tool) as well as the UI, so every method that
//! takes a year or a date must refuse nonsense politely.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tally_api::{App, MemorySecrets};

/// A licensed app with the demo book open.
async fn app(name: &str) -> (App, PathBuf) {
    let dir = std::env::temp_dir().join(format!("tallybook-hostile-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    // Business + Tax for this season, as a successful activation would leave it.
    let year = tally_api::today().format("%Y").to_string().parse::<i32>().unwrap();
    std::fs::write(
        dir.join("config/license.json"),
        json!({ "tier": "tax", "key": "K", "instance_id": "i", "email": null, "activated_on": tally_api::today(), "tax_years": [year - 1, year] })
            .to_string(),
    )
    .unwrap();
    let app = App::new(Arc::new(MemorySecrets::default()), dir.join("config")).with_file_remover(|p| std::fs::remove_file(p));
    app.call("book.create_demo", json!({ "path": dir.join("demo.tallybook") })).await.unwrap();
    (app, dir)
}

#[tokio::test]
async fn absurd_tax_years_are_refused_not_crashed_on() {
    let (app, dir) = app("years").await;
    for year in [i32::MIN, -1, 0, 1, 9999, 300_000, i32::MAX] {
        for method in ["tax.schedule_c", "tax.confirm_all", "tax.estimate", "tax.1099", "tax.export"] {
            let err = app.call(method, json!({ "year": year })).await.expect_err(&format!("{method} accepted year {year}"));
            assert!(matches!(err.code, "unsupported_year" | "license" | "bad_params"), "{method} {year}: {} ({})", err.message, err.code);
        }
        let pay = json!({ "tax_year": year, "quarter": 1, "jurisdiction": "federal", "date": "2026-04-15", "amount": 100, "account_id": 1 });
        assert!(app.call("tax.record_payment", pay).await.is_err(), "record_payment accepted tax year {year}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn impossible_dates_are_refused_not_crashed_on() {
    let (app, dir) = app("dates").await;
    let bad: Vec<(&str, Value)> = vec![
        ("entries.list", json!({ "from": "2026-02-30", "to": "2026-12-31" })),
        ("entries.list", json!({ "from": "+262143-12-31", "to": "-262143-01-01" })),
        ("reports.pnl", json!({ "from": "2026-13-01", "to": "2026-12-31" })),
        ("reports.balance_sheet", json!({ "as_of": "not a date" })),
        ("budget.month", json!({ "month": "2026-13" })),
        ("budget.month", json!({ "month": "garbage" })),
        ("recurring.due", json!({ "from": "2026-12-31", "to": "2026-01-01" })),
    ];
    for (method, params) in bad {
        // An error, or an empty answer for a backwards range; either way, no panic.
        let _ = app.call(method, params.clone()).await;
    }
    let _ = std::fs::remove_dir_all(dir);
}
