use anyhow::Result;

use crate::config::paths;
use crate::output;

pub fn run() -> Result<i32> {
    output::print(&format!("{}\n", paths::config_file()?.display()))?;
    Ok(0)
}
