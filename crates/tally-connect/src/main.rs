//! `tally-connect` serves the Connect API; `tally-connect sweep` runs the
//! lapse sweeper once and exits. Configuration is environment variables only.

use std::process::ExitCode;
use tally_connect::config::Config;
use tally_connect::{AppState, proxy, stderr_log, sweep};

#[tokio::main]
async fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    if !matches!(command.as_deref(), None | Some("serve") | Some("sweep")) {
        eprintln!("usage: tally-connect [serve | sweep]   (configured by environment variables; see docs/CONNECT.md)");
        return ExitCode::from(2);
    }
    let state = match Config::from_env().map_err(|e| e.to_string()).and_then(|cfg| AppState::new(cfg, stderr_log())) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tally-connect: {e}");
            return ExitCode::FAILURE;
        }
    };

    if command.as_deref() == Some("sweep") {
        let report = sweep::sweep_once(&state, chrono::Utc::now().timestamp()).await;
        println!("{report:?}");
        return ExitCode::SUCCESS;
    }

    let listener = match tokio::net::TcpListener::bind(&state.cfg.bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("tally-connect: cannot listen on {}: {e}", state.cfg.bind);
            return ExitCode::FAILURE;
        }
    };
    (state.log)(&format!("listening on {}", state.cfg.bind));
    tokio::spawn(sweep::run_daily(state.clone()));
    match proxy::serve(state, listener, shutdown_signal()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tally-connect: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
