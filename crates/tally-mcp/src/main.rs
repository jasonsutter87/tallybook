//! `tallybook-mcp`: Tallybook as an MCP server on stdin/stdout.
//!
//! By default it shares the installed desktop app's recent books, license and
//! keychain, and opens the most recently used book read-only. See
//! docs/AGENTS.md for wiring it into Claude.

use std::sync::Arc;
use tally_api::{App, Keychain};
use tally_mcp::{Options, desktop_config_dir, parse_args, serve_stdio};

const HELP: &str = "tallybook-mcp: Tallybook for agents, as an MCP server over stdio.

USAGE: tallybook-mcp [--book <path>] [--write] [--dev] [--config-dir <dir>]

  --book <path>        open this .tallybook file (default: the most recently used book)
  --write              offer tools that change the book (default: read-only)
  --dev                use the development build's recent books and keychain
  --config-dir <dir>   where recent books and the license live (default: the desktop app's)";

#[tokio::main]
async fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("tallybook-mcp: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    if args.help {
        println!("{HELP}");
        return;
    }
    // The same split the desktop app makes between its release and dev builds.
    let (dir, service) = if args.dev { (desktop_config_dir().join("dev"), "app.tallybook.dev") } else { (desktop_config_dir(), "app.tallybook") };
    let app = Arc::new(App::new(Arc::new(Keychain::new(service)), args.config_dir.unwrap_or(dir)));
    if let Err(e) = serve_stdio(app, Options { write: args.write, book: args.book }).await {
        eprintln!("tallybook-mcp: {e}");
        std::process::exit(1);
    }
}
