//! Writes typed changes into the user's TOML document without reformatting it.
//!
//! `apply` compares `before` and `after` (profiles matched by `id`) and touches only what
//! changed. Comments, blank lines, key order and keys lopi does not know survive.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

use super::model::{Auth, Config, Profile};

/// One stored field, compared by value so unchanged keys are left untouched.
#[derive(Debug, Clone, PartialEq)]
enum Field {
    Str(String),
    Int(i64),
    List(Vec<String>),
}

impl Field {
    fn to_value(&self) -> Value {
        match self {
            Field::Str(s) => Value::from(s.as_str()),
            Field::Int(n) => Value::from(*n),
            Field::List(items) => {
                let mut array = Array::new();
                for item in items {
                    array.push(item.as_str());
                }
                Value::Array(array)
            }
        }
    }
}

/// Profile fields in file order. Must list every field of `Profile`.
fn fields(p: &Profile) -> [(&'static str, Option<Field>); 10] {
    let text = |s: &Option<String>| s.clone().map(Field::Str);
    [
        ("host", Some(Field::Str(p.host.clone()))),
        ("user", text(&p.user)),
        ("port", p.port.map(|port| Field::Int(i64::from(port)))),
        ("key", text(&p.key)),
        ("jump", text(&p.jump)),
        (
            "forward",
            (!p.forward.is_empty()).then(|| Field::List(p.forward.clone())),
        ),
        (
            "auth",
            p.auth
                .and_then(Auth::as_str)
                .map(|auth| Field::Str(auth.into())),
        ),
        ("note", text(&p.note)),
        ("id", text(&p.id)),
        ("updated_at", text(&p.updated_at)),
    ]
}

/// Applies the difference between `before` (what `doc` contains) and `after` to `doc`.
/// Every profile in both configs must have a unique id (`meta::ensure_ids` + `stamp`).
pub fn apply(doc: &mut DocumentMut, before: &Config, after: &Config) -> Result<()> {
    set_root(
        doc,
        "schema_version",
        Some(Field::Int(i64::from(after.schema_version))),
        before.schema_version != after.schema_version || !doc.contains_key("schema_version"),
    );
    set_root(
        doc,
        "ssh_bin",
        after.ssh_bin.clone().map(Field::Str),
        before.ssh_bin != after.ssh_bin,
    );

    let profiles = profiles_table(doc)?;
    let old_by_id = by_id(before)?;
    let new_by_id = by_id(after)?;

    // Take out every table that is removed or renamed first, so a rename can never clash
    // with a name that is freed in the same change.
    let mut moved: BTreeMap<&str, Item> = BTreeMap::new();
    for (id, (old_name, _)) in &old_by_id {
        let keep_name = new_by_id.get(id).is_some_and(|(name, _)| name == old_name);
        if !keep_name {
            let item = profiles
                .remove(old_name)
                .with_context(|| format!("internal error: profile '{old_name}' not in file"))?;
            moved.insert(id, item);
        }
    }

    for (id, (name, profile)) in &new_by_id {
        match old_by_id.get(id) {
            Some((old_name, old_profile)) => {
                if old_name != name {
                    let item = moved.remove(id).expect("taken out above");
                    profiles.insert(name, item);
                }
                let table = profiles
                    .get_mut(name)
                    .and_then(Item::as_table_like_mut)
                    .with_context(|| format!("internal error: profile '{name}' is not a table"))?;
                update_fields(table, old_profile, profile);
            }
            None => {
                profiles.insert(name, Item::Table(new_table(profile)));
            }
        }
    }
    Ok(())
}

/// Writes ids that `meta::ensure_ids` added or replaced into `doc`, so `doc` matches
/// `with_ids` and can serve as the `before` side of [`apply`].
pub fn write_ids(doc: &mut DocumentMut, parsed: &Config, with_ids: &Config) -> Result<()> {
    let changed: Vec<(&String, &Profile)> = with_ids
        .profiles
        .iter()
        .filter(|(name, profile)| parsed.profiles.get(*name).map(|p| &p.id) != Some(&profile.id))
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    let profiles = profiles_table(doc)?;
    for (name, profile) in changed {
        let table = profiles
            .get_mut(name)
            .and_then(Item::as_table_like_mut)
            .with_context(|| format!("internal error: profile '{name}' not in file"))?;
        let id = profile.id.clone().expect("ensure_ids sets every id");
        set_value(table, "id", &Field::Str(id));
    }
    Ok(())
}

fn set_root(doc: &mut DocumentMut, key: &str, field: Option<Field>, changed: bool) {
    if !changed {
        return;
    }
    let root = doc.as_table_mut();
    match field {
        Some(field) => set_value(root, key, &field),
        None => {
            root.remove(key);
        }
    }
}

/// The `profiles` table, created (as an implicit table, so no bare `[profiles]` header
/// is written) when missing.
fn profiles_table(doc: &mut DocumentMut) -> Result<&mut dyn TableLike> {
    let root = doc.as_table_mut();
    if !root.contains_key("profiles") {
        let mut table = Table::new();
        table.set_implicit(true);
        root.insert("profiles", Item::Table(table));
    }
    root.get_mut("profiles")
        .and_then(Item::as_table_like_mut)
        .context("`profiles` in the profiles file is not a table; fix it by hand")
}

fn by_id(config: &Config) -> Result<BTreeMap<&str, (&str, &Profile)>> {
    let mut map = BTreeMap::new();
    for (name, profile) in &config.profiles {
        let Some(id) = profile.id.as_deref() else {
            bail!("internal error: profile '{name}' has no id");
        };
        if map.insert(id, (name.as_str(), profile)).is_some() {
            bail!("internal error: id '{id}' is used twice");
        }
    }
    Ok(map)
}

fn update_fields(table: &mut dyn TableLike, old: &Profile, new: &Profile) {
    for ((key, old_field), (_, new_field)) in fields(old).into_iter().zip(fields(new)) {
        if old_field == new_field {
            continue;
        }
        match new_field {
            Some(field) => set_value(table, key, &field),
            None => {
                table.remove(key);
            }
        }
    }
}

/// Sets `key`, keeping the old value's surrounding whitespace and trailing comment.
fn set_value(table: &mut dyn TableLike, key: &str, field: &Field) {
    let mut value = field.to_value();
    if let Some(old) = table.get(key).and_then(Item::as_value) {
        *value.decor_mut() = old.decor().clone();
    }
    table.insert(key, Item::Value(value));
}

fn new_table(profile: &Profile) -> Table {
    let mut table = Table::new();
    for (key, field) in fields(profile) {
        if let Some(field) = field {
            table.insert(key, Item::Value(field.to_value()));
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::store::parse;

    /// Applies `change` to the config parsed from `text` (every profile in `text` must
    /// have an id) and returns the new text, checking it parses back to the changed config.
    fn edit(text: &str, change: impl FnOnce(&mut Config)) -> String {
        let mut doc: DocumentMut = text.parse().unwrap();
        let before = parse(text).unwrap();
        let mut after = before.clone();
        change(&mut after);
        apply(&mut doc, &before, &after).unwrap();
        let out = doc.to_string();
        assert_eq!(
            parse(&out).unwrap(),
            after,
            "round trip of:
{out}"
        );
        out
    }

    const FILE: &str = r#"# my servers
schema_version = 1

# the office box
[profiles.kantor]
host = "10.0.0.5"   # LAN address
port = 22
color = "blue"      # not a lopi field
id = "id-kantor"

[profiles.vps]
host = "vps.example"
id = "id-vps"
"#;

    #[test]
    fn unchanged_config_keeps_the_file_byte_for_byte() {
        let mut doc: DocumentMut = FILE.parse().unwrap();
        let config = parse(FILE).unwrap();
        apply(&mut doc, &config, &config).unwrap();
        assert_eq!(doc.to_string(), FILE);
    }

    #[test]
    fn field_change_keeps_comments_and_unknown_keys() {
        let out = edit(FILE, |c| {
            let p = c.profiles.get_mut("kantor").unwrap();
            p.host = "10.0.0.6".into();
            p.port = None;
            p.user = Some("admin".into());
        });
        assert_eq!(
            out,
            r#"# my servers
schema_version = 1

# the office box
[profiles.kantor]
host = "10.0.0.6"   # LAN address
color = "blue"      # not a lopi field
id = "id-kantor"
user = "admin"

[profiles.vps]
host = "vps.example"
id = "id-vps"
"#
        );
    }

    #[test]
    fn rename_keeps_position_and_comment() {
        let out = edit(FILE, |c| {
            let p = c.profiles.remove("kantor").unwrap();
            c.profiles.insert("office".into(), p);
        });
        assert!(
            out.contains(
                "# the office box\n[profiles.office]\nhost = \"10.0.0.5\"   # LAN address"
            ),
            "{out}"
        );
        assert!(
            out.find("[profiles.office]") < out.find("[profiles.vps]"),
            "{out}"
        );
        assert!(!out.contains("[profiles.kantor]"), "{out}");
    }

    #[test]
    fn swap_names_in_one_change() {
        edit(FILE, |c| {
            let a = c.profiles.remove("kantor").unwrap();
            let b = c.profiles.remove("vps").unwrap();
            c.profiles.insert("vps".into(), a);
            c.profiles.insert("kantor".into(), b);
        });
    }

    #[test]
    fn add_and_remove() {
        let out = edit(FILE, |c| {
            c.profiles.remove("vps");
            c.profiles.insert(
                "db".into(),
                Profile {
                    user: Some("root".into()),
                    forward: vec!["L:5432:localhost:5432".into()],
                    id: Some("id-db".into()),
                    ..Profile::new("db.example")
                },
            );
        });
        assert!(!out.contains("vps"), "{out}");
        assert!(
            out.ends_with(
                "[profiles.db]\nhost = \"db.example\"\nuser = \"root\"\nforward = [\"L:5432:localhost:5432\"]\nid = \"id-db\"\n"
            ),
            "{out}"
        );
    }

    #[test]
    fn empty_file_gets_schema_and_profile_without_bare_header() {
        let out = edit("", |c| {
            c.profiles.insert(
                "vps".into(),
                Profile {
                    id: Some("id-vps".into()),
                    ..Profile::new("h")
                },
            );
        });
        assert_eq!(
            out,
            "schema_version = 1\n\n[profiles.vps]\nhost = \"h\"\nid = \"id-vps\"\n"
        );
    }

    #[test]
    fn auth_is_written_before_note() {
        let out = edit(FILE, |c| {
            c.profiles.get_mut("vps").unwrap().auth = Some(Auth::Password);
        });
        assert!(
            out.ends_with(
                "[profiles.vps]\nhost = \"vps.example\"\nid = \"id-vps\"\nauth = \"password\"\n"
            ),
            "{out}"
        );
        let out = edit(&out, |c| c.profiles.get_mut("vps").unwrap().auth = None);
        assert!(!out.contains("auth"), "{out}");
    }

    #[test]
    fn unknown_auth_from_a_newer_version_survives_edits() {
        let text = "[profiles.a]\nhost = \"h\"\nauth = \"fido\"\nid = \"id-a\"\n";
        let out = edit(text, |c| {
            let p = c.profiles.get_mut("a").unwrap();
            assert_eq!(p.auth, Some(Auth::Other));
            p.port = Some(2222);
        });
        assert!(out.contains("auth = \"fido\""), "{out}");
    }

    #[test]
    fn inline_profile_tables_are_edited_in_place() {
        let text = "[profiles]\nvps = { host = \"h\", id = \"id-vps\" }  # inline\n";
        let out = edit(text, |c| {
            c.profiles.get_mut("vps").unwrap().port = Some(2222)
        });
        assert!(out.contains("# inline"), "{out}");
    }
}
