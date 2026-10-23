//! `tallybook-mcp`: Tallybook as an MCP server on stdin/stdout.
//!
//! By default it shares the installed desktop app's recent books, license and
//! keychain, and opens the most recently used book read-only. See
//! docs/AGENTS.md for wiring it into Claude.

use std::path::PathBuf;
use std::sync::Arc;
use tally_api::{App, Keychain};
use tally_mcp::{Options, desktop_config_dir, serve_stdio};

const HELP: &str = "tallybook-mcp: Tallybook for agents, as an MCP server over stdio.

USAGE: tallybook-mcp [--book <path>] [--write] [--dev] [--config-dir <dir>]

  --book <path>        open this .tallybook file (default: the most recently used book)
  --write              offer tools that change the book (default: read-only)
  --dev                use the development build's recent books and keychain
  --config-dir <dir>   where recent books and the license live (default: the desktop app's)";

#[tokio::main]
async fn main() {
    let mut opts = Options { write: false, book: None };
    let (mut dev, mut config) = (false, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--write" => opts.write = true,
            "--dev" => dev = true,
            "--book" => opts.book = args.next().map(PathBuf::from),
            "--config-dir" => config = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            other => {
                eprintln!("tallybook-mcp: unknown argument {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    // The same split the desktop app makes between its release and dev builds.
    let (dir, service) = if dev { (desktop_config_dir().join("dev"), "app.tallybook.dev") } else { (desktop_config_dir(), "app.tallybook") };
    let app = Arc::new(App::new(Arc::new(Keychain::new(service)), config.unwrap_or(dir)));
    if let Err(e) = serve_stdio(app, opts).await {
        eprintln!("tallybook-mcp: {e}");
        std::process::exit(1);
    }
}
