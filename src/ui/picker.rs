use anyhow::{Result, bail};

use super::prompt::Prompter;
use super::{KeyColumn, profile_table};
use crate::config::Config;
use crate::state::State;
use crate::time;

/// Lets the user choose a profile from a table, most recently used first. Returns its name.
pub fn pick_profile<'a>(
    config: &'a Config,
    history: &State,
    prompter: &mut dyn Prompter,
    message: &str,
) -> Result<&'a str> {
    let names = history.recent_first(config);
    if names.is_empty() {
        bail!("no profiles yet; add one with `lopi add`");
    }
    let (header, rows) = profile_table(config, history, &names, time::unix_now(), KeyColumn::Hide);
    let index = prompter.pick_row(message, &header, &rows)?;
    Ok(names[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;
    use crate::error::Abort;
    use crate::ui::prompt::scripted::{Answer, Scripted};

    fn config() -> Config {
        let mut config = Config::default();
        for (name, id) in [("kantor", "a"), ("vps", "b"), ("db", "c")] {
            let profile = Profile {
                id: Some(id.into()),
                ..Profile::new(format!("{name}.example"))
            };
            config.profiles.insert(name.into(), profile);
        }
        config
    }

    #[test]
    fn recent_first() {
        let cfg = config();
        let mut history = State::default();
        history.last_used.insert("b".into(), 10);
        let mut prompter = Scripted::new([Answer::Pick("kantor")]);

        let picked = pick_profile(&cfg, &history, &mut prompter, "Connect to").unwrap();
        assert_eq!(picked, "kantor");
        assert_eq!(prompter.asked, [r#"Connect to ["vps", "db", "kantor"]"#]);
    }

    #[test]
    fn escape_cancels() {
        let mut prompter = Scripted::new([Answer::Esc]);
        let err = pick_profile(&config(), &State::default(), &mut prompter, "x").unwrap_err();
        assert_eq!(err.downcast_ref::<Abort>(), Some(&Abort::Cancelled));
    }

    #[test]
    fn empty_config_explains() {
        let mut prompter = Scripted::new([]);
        let empty = Config::default();
        let err = pick_profile(&empty, &State::default(), &mut prompter, "x").unwrap_err();
        assert!(err.to_string().contains("lopi add"), "{err}");
    }
}
