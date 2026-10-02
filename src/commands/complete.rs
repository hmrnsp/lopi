use std::io::{self, Write};

use crate::config::{paths, store};
use crate::state;

/// Hidden `lopi __complete`, run by the shell on every Tab: prints profile names, most
/// recently used first. It must stay fast and silent, so it never writes files, never
/// prints warnings, and exits 0 even when the profiles file is missing or broken.
pub fn run() -> i32 {
    let Ok(config) = paths::config_file().and_then(|path| store::load_from(&path)) else {
        return 0;
    };
    let history = state::load();
    let mut out = io::stdout().lock();
    for name in history.recent_first(&config) {
        // A closed pipe (the shell stopped reading) is not worth reporting.
        if writeln!(out, "{name}").is_err() {
            break;
        }
    }
    0
}
