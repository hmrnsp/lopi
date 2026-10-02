use anyhow::Result;

use crate::config::{paths, store};
use crate::state;
use crate::ui::{self, KeyColumn, table};
use crate::{output, time};

pub fn run(recent: bool) -> Result<i32> {
    let config = store::load()?;
    if config.profiles.is_empty() {
        eprintln!(
            "no profiles yet; add one with `lopi add <name> [user@]host` (file: {})",
            paths::config_file()?.display()
        );
        return Ok(0);
    }
    let history = state::load();
    let names: Vec<&str> = if recent {
        history.recent_first(&config)
    } else {
        config.profiles.keys().map(String::as_str).collect()
    };
    let (header, rows) =
        ui::profile_table(&config, &history, &names, time::unix_now(), KeyColumn::Show);
    output::print(&table::render(&header, &rows))?;
    Ok(0)
}
