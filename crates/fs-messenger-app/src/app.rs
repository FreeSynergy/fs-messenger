//! `app_command`: opens the store, then the window (ADR 0063).
//!
//! Pattern: Facade consumer — arguments, the core, the window. The runtime lives as long as the window: the Backends run
//! the core on it.

use std::sync::Arc;

use fs_components_app::{WindowOptions, launch};

use crate::args::{HELP, parse};
use crate::shared::Shared;
use crate::store::open_repo;
use crate::window::build_window;

/// Run the program with the words after its name; the exit code.
pub fn app_command(words: &[String]) -> i32 {
    match run(words) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("fs-messenger: {message}");
            1
        }
    }
}

fn run(words: &[String]) -> Result<(), String> {
    let args = parse(words)?;
    if args.help {
        println!("{HELP}");
        return Ok(());
    }
    if args.version {
        println!("fs-messenger {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let repo = runtime.block_on(open_repo(args.store.as_deref()))?;
    let shared = Shared::new(Arc::new(repo), runtime.handle().clone());
    let app = build_window(&shared).map_err(|e| e.to_string())?;
    launch(WindowOptions::default(), app).map_err(|e| e.to_string())
}
