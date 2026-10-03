use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

/// A temporary config file plus a fake ssh that prints each argument on its own line,
/// writes `$FAKE_SSH_STDERR` (if set) to stderr and exits with `$FAKE_SSH_EXIT` (default 0).
struct Env {
    dir: TempDir,
    config: PathBuf,
    ssh: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("cfg").join("profiles.toml");
        let ssh = write_fake_ssh(&dir);
        Self { dir, config, ssh }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("lopi").unwrap();
        cmd.env("LOPI_CONFIG", &self.config)
            .env("LOPI_SSH_BIN", &self.ssh)
            .env("LOPI_DATA_DIR", self.dir.path().join("data"))
            .env_remove("FAKE_SSH_EXIT")
            .env_remove("FAKE_SSH_STDERR");
        cmd
    }

    fn write_config(&self, text: &str) {
        fs::create_dir_all(self.config.parent().unwrap()).unwrap();
        fs::write(&self.config, text).unwrap();
    }

    /// Runs lopi, expects success, returns stdout lines.
    fn ok(&self, args: &[&str]) -> Vec<String> {
        let out = self
            .cmd()
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        lines(&out.stdout)
    }

    /// Runs lopi, expects failure, returns stderr.
    fn fail(&self, args: &[&str]) -> String {
        let out = self
            .cmd()
            .args(args)
            .assert()
            .failure()
            .get_output()
            .clone();
        String::from_utf8_lossy(&out.stderr).into_owned()
    }
}

#[cfg(unix)]
fn write_fake_ssh(dir: &TempDir) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join("fake-ssh");
    fs::write(
        &path,
        "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done\n\
         if [ -n \"$FAKE_SSH_STDERR\" ]; then printf '%s\\n' \"$FAKE_SSH_STDERR\" >&2; fi\n\
         exit ${FAKE_SSH_EXIT:-0}\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(windows)]
fn write_fake_ssh(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("fake-ssh.cmd");
    fs::write(
        &path,
        "@echo off\r\n\
         if not defined FAKE_SSH_STDERR goto loop\r\n\
         echo %FAKE_SSH_STDERR% 1>&2\r\n\
         :loop\r\n\
         if \"%~1\"==\"\" goto end\r\n\
         echo %~1\r\n\
         shift\r\n\
         goto loop\r\n\
         :end\r\n\
         if not defined FAKE_SSH_EXIT set FAKE_SSH_EXIT=0\r\n\
         exit /b %FAKE_SSH_EXIT%\r\n",
    )
    .unwrap();
    path
}

fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect()
}

const TWO_PROFILES: &str = r#"
[profiles.kantor]
host = "10.0.0.5"
user = "root"
port = 2222
key = "/keys/id_kantor"

[profiles.kantin]
host = "kantin.example"
"#;

#[test]
fn help_works() {
    Env::new().cmd().arg("--help").assert().success();
}

#[test]
fn no_args_prints_help() {
    let out = Env::new()
        .cmd()
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&out).contains("Usage:"));
}

#[test]
fn connect_passes_options_and_remote_command() {
    let env = Env::new();
    env.write_config(TWO_PROFILES);
    assert_eq!(
        env.ok(&["kantor", "-L", "8080:localhost:80", "--", "uptime"]),
        [
            "-L",
            "8080:localhost:80",
            "-p",
            "2222",
            "-i",
            "/keys/id_kantor",
            "--",
            "root@10.0.0.5",
            "uptime",
        ]
    );
}

#[test]
fn connect_subcommand_works_too() {
    let env = Env::new();
    env.write_config(TWO_PROFILES);
    assert_eq!(
        env.ok(&["connect", "kantin", "-v"]),
        ["-v", "--", "kantin.example"]
    );
    assert_eq!(
        env.ok(&["connect", "kantin", "--", "ls"]),
        ["--", "kantin.example", "ls"]
    );
    assert_eq!(
        env.ok(&["connect", "kantin", "-v", "--", "ls", "-la"]),
        ["-v", "--", "kantin.example", "ls", "-la"]
    );
    env.cmd().arg("connect").assert().code(2);
}

#[test]
fn reserved_name_reachable_via_connect() {
    let env = Env::new();
    env.write_config("[profiles.list]\nhost = \"h\"\n");
    let out = env.cmd().args(["connect", "list"]).assert().success();
    let out = out.get_output();
    assert_eq!(lines(&out.stdout), ["--", "h"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("clashes with a subcommand"));
}

#[test]
fn prefixes_never_connect() {
    let env = Env::new();
    env.write_config(TWO_PROFILES);
    for (query, expected) in [
        (
            "kan",
            "type the full name: did you mean one of: kantin, kantor?",
        ),
        ("kanto", "type the full name: did you mean 'kantor'?"),
        ("db", "no profile named 'db' (see `lopi list`)"),
    ] {
        let out = env.cmd().arg(query).assert().failure().get_output().clone();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(expected), "{query}: {err}");
        assert!(out.stdout.is_empty(), "{query}: ssh must not run");
    }
}

#[test]
fn ssh_exit_code_is_passed_on() {
    let env = Env::new();
    env.write_config(TWO_PROFILES);
    env.cmd()
        .arg("kantin")
        .env("FAKE_SSH_EXIT", "255")
        .assert()
        .code(255);
}

#[test]
fn missing_ssh_gives_install_hint() {
    let env = Env::new();
    env.write_config(TWO_PROFILES);
    let out = env
        .cmd()
        .arg("kantin")
        .env("LOPI_SSH_BIN", "lopi-no-such-ssh")
        .assert()
        .failure()
        .get_output()
        .clone();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("install the OpenSSH client"), "{err}");
}

#[test]
fn broken_config_is_reported_with_location() {
    let env = Env::new();
    env.write_config("[profiles.kantor\nhost = 1\n");
    let err = env.fail(&["kantor"]);
    assert!(
        err.contains("is not valid") && err.contains("line 1"),
        "{err}"
    );
}

#[test]
fn broken_config_is_not_overwritten_by_add() {
    let env = Env::new();
    env.write_config("not = = toml");
    env.fail(&["add", "vps", "h"]);
    assert_eq!(fs::read_to_string(&env.config).unwrap(), "not = = toml");
}

#[test]
fn path_prints_config_file() {
    let env = Env::new();
    assert_eq!(env.ok(&["path"]), [env.config.display().to_string()]);
}

#[test]
fn add_list_connect() {
    let env = Env::new();
    let empty = env
        .cmd()
        .arg("list")
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&empty.stderr).contains("no profiles yet"));

    env.ok(&[
        "add",
        "vpsku",
        "root@103.1.2.3",
        "-p",
        "2222",
        "-i",
        "~/.ssh/id_vps",
    ]);
    env.ok(&["add", "kantor", "10.0.0.5", "--note", "office box"]);
    assert_eq!(
        env.ok(&["list"]),
        [
            "NAME    TARGET               KEY            AUTH   NOTE",
            "kantor  10.0.0.5                            agent  office box",
            "vpsku   root@103.1.2.3:2222  ~/.ssh/id_vps  key",
        ]
    );
    let args = env.ok(&["vpsku"]);
    assert_eq!(&args[..3], ["-p", "2222", "-i"]);
    assert!(args[3].ends_with("id_vps") && !args[3].starts_with('~'));
    assert_eq!(&args[4..], ["--", "root@103.1.2.3"]);
}

#[test]
fn add_rejects_bad_input() {
    let env = Env::new();
    env.ok(&["add", "vps", "h"]);
    // clap already rejects `add x -oProxyCommand=evil`; `--` gets the value past clap,
    // so our own validation is what stops it here.
    let cases: [(&[&str], &str); 6] = [
        (&["add", "vps", "h2"], "already exists"),
        (&["add", "list", "h"], "reserved"),
        (
            &["add", "x", "--", "-oProxyCommand=evil"],
            "must not start with '-'",
        ),
        (
            &["add", "x", "root@-oProxyCommand=evil"],
            "must not start with '-'",
        ),
        (&["add", "x", "root@"], "host is empty"),
        (&["add", "my box", "h"], "use only letters"),
    ];
    for (args, expected) in cases {
        let err = env.fail(args);
        assert!(err.contains(expected), "{args:?}: {err}");
    }
    assert_eq!(env.ok(&["list"]).len(), 2);
}

#[test]
fn rm_needs_exact_name_and_confirmation() {
    let env = Env::new();
    env.ok(&["add", "kantor", "h"]);

    let err = env.fail(&["rm", "kan"]);
    assert!(
        err.contains("type the full name: did you mean 'kantor'?"),
        "{err}"
    );

    // Tests run without a terminal, so asking is impossible.
    let err = env.fail(&["rm", "kantor"]);
    assert!(
        err.contains("use -y to remove without confirmation"),
        "{err}"
    );
    assert_eq!(env.ok(&["list"]).len(), 2);

    assert_eq!(env.ok(&["rm", "kantor", "-y"]), ["removed 'kantor'"]);
    env.fail(&["kantor"]);
}

#[test]
fn edit_changes_only_given_fields() {
    let env = Env::new();
    env.ok(&[
        "add", "kantor", "root@h", "-p", "2222", "-i", "~/k", "-n", "note",
    ]);
    env.ok(&["edit", "kantor", "--host", "10.0.0.5", "-p", "2200"]);
    let args = env.ok(&["kantor"]);
    let key = dirs::home_dir().unwrap().join("k");
    assert_eq!(
        args,
        [
            "-p",
            "2200",
            "-i",
            &key.display().to_string(),
            "--",
            "root@10.0.0.5"
        ]
    );

    // "" clears optional fields
    env.ok(&["edit", "kantor", "--user", "", "--key", "", "--note", ""]);
    assert_eq!(env.ok(&["kantor"]), ["-p", "2200", "--", "10.0.0.5"]);
    // the connection above shows up as LAST USED
    assert_eq!(
        env.ok(&["list"])[1],
        "kantor  10.0.0.5:2200       agent  just now"
    );
}

#[test]
fn edit_rename_and_errors() {
    let env = Env::new();
    env.ok(&["add", "kantor", "h"]);
    env.ok(&["add", "vps", "v"]);

    let err = env.fail(&["edit", "kantor"]);
    assert!(err.contains("e.g. `lopi edit kantor --port 2200"), "{err}");
    assert!(
        env.fail(&["edit", "kan", "-p", "1"])
            .contains("did you mean")
    );
    assert!(
        env.fail(&["edit", "kantor", "--rename", "vps"])
            .contains("already exists")
    );
    assert!(
        env.fail(&["edit", "kantor", "--rename", "rm"])
            .contains("reserved")
    );
    assert!(
        env.fail(&["edit", "kantor", "--host=-x"])
            .contains("must not start with '-'")
    );

    assert_eq!(
        env.ok(&["edit", "kantor", "--rename", "office"]),
        ["updated 'kantor', now named 'office'"]
    );
    assert_eq!(env.ok(&["office"]), ["--", "h"]);
    env.fail(&["kantor"]);
}

#[test]
fn names_are_case_sensitive() {
    let env = Env::new();
    env.ok(&["add", "kantor", "h", "-p", "22"]);
    let saved = fs::read_to_string(&env.config).unwrap();

    let out = env
        .cmd()
        .arg("KANTOR")
        .assert()
        .failure()
        .get_output()
        .clone();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("names are case-sensitive: did you mean 'kantor'?"),
        "{err}"
    );
    assert!(out.stdout.is_empty(), "ssh must not run");
    for args in [
        &["Kantor", "--", "uptime"][..],
        &["connect", "KANTOR"][..],
        &["rm", "KANTOR", "-y"][..],
        &["edit", "Kantor", "-p", "2222"][..],
        &["passwd", "KANTOR", "--remove"][..],
    ] {
        let out = env
            .cmd()
            .args(args)
            .env("LOPI_KEYRING_SERVICE", "lopi-test-never-written")
            .assert()
            .failure()
            .get_output()
            .clone();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("did you mean 'kantor'?"), "{args:?}: {err}");
        assert!(out.stdout.is_empty(), "{args:?}: ssh must not run");
    }
    assert_eq!(
        fs::read_to_string(&env.config).unwrap(),
        saved,
        "nothing changed"
    );

    let err = env.fail(&["add", "Kantor", "h2"]);
    assert!(err.contains("differ only in letter case"), "{err}");
    let err = env.fail(&["add", "LIST", "h"]);
    assert!(err.contains("reserved"), "{err}");

    // a case-only rename of the same profile is allowed; the old spelling is then gone
    assert_eq!(
        env.ok(&["edit", "kantor", "--rename", "Kantor"]),
        ["updated 'kantor', now named 'Kantor'"]
    );
    assert_eq!(env.ok(&["Kantor"]), ["-p", "22", "--", "h"]);
    env.fail(&["kantor"]);
    assert_eq!(env.ok(&["rm", "Kantor", "-y"]), ["removed 'Kantor'"]);
}

#[test]
fn typo_gets_a_suggestion() {
    let env = Env::new();
    env.ok(&["add", "kantor", "h"]);
    let err = env.fail(&["kantro"]);
    assert!(err.contains("did you mean 'kantor'?"), "{err}");
}

#[test]
fn warnings_are_printed_once() {
    let env = Env::new();
    env.write_config("[profiles.list]\nhost = \"h\"\n[profiles.vps]\nhost = \"v\"\n");
    let out = env
        .cmd()
        .args(["rm", "vps", "-y"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.matches("clashes with a subcommand").count(),
        1,
        "{stderr}"
    );
}

#[test]
fn edit_can_clear_port() {
    let env = Env::new();
    env.ok(&["add", "vps", "h", "-p", "2222"]);
    env.ok(&["edit", "vps", "--port", ""]);
    assert_eq!(env.ok(&["vps"]), ["--", "h"]);
    let err = env.fail(&["edit", "vps", "--port", "0"]);
    assert!(err.contains("not a port"), "{err}");
}

#[test]
fn key_is_stored_portably() {
    let env = Env::new();
    let home = dirs::home_dir().unwrap();
    let absolute = home.join(".ssh").join("lopi_test_missing_key");
    let out = env
        .cmd()
        .args(["add", "vps", "h", "-i"])
        .arg(&absolute)
        .assert()
        .success()
        .get_output()
        .clone();
    // the key does not exist: warned, but still saved
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist (saved anyway)"));
    let config = fs::read_to_string(&env.config).unwrap();
    assert!(
        config.contains("key = \"~/.ssh/lopi_test_missing_key\""),
        "{config}"
    );
}

#[test]
fn ssh_bin_from_config_and_saved_jump_forward() {
    let env = Env::new();
    env.write_config(&format!(
        "ssh_bin = '{}'\n\
         [profiles.app]\n\
         host = \"10.0.0.9\"\n\
         jump = \"admin@bastion:2200\"\n\
         forward = [\"L:8080:localhost:80\", \"D:1080\"]\n",
        env.ssh.display()
    ));
    let out = env
        .cmd()
        .env_remove("LOPI_SSH_BIN")
        .arg("app")
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(
        lines(&out.stdout),
        [
            "-J",
            "admin@bastion:2200",
            "-L",
            "8080:localhost:80",
            "-D",
            "1080",
            "--",
            "10.0.0.9"
        ]
    );
}

#[test]
fn saved_file_has_schema_and_metadata() {
    let env = Env::new();
    env.ok(&["add", "vps", "h"]);
    let text = fs::read_to_string(&env.config).unwrap();
    assert!(text.starts_with("schema_version = 1\n"), "{text}");
    assert!(
        text.contains("\nid = \"") && text.contains("\nupdated_at = \""),
        "{text}"
    );
}

#[test]
fn hand_written_comments_survive_every_command() {
    let env = Env::new();
    env.write_config(
        "# my servers\n\n# office\n[profiles.kantor]\nhost = \"10.0.0.5\" # LAN\nfavorite = true\n",
    );
    env.ok(&["add", "vps", "h"]);
    env.ok(&["edit", "kantor", "--rename", "office", "-p", "2222"]);
    env.ok(&["rm", "vps", "-y"]);
    let text = fs::read_to_string(&env.config).unwrap();
    for kept in [
        "# my servers\n",
        "# office\n[profiles.office]\n",
        "host = \"10.0.0.5\" # LAN\n",
        "favorite = true\n",
        "port = 2222\n",
    ] {
        assert!(text.contains(kept), "{kept:?} missing in:\n{text}");
    }
    assert!(!text.contains("vps"), "{text}");
}

#[test]
fn backups_go_to_the_data_dir() {
    let env = Env::new();
    env.ok(&["add", "a", "h"]);
    env.ok(&["add", "b", "h"]);
    env.ok(&["rm", "a", "-y"]);
    let backups = env.dir.path().join("data").join("backups");
    assert_eq!(fs::read_dir(backups).unwrap().count(), 2);
}

#[test]
fn list_recent_orders_by_last_connection() {
    let env = Env::new();
    for name in ["a", "b", "c"] {
        env.ok(&["add", name, "h"]);
    }
    assert_eq!(env.ok(&["list"])[0], "NAME  TARGET  KEY  AUTH   NOTE");

    env.ok(&["b"]);
    let recent = env.ok(&["list", "--recent"]);
    assert_eq!(recent[0], "NAME  TARGET  KEY  AUTH   LAST USED  NOTE");
    let order: Vec<&str> = recent[1..].iter().map(|l| &l[..1]).collect();
    assert_eq!(order, ["b", "a", "c"]);

    // a rename keeps the history
    env.ok(&["edit", "b", "--rename", "bb"]);
    assert!(env.ok(&["list", "-r"])[1].starts_with("bb "));
}

#[test]
fn list_json_for_scripts() {
    let env = Env::new();
    let json = |args: &[&str]| -> serde_json::Value {
        serde_json::from_str(&env.ok(args).join("\n")).unwrap()
    };
    // no profiles: an empty array on stdout, not the hint
    assert_eq!(json(&["list", "--json"]), serde_json::json!([]));

    env.ok(&["add", "a", "root@h", "-p", "2222", "-n", "first"]);
    env.ok(&["add", "b", "h2"]);
    env.ok(&["b"]);
    let all = json(&["list", "--json"]);
    assert_eq!(all[0]["name"], "a");
    assert_eq!(all[0]["user"], "root");
    assert_eq!(all[0]["port"], 2222);
    assert_eq!(all[0]["note"], "first");
    assert_eq!(all[0]["last_used"], serde_json::Value::Null);
    assert_eq!(all[1]["auth"], "agent");
    assert!(all[1]["last_used"].as_str().unwrap().ends_with('Z'));

    let recent = json(&["list", "--json", "--recent"]);
    assert_eq!(recent[0]["name"], "b");
}

#[test]
fn jump_and_forward_from_the_command_line() {
    let env = Env::new();
    env.ok(&["add", "bastion", "admin@b.example", "-p", "2200"]);
    env.ok(&[
        "add",
        "app",
        "10.0.0.9",
        "--jump",
        "bastion",
        "-f",
        "L:8080:localhost:80",
        "-f",
        "D:1080",
    ]);
    assert_eq!(
        env.ok(&["app"]),
        [
            "-J",
            "admin@b.example:2200",
            "-L",
            "8080:localhost:80",
            "-D",
            "1080",
            "--",
            "10.0.0.9"
        ]
    );
    assert!(env.ok(&["list"])[1].starts_with("app      10.0.0.9 via bastion"));

    // edit replaces the forward list; "" clears jump and forwards
    env.ok(&["edit", "app", "-f", "R:9000:localhost:9000"]);
    assert_eq!(
        env.ok(&["app"])[..4],
        ["-J", "admin@b.example:2200", "-R", "9000:localhost:9000"]
    );
    env.ok(&["edit", "app", "--jump", "", "--forward", ""]);
    assert_eq!(env.ok(&["app"]), ["--", "10.0.0.9"]);

    let err = env.fail(&["edit", "app", "-f", "8080:localhost:80"]);
    assert!(err.contains("must start with L:, R: or D:"), "{err}");
}

#[test]
fn jump_through_a_profile_with_a_key_explains_once() {
    let env = Env::new();
    env.ok(&["add", "bastion", "b.example", "-i", "~/.ssh/id_bastion"]);
    let out = env
        .cmd()
        .args(["add", "app", "h", "-J", "bastion"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does not use the key of 'bastion'"),
        "{stderr}"
    );

    // not repeated on every connection
    let out = env.cmd().arg("app").assert().success().get_output().clone();
    assert!(String::from_utf8_lossy(&out.stderr).is_empty());
}

#[test]
fn jump_in_another_case_warns_and_is_a_host() {
    let env = Env::new();
    let warning = "'BASTION' is not a profile (did you mean 'bastion'?); using it as a host name";
    env.ok(&["add", "bastion", "admin@b.example", "-p", "2200"]);

    let out = env
        .cmd()
        .args(["add", "app", "10.0.0.9", "-J", "BASTION"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&out.stderr).contains(warning));

    // every connection warns again, and BASTION goes to ssh as a host name
    let out = env.cmd().arg("app").assert().success().get_output().clone();
    assert_eq!(lines(&out.stdout), ["-J", "BASTION", "--", "10.0.0.9"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains(warning));

    let ssh = fake_ssh_with_version(&env);
    let out = env
        .cmd()
        .arg("doctor")
        .env("LOPI_SSH_BIN", &ssh)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(
        report.contains("profile 'app' jumps through 'BASTION', which is not a profile"),
        "{report}"
    );
    assert!(report.contains("lopi edit app --jump bastion"), "{report}");
}

#[test]
fn add_accepts_host_port_and_ssh_uri() {
    let env = Env::new();
    env.ok(&["add", "web", "example.com:2222"]);
    assert_eq!(env.ok(&["web"]), ["-p", "2222", "--", "example.com"]);
    env.ok(&["add", "u", "ssh://root@h:2200"]);
    assert_eq!(env.ok(&["u"]), ["-p", "2200", "--", "root@h"]);
    env.ok(&["add", "v6", "root@[2001:db8::1]:2200"]);
    assert_eq!(env.ok(&["v6"]), ["-p", "2200", "--", "root@2001:db8::1"]);
    env.ok(&["add", "v6b", "2001:db8::1"]);
    assert_eq!(env.ok(&["v6b"]), ["--", "2001:db8::1"]);
    // the same port twice is fine
    env.ok(&["add", "same", "h:22", "-p", "22"]);

    let err = env.fail(&["add", "x", "h:22", "-p", "2200"]);
    assert!(err.contains("port given twice"), "{err}");
    let err = env.fail(&["add", "x", "h:abc"]);
    assert!(err.contains("give the port with -p"), "{err}");
    let err = env.fail(&["edit", "web", "--host", "example.com:22"]);
    assert!(err.contains("use --host example.com --port 22"), "{err}");
    env.ok(&["edit", "v6b", "--host", "[2001:db8::2]"]);
    assert_eq!(env.ok(&["v6b"]), ["--", "2001:db8::2"]);
}

#[test]
fn doctor_reports_a_port_inside_the_host() {
    let env = Env::new();
    env.write_config(
        "[profiles.web]\nhost = \"example.com:2222\"\nuser = \"root\"\n\n\
         [profiles.u]\nhost = \"h:22\"\nuser = \"ssh://admin\"\n\n\
         [profiles.ok]\nhost = \"2001:db8::1\"\n",
    );
    let ssh = fake_ssh_with_version(&env);
    let out = env
        .cmd()
        .arg("doctor")
        .env("LOPI_SSH_BIN", &ssh)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(
        report.contains("profile 'web' has more than a host name in its address"),
        "{report}"
    );
    assert!(
        report.contains("`lopi edit web --host example.com --port 2222`"),
        "{report}"
    );
    assert!(
        report.contains("`lopi edit u --host h --user admin --port 22`"),
        "{report}"
    );
    assert!(!report.contains("profile 'ok'"), "{report}");
}

#[test]
fn ipv6_jump_hosts_are_bracketed() {
    let env = Env::new();
    env.ok(&["add", "v6", "admin@2001:db8::1", "-p", "2200"]);
    env.ok(&["add", "inner", "10.0.0.9", "-J", "v6"]);
    assert_eq!(
        env.ok(&["inner"]),
        ["-J", "admin@[2001:db8::1]:2200", "--", "10.0.0.9"]
    );
    let list = env.ok(&["list"]);
    assert!(
        list.iter()
            .any(|row| row.starts_with("v6 ") && row.contains("admin@[2001:db8::1]:2200")),
        "{list:?}"
    );

    let err = env.fail(&["add", "x", "h", "-J", "2001:db8::1"]);
    assert!(err.contains("write IPv6 addresses in brackets"), "{err}");
    let err = env.fail(&["edit", "inner", "--jump", "root@::1"]);
    assert!(err.contains("jump 'root@::1'"), "{err}");
    env.ok(&["add", "y", "h", "-J", "root@[2001:db8::2]:22"]);
    assert_eq!(env.ok(&["y"])[..2], ["-J", "root@[2001:db8::2]:22"]);
}

#[test]
fn ping_reports_whether_the_server_answers() {
    let env = Env::new();
    env.ok(&[
        "add",
        "office",
        "admin@10.0.0.5",
        "-p",
        "2222",
        "-f",
        "D:1080",
    ]);
    env.ok(&["add", "zayd", "admin@zayd.example.com"]);
    env.ok(&["add", "db", "10.0.0.9", "-J", "zayd"]);
    let ping = |name: &str, code: i32, stderr: &str| {
        let out = env
            .cmd()
            .args(["ping", name])
            .env("FAKE_SSH_EXIT", "255")
            .env("FAKE_SSH_STDERR", stderr)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(code), "{out:?}");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };

    let (stdout, stderr) = ping(
        "office",
        0,
        "admin@10.0.0.5: Permission denied (publickey,password).",
    );
    assert!(
        stdout.starts_with("office: ok, the ssh server answered in "),
        "{stdout}"
    );
    assert!(stdout.contains("(login: publickey,password)"), "{stdout}");
    assert!(stderr.is_empty(), "{stderr}");

    let (stdout, _) = ping("office", 0, "Host key verification failed.");
    assert!(
        stdout.contains("connect once with `lopi office`"),
        "{stdout}"
    );

    let (stdout, stderr) = ping(
        "db",
        1,
        "ssh: connect to host zayd.example.com port 22: Connection timed out",
    );
    assert_eq!(
        stdout.trim(),
        "db: FAIL, connection timed out (via admin@zayd.example.com)"
    );
    assert!(stderr.contains("  ssh: ssh: connect to host"), "{stderr}");

    let err = env.fail(&["ping", "Office"]);
    assert!(err.contains("did you mean 'office'"), "{err}");
    let err = env.fail(&["ping"]);
    assert!(err.contains("use lopi ping <name>"), "{err}");
}

#[test]
fn rename_updates_jump_references() {
    let env = Env::new();
    env.write_config(
        "[profiles.zayd]\nhost = \"zayd.example.com\"\nuser = \"admin\"\n\n\
         # through the bastion\n[profiles.db]\nhost = \"10.0.0.9\"\njump = \"zayd\" # keep me\n\n\
         [profiles.app]\nhost = \"10.0.0.10\"\njump = \"gw,zayd,root@zayd\"\n",
    );
    let out = env.ok(&["edit", "zayd", "--rename", "bastion"]);
    assert_eq!(
        out,
        [
            "updated 'zayd', now named 'bastion'",
            "updated the jump of 'app' to use 'bastion'",
            "updated the jump of 'db' to use 'bastion'",
        ]
    );
    assert_eq!(
        env.ok(&["db"]),
        ["-J", "admin@zayd.example.com", "--", "10.0.0.9"]
    );
    // a host name that only looks like the old name stays a host name
    assert_eq!(env.ok(&["app"])[1], "gw,admin@zayd.example.com,root@zayd");
    let text = fs::read_to_string(&env.config).unwrap();
    assert!(text.contains("# through the bastion"), "{text}");
    assert!(text.contains("jump = \"bastion\" # keep me"), "{text}");
}

#[test]
fn rm_refuses_a_profile_used_as_jump() {
    let env = Env::new();
    env.ok(&["add", "zayd", "admin@zayd.example.com"]);
    env.ok(&["add", "db", "10.0.0.9", "-J", "zayd"]);
    env.ok(&["add", "app", "10.0.0.10", "-J", "gw,zayd"]);
    for args in [&["rm", "zayd"][..], &["rm", "zayd", "-y"]] {
        let err = env.fail(args);
        assert!(err.contains("'zayd' is the jump host of: app, db"), "{err}");
        assert!(err.contains("lopi edit app --jump"), "{err}");
        assert!(err.contains("nothing was removed"), "{err}");
    }
    assert!(env.ok(&["list"]).iter().any(|row| row.starts_with("zayd ")));

    env.ok(&["edit", "db", "--jump", ""]);
    env.ok(&["edit", "app", "--jump", "gw"]);
    env.ok(&["rm", "zayd", "-y"]);
}

#[test]
fn new_profile_named_like_a_jump_host_warns() {
    let env = Env::new();
    env.ok(&["add", "app", "10.0.0.9", "-J", "bastion"]);
    let warning = "'app' jumps through 'bastion', which now means this profile";
    let out = env
        .cmd()
        .args(["add", "bastion", "admin@b.example"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&out.stderr).contains(warning));
    assert_eq!(env.ok(&["app"])[..2], ["-J", "admin@b.example"]);

    // renaming another profile to a name used as a jump host warns too
    env.ok(&["add", "web", "10.0.0.10", "-J", "gw"]);
    env.ok(&["add", "other", "gw.example"]);
    let out = env
        .cmd()
        .args(["edit", "other", "--rename", "gw"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("'web' jumps through 'gw', which now means this profile"),
        "{stderr}"
    );
}

#[test]
fn no_terminal_in_git_bash_explains_winpty() {
    let env = Env::new();
    let out = env
        .cmd()
        .env("MSYSTEM", "MINGW64")
        .assert()
        .code(2)
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stderr.contains("winpty"), cfg!(windows), "{stderr}");

    let out = env
        .cmd()
        .env_remove("MSYSTEM")
        .env_remove("TERM_PROGRAM")
        .assert()
        .code(2)
        .get_output()
        .clone();
    assert!(!String::from_utf8_lossy(&out.stderr).contains("winpty"));
}

#[test]
fn missing_arguments_without_a_terminal_say_what_to_pass() {
    let env = Env::new();
    env.ok(&["add", "vps", "h"]);
    let cases: [(&[&str], &str); 4] = [
        (&["add"], "use lopi add <name> [user@]host"),
        (&["add", "db"], "use lopi add <name> [user@]host"),
        (&["rm"], "use lopi rm <name> [-y]"),
        (&["edit"], "use lopi edit <name>"),
    ];
    for (args, expected) in cases {
        let err = env.fail(args);
        assert!(err.contains("not a terminal"), "{args:?}: {err}");
        assert!(err.contains(expected), "{args:?}: {err}");
    }
    // nothing was written
    assert_eq!(env.ok(&["list"]).len(), 2);
}

#[test]
fn hidden_complete_prints_names_quietly() {
    let env = Env::new();
    env.ok(&["add", "kantor", "h"]);
    env.ok(&["add", "vps", "h"]);
    env.ok(&["vps"]); // most recent first
    let out = env
        .cmd()
        .arg("__complete")
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(lines(&out.stdout), ["vps", "kantor"]);
    assert!(out.stderr.is_empty());

    // broken or reserved-name files: still exit 0 and no warnings
    env.write_config("[profiles.list]\nhost = \"h\"\n");
    let out = env
        .cmd()
        .arg("__complete")
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(out.stderr.is_empty());
    env.write_config("not = = toml");
    let out = env
        .cmd()
        .arg("__complete")
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(out.stdout.is_empty() && out.stderr.is_empty());

    let help = env.ok(&["--help"]).join("\n");
    assert!(!help.contains("__complete"), "{help}");
}

#[test]
fn completion_scripts_are_printed() {
    let env = Env::new();
    let cases = [
        ("bash", "complete -o bashdefault -o default -F _lopi"),
        ("zsh", "compdef _lopi lopi"),
        ("powershell", "Register-ArgumentCompleter -Native"),
    ];
    for (shell, expected) in cases {
        let script = env.ok(&["completion", shell]).join("\n");
        assert!(script.contains(expected), "{shell}");
    }
    env.cmd().args(["completion", "cmd"]).assert().code(2);
}

/// Runs the real bash completion function: profile names after the commands that take an
/// existing profile, never after `add` (a new name) or `list` (no name).
#[cfg(unix)]
#[test]
fn bash_completes_profile_names_where_a_profile_is_expected() {
    let env = Env::new();
    env.ok(&["add", "kantor", "h"]);
    env.ok(&["add", "vps", "h"]);
    let bin_dir = std::path::Path::new(env!("CARGO_BIN_EXE_lopi"))
        .parent()
        .unwrap()
        .to_path_buf();
    let path = std::env::join_paths(
        std::iter::once(bin_dir).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let complete = |words: &str, cword: usize| -> Vec<String> {
        let script = format!(
            "eval \"$(lopi completion bash)\"; COMP_WORDS=({words}); COMP_CWORD={cword}; \
             _lopi; printf '%s\\n' \"${{COMPREPLY[@]}}\""
        );
        let out = std::process::Command::new("bash")
            .args(["-c", &script])
            .env("PATH", &path)
            .env("LOPI_CONFIG", &env.config)
            .env("LOPI_DATA_DIR", env.dir.path().join("data"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        lines(&out.stdout)
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect()
    };

    for sub in ["rm", "edit", "connect", "passwd", "ping"] {
        let got = complete(&format!("lopi {sub} \"\""), 2);
        assert!(
            got.contains(&"kantor".to_string()) && got.contains(&"vps".to_string()),
            "{sub}: {got:?}"
        );
    }
    for sub in ["add", "list"] {
        let got = complete(&format!("lopi {sub} \"\""), 2);
        assert!(!got.contains(&"kantor".to_string()), "{sub}: {got:?}");
    }
    assert_eq!(complete("lopi ka", 1), ["kantor"]);
}

/// Unix only: on Windows these commands change the real user PATH (registry) and
/// PowerShell profiles, which tests must never touch. Windows is verified by hand.
#[cfg(unix)]
#[test]
fn install_and_uninstall_into_a_custom_dir() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let bin = env.dir.path().join("bin");
    let installed = bin.join("lopi");
    let run = |args: &[&str]| {
        let out = env
            .cmd()
            .env("LOPI_INSTALL_DIR", &bin)
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        lines(&out.stdout).join("\n")
    };

    let out = run(&["install"]);
    assert!(out.contains("installed lopi"), "{out}");
    assert!(out.contains("is not on your PATH"), "{out}");
    let mode = fs::metadata(&installed).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);

    assert!(run(&["install"]).contains("updated"));
    assert!(run(&["uninstall", "-y"]).contains("deleted"));
    assert!(!installed.exists());
    assert!(bin.exists(), "a shared bin folder is never removed");
    assert!(run(&["uninstall", "-y"]).contains("not installed"));
}

#[test]
fn install_names_are_reserved() {
    let env = Env::new();
    assert!(env.fail(&["add", "install", "h"]).contains("reserved"));
    assert!(env.fail(&["add", "Uninstall", "h"]).contains("reserved"));
    assert!(env.fail(&["add", "update", "h"]).contains("reserved"));
}

/// Only reads the credential store (an id that cannot exist), never writes to it.
#[test]
fn password_profile_without_saved_password_lets_ssh_ask() {
    let env = Env::new();
    env.write_config("[profiles.vps]\nhost = \"h\"\nauth = \"password\"\n");
    let out = env
        .cmd()
        .arg("vps")
        .env("LOPI_KEYRING_SERVICE", "lopi-test-never-written")
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(lines(&out.stdout), ["--", "h"], "no askpass option added");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ssh will ask") || stderr.contains("no saved password"),
        "{stderr}"
    );
    // With a usable credential store (none on Linux CI), connecting gave the hand-written
    // profile an id, needed to save a password later. Without one, the file is left alone.
    if stderr.contains("no saved password") {
        assert!(fs::read_to_string(&env.config).unwrap().contains("id = "));
    }
}

#[test]
fn passwords_need_a_terminal_and_are_never_arguments() {
    let env = Env::new();
    env.ok(&["add", "vps", "h"]);
    for args in [
        &["add", "db", "h", "--password"][..],
        &["passwd", "vps"][..],
        &["edit", "vps", "--auth", "password"][..],
    ] {
        let out = env
            .cmd()
            .args(args)
            .env("LOPI_KEYRING_SERVICE", "lopi-test-never-written")
            .assert()
            .failure()
            .get_output()
            .clone();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("never passed as arguments")
                || stderr.contains("cannot save passwords"),
            "{args:?}: {stderr}"
        );
    }
    // `--password` takes no value
    env.cmd()
        .args(["add", "db", "h", "--password=s3cret"])
        .assert()
        .code(2);
    assert_eq!(env.ok(&["list"]).len(), 2, "nothing was added");
}

#[test]
fn list_shows_how_each_profile_logs_in() {
    let env = Env::new();
    env.write_config(
        "[profiles.a]\nhost = \"h\"\nauth = \"password\"\n[profiles.b]\nhost = \"h\"\nkey = \"/k\"\n[profiles.c]\nhost = \"h\"\n",
    );
    let list = env.ok(&["list"]);
    let auth: Vec<&str> = list[1..]
        .iter()
        .map(|line| line.split_whitespace().nth(2).unwrap_or(""))
        .collect();
    assert_eq!(auth, ["password", "/k", "agent"]);
    assert!(list[2].contains("key"));
}

#[test]
fn passwd_remove_switches_back_without_touching_other_fields() {
    let env = Env::new();
    env.write_config("[profiles.a]\nhost = \"h\"\nauth = \"password\"\nport = 2222\n");
    // Without a credential store (Linux CI) deleting fails; auth must be off either way.
    let _ = env
        .cmd()
        .args(["passwd", "a", "--remove"])
        .env("LOPI_KEYRING_SERVICE", "lopi-test-never-written")
        .output()
        .unwrap();
    let text = fs::read_to_string(&env.config).unwrap();
    assert!(!text.contains("auth"), "{text}");
    assert!(text.contains("port = 2222"), "{text}");
}

#[test]
fn passwd_show_refuses_pipes_and_changes_nothing() {
    let env = Env::new();
    let config = "[profiles.vps]\nhost = \"h\"\nauth = \"password\"\n";
    env.write_config(config);
    // Tests run without a terminal, like `lopi passwd vps --show | cat`.
    let out = env
        .cmd()
        .args(["passwd", "vps", "--show"])
        .env("LOPI_KEYRING_SERVICE", "lopi-test-never-written")
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("shown only on a terminal"), "{stderr}");
    assert!(out.stdout.is_empty());
    // The same without a name: refused before the picker.
    let stderr = env.fail(&["passwd", "--show"]);
    assert!(stderr.contains("shown only on a terminal"), "{stderr}");

    env.cmd()
        .args(["passwd", "vps", "--show", "--remove"])
        .assert()
        .code(2);
    assert_eq!(fs::read_to_string(&env.config).unwrap(), config);
}

/// Real credential store, throw-away service name, cleaned up. Run by hand:
/// `cargo test --test cli askpass_real -- --ignored`
#[test]
#[ignore]
fn askpass_real_store_answers_only_its_own_server() {
    /// Deletes the test credential even when an assertion fails.
    struct Cleanup(keyring::Entry);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.delete_credential();
        }
    }

    let service = format!("lopi-test-{}", std::process::id());
    let entry = keyring::Entry::new(&service, "id-e2e").unwrap();
    entry.set_password("s3cret pw").unwrap();
    let _cleanup = Cleanup(entry);

    // Any prompt that is not answered from the store goes to the terminal and would wait
    // for typing forever; the timeout kills it, which yields no stdout.
    let askpass = |prompt: &str| -> String {
        Command::cargo_bin("lopi")
            .unwrap()
            .arg(prompt)
            .env("LOPI_KEYRING_SERVICE", &service)
            .env("LOPI_ASKPASS_ID", "id-e2e")
            .env("LOPI_ASKPASS_TARGET", "root@10.0.0.5")
            .timeout(std::time::Duration::from_secs(10))
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default()
    };

    assert_eq!(askpass("root@10.0.0.5's password: "), "s3cret pw\n");
    // A jump host's prompt must not get it.
    assert!(!askpass("admin@bastion's password: ").contains("s3cret"));
}

/// A fake ssh that answers `-V` like OpenSSH 9.9.
fn fake_ssh_with_version(env: &Env) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = env.dir.path().join("ssh-version");
        fs::write(&path, "#!/bin/sh\necho 'OpenSSH_9.9p1, OpenSSL 3.0' >&2\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[cfg(windows)]
    {
        let path = env.dir.path().join("ssh-version.cmd");
        fs::write(&path, "@echo OpenSSH_9.9p1, OpenSSL 3.0 1>&2\r\n").unwrap();
        path
    }
}

#[test]
fn doctor_reports_and_fails_on_a_broken_file() {
    let env = Env::new();
    let ssh = fake_ssh_with_version(&env);
    env.write_config("[profiles.vps]\nhost = \"h\"\nkey = \"/nonexistent/id_vps\"\n");
    let out = env
        .cmd()
        .arg("doctor")
        .env("LOPI_SSH_BIN", &ssh)
        .assert()
        .success()
        .get_output()
        .clone();
    let text = lines(&out.stdout).join("\n");
    assert!(
        text.contains("profiles: ") && text.contains("(1 profile)"),
        "{text}"
    );
    assert!(text.contains("(OpenSSH_9.9p1)"), "{text}");
    assert!(text.contains("warn  key for 'vps' not found"), "{text}");

    env.write_config("broken = = file");
    let out = env
        .cmd()
        .arg("doctor")
        .env("LOPI_SSH_BIN", &ssh)
        .assert()
        .code(1)
        .get_output()
        .clone();
    let text = lines(&out.stdout).join("\n");
    assert!(
        text.contains("FAIL") && text.contains("lopi restore"),
        "{text}"
    );

    let out = env
        .cmd()
        .arg("doctor")
        .env("LOPI_SSH_BIN", "lopi-no-such-ssh")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(
        lines(&out.stdout)
            .join("\n")
            .contains("FAIL  ssh not found")
    );
}

#[test]
fn backup_needs_a_terminal_for_the_passphrase() {
    let env = Env::new();
    env.ok(&["add", "vps", "h"]);
    let target = env.dir.path().join("out.age");
    let err = env.fail(&["backup", target.to_str().unwrap()]);
    assert!(err.contains("never passed as an argument"), "{err}");
    assert!(!target.exists());
}

#[test]
fn restore_from_a_snapshot_or_profiles_file() {
    let env = Env::new();
    env.ok(&["add", "a", "h"]);
    env.ok(&["add", "b", "h"]); // snapshot taken: profiles with only `a`
    let snapshots: Vec<_> = fs::read_dir(env.dir.path().join("data").join("backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(snapshots.len(), 1);

    // without -y there is nobody to confirm
    let err = env.fail(&["restore", snapshots[0].to_str().unwrap()]);
    assert!(err.contains("use -y"), "{err}");

    let out = env.ok(&["restore", snapshots[0].to_str().unwrap(), "-y"]);
    assert!(
        out.iter().any(|l| l.contains("restored 1 profile")),
        "{out:?}"
    );
    assert_eq!(env.ok(&["list"]).len(), 2, "back to just `a`");

    // the state before the restore became a snapshot, so it can be restored in turn
    let count = fs::read_dir(env.dir.path().join("data").join("backups"))
        .unwrap()
        .count();
    assert_eq!(count, 2);

    // a hand-made profiles file works too; a broken one is refused and changes nothing
    let plain = env.dir.path().join("mine.toml");
    fs::write(&plain, "# mine\n[profiles.x]\nhost = \"x\"\n").unwrap();
    env.ok(&["restore", plain.to_str().unwrap(), "-y"]);
    assert!(
        fs::read_to_string(&env.config)
            .unwrap()
            .starts_with("# mine\n")
    );
    fs::write(&plain, "broken = = toml").unwrap();
    assert!(
        env.fail(&["restore", plain.to_str().unwrap(), "-y"])
            .contains("valid profiles")
    );
    assert!(
        fs::read_to_string(&env.config)
            .unwrap()
            .starts_with("# mine\n")
    );
}

#[test]
fn restore_without_a_file_needs_a_terminal_to_choose() {
    let env = Env::new();
    let err = env.fail(&["restore"]);
    assert!(err.contains("use lopi restore <file>"), "{err}");
}

/// The mix-up that happened for real: `rm` deletes the saved password, and a snapshot
/// holds profiles only. Restore must say so and point at the fix.
#[test]
fn restoring_a_snapshot_explains_missing_passwords() {
    let env = Env::new();
    let snapshot = env.dir.path().join("snapshot.toml");
    fs::write(
        &snapshot,
        "[profiles.dev]\nhost = \"h\"\nauth = \"password\"\nid = \"lopi-test-none\"\n",
    )
    .unwrap();
    let out = env
        .cmd()
        .args(["restore", snapshot.to_str().unwrap(), "-y"])
        .env("LOPI_KEYRING_SERVICE", "lopi-test-never-written")
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = lines(&out.stdout).join("\n");
    assert!(stdout.contains("profiles only"), "{stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Linux CI has no credential store; then it says it cannot check.
    assert!(
        (stderr.contains("'dev' uses a password but none is saved")
            && stderr.contains("lopi passwd dev")
            && stderr.contains("lopi restore <file>.age"))
            || stderr.contains("cannot check saved passwords"),
        "{stderr}"
    );
}

// lopi update: a local web server stands in for GitHub.

const NEWER: &str = "99.0.0";

/// Answers GET requests for `files` (path → body), 404 for anything else, until the test
/// process ends. Returns the base URL for `LOPI_UPDATE_URL`.
fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            let _ = reader.read_line(&mut request);
            let mut header = String::new();
            while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                header.clear();
            }
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let (status, body) = match files.iter().find(|(file, _)| file == path) {
                Some((_, body)) => ("200 OK", body.as_slice()),
                None => ("404 Not Found", &[][..]),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(body);
        }
    });
    format!("http://{address}/releases")
}

fn archive_name() -> String {
    let target = lopi::update::RELEASE_TARGET.unwrap_or("no-target");
    format!("lopi-ssh-{target}{}", lopi::update::ARCHIVE_SUFFIX)
}

/// The files of release `version` as GitHub serves them: the manifest under `latest`, and
/// with `archive`, the archive and its checksum file (`checksum`, or the real one).
fn release(
    version: &str,
    archive: Option<&[u8]>,
    checksum: Option<&str>,
) -> Vec<(String, Vec<u8>)> {
    let target = lopi::update::RELEASE_TARGET.unwrap_or("no-target");
    let name = archive_name();
    let manifest = format!(
        r#"{{"releases":[{{"app_name":"lopi-ssh","app_version":"{version}","artifacts":["{name}","{name}.sha256"]}}],
           "artifacts":{{"{name}":{{"kind":"executable-zip","target_triples":["{target}"],"checksum":"{name}.sha256"}}}}}}"#
    );
    let mut files = vec![(
        "/releases/latest/download/dist-manifest.json".to_string(),
        manifest.into_bytes(),
    )];
    if let Some(archive) = archive {
        let sum = checksum
            .map(str::to_string)
            .unwrap_or_else(|| lopi::update::checksum::sha256_hex(archive));
        let tag = format!("/releases/download/v{version}");
        files.push((format!("{tag}/{name}"), archive.to_vec()));
        files.push((
            format!("{tag}/{name}.sha256"),
            format!("{sum} *{name}\n").into_bytes(),
        ));
    }
    files
}

/// A release archive whose `lopi` is a script printing `lopi <says>`.
#[cfg(unix)]
fn fake_lopi_archive(says: &str) -> Vec<u8> {
    let script = format!("#!/bin/sh\necho \"lopi {says}\"\n");
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_data(&mut header, "lopi-ssh-test/lopi", script.as_bytes())
        .unwrap();
    let tar = builder.into_inner().unwrap();
    let mut xz = Vec::new();
    lzma_rs::xz_compress(&mut std::io::Cursor::new(tar), &mut xz).unwrap();
    xz
}

impl Env {
    /// Everything `lopi update` looks at, moved into the test folder, so no real receipt,
    /// cargo record or install is ever seen.
    fn update_env(&self, cmd: &mut Command, url: &str) {
        let dir = self.dir.path();
        cmd.env("LOPI_UPDATE_URL", url)
            .env("XDG_CONFIG_HOME", dir.join("xdg"))
            .env("CARGO_HOME", dir.join("cargo"))
            .env("LOPI_INSTALL_DIR", dir.join("bin"));
    }

    /// `lopi update ...` with the binary cargo built.
    fn update(&self, url: &str, args: &[&str]) -> Command {
        let mut cmd = self.cmd();
        self.update_env(&mut cmd, url);
        cmd.arg("update").args(args);
        cmd
    }

    /// Copies the built binary to `<test folder>/<folder>/lopi`.
    #[cfg(unix)]
    fn copy_of_lopi(&self, folder: &str) -> PathBuf {
        let dir = self.dir.path().join(folder);
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("lopi");
        fs::copy(env!("CARGO_BIN_EXE_lopi"), &exe).unwrap();
        exe
    }

    /// `lopi update ...` run by the copy at `exe`.
    #[cfg(unix)]
    fn update_with(&self, exe: &std::path::Path, url: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(exe);
        cmd.env("LOPI_CONFIG", &self.config)
            .env("LOPI_SSH_BIN", &self.ssh)
            .env("LOPI_DATA_DIR", self.dir.path().join("data"));
        self.update_env(&mut cmd, url);
        cmd.arg("update").args(args);
        cmd
    }
}

fn stdout_of(assert: assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stdout).into_owned()
}

fn stderr_of(assert: assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).into_owned()
}

#[test]
fn update_check_reports_versions() {
    let env = Env::new();
    let current = env!("CARGO_PKG_VERSION");

    let url = serve(release(current, None, None));
    let out = stdout_of(env.update(&url, &["--check"]).assert().success());
    assert!(
        out.contains(&format!("lopi {current} is up to date")),
        "{out}"
    );

    let url = serve(release("0.0.1", None, None));
    let out = stdout_of(env.update(&url, &["--check"]).assert().success());
    assert!(
        out.contains("newer than the latest release (0.0.1)"),
        "{out}"
    );

    let url = serve(release(NEWER, None, None));
    let out = stdout_of(env.update(&url, &["--check"]).assert().code(1));
    assert!(
        out.contains(&format!("lopi {NEWER} is available (you have {current})")),
        "{out}"
    );
    assert!(out.contains(&format!("{url}/tag/v{NEWER}")), "{out}");
    // the test binary runs from cargo's build folder
    assert!(out.contains("to update, download it from"), "{out}");
}

#[test]
fn update_failures_are_explained() {
    let env = Env::new();
    let url = serve(Vec::new());
    let err = stderr_of(env.update(&url, &["--check"]).assert().failure());
    assert!(err.contains("no published release found"), "{err}");

    let err = stderr_of(
        env.update("http://example.com/r", &["--check"])
            .assert()
            .failure(),
    );
    assert!(err.contains("https://"), "{err}");

    let err = stderr_of(env.update(&url, &["--check", "-y"]).assert().code(2));
    assert!(err.contains("cannot be used with"), "{err}");
}

#[test]
fn update_leaves_a_build_folder_alone() {
    let env = Env::new();
    let url = serve(release(NEWER, Some(b"unused"), None));
    let err = stderr_of(env.update(&url, &["-y"]).assert().failure());
    assert!(
        err.contains("only replaces a lopi put in place by"),
        "{err}"
    );
    assert!(err.contains("download it from"), "{err}");
}

#[cfg(unix)]
#[test]
fn update_replaces_a_lopi_install_copy() {
    let env = Env::new();
    let exe = env.copy_of_lopi("bin");
    let url = serve(release(NEWER, Some(&fake_lopi_archive(NEWER)), None));

    // no terminal to confirm
    let err = stderr_of(env.update_with(&exe, &url, &[]).assert().failure());
    assert!(err.contains("-y to update without confirmation"), "{err}");

    let out = stdout_of(env.update_with(&exe, &url, &["-y"]).assert().success());
    // the path with symlinks resolved (macOS: /var is /private/var)
    let shown = fs::canonicalize(&exe).unwrap();
    assert!(
        out.contains(&format!("updated {} to lopi {NEWER}", shown.display())),
        "{out}"
    );
    let version = Command::new(&exe).arg("--version").assert().success();
    assert_eq!(stdout_of(version).trim(), format!("lopi {NEWER}"));
    let names: Vec<_> = fs::read_dir(exe.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["lopi"], "no temporary or .old files left");
}

#[cfg(unix)]
#[test]
fn update_replaces_an_install_script_copy_and_its_receipt() {
    let env = Env::new();
    let exe = env.copy_of_lopi("script/bin");
    let receipt = env.dir.path().join("xdg/lopi-ssh/lopi-ssh-receipt.json");
    fs::create_dir_all(receipt.parent().unwrap()).unwrap();
    let prefix = env.dir.path().join("script");
    fs::write(
        &receipt,
        format!(
            r#"{{"binaries":["lopi"],"install_layout":"cargo-home","install_prefix":"{}","modify_path":true,"source":{{"app_name":"lopi-ssh","name":"lopi","owner":"hmrnsp","release_type":"github"}},"version":"{}"}}"#,
            prefix.display(),
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    let url = serve(release(NEWER, Some(&fake_lopi_archive(NEWER)), None));

    let out = stdout_of(env.update_with(&exe, &url, &["--check"]).assert().code(1));
    assert!(out.contains("to update, run `lopi update`"), "{out}");
    env.update_with(&exe, &url, &["-y"]).assert().success();
    let version = Command::new(&exe).arg("--version").assert().success();
    assert_eq!(stdout_of(version).trim(), format!("lopi {NEWER}"));
    let text = fs::read_to_string(&receipt).unwrap();
    assert!(text.contains(&format!("\"version\":\"{NEWER}\"")), "{text}");
    assert!(text.contains("\"modify_path\":true"), "{text}");
}

#[cfg(unix)]
#[test]
fn update_leaves_a_cargo_install_to_cargo() {
    let env = Env::new();
    let exe = env.copy_of_lopi("cargo/bin");
    fs::write(
        env.dir.path().join("cargo/.crates2.json"),
        r#"{"installs":{"lopi-ssh 0.4.0 (registry+https://github.com/rust-lang/crates.io-index)":{"bins":["lopi"]}}}"#,
    )
    .unwrap();
    let before = fs::read(&exe).unwrap();
    let url = serve(release(NEWER, Some(&fake_lopi_archive(NEWER)), None));

    let out = stdout_of(env.update_with(&exe, &url, &["--check"]).assert().code(1));
    assert!(out.contains("cargo install lopi-ssh --locked"), "{out}");
    let err = stderr_of(env.update_with(&exe, &url, &["-y"]).assert().failure());
    assert!(err.contains("installed with cargo"), "{err}");
    assert_eq!(fs::read(&exe).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn update_changes_nothing_when_a_check_fails() {
    let env = Env::new();
    let exe = env.copy_of_lopi("bin");
    let before = fs::read(&exe).unwrap();
    let unchanged = || {
        assert_eq!(fs::read(&exe).unwrap(), before);
        let names: Vec<_> = fs::read_dir(exe.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["lopi"], "no temporary files left");
    };

    let wrong_sum = "0".repeat(64);
    let url = serve(release(
        NEWER,
        Some(&fake_lopi_archive(NEWER)),
        Some(&wrong_sum),
    ));
    let err = stderr_of(env.update_with(&exe, &url, &["-y"]).assert().failure());
    assert!(err.contains("is not the file that was published"), "{err}");
    unchanged();

    let url = serve(release(NEWER, Some(&fake_lopi_archive("98.0.0")), None));
    let err = stderr_of(env.update_with(&exe, &url, &["-y"]).assert().failure());
    assert!(
        err.contains("says 'lopi 98.0.0' instead of 'lopi 99.0.0'"),
        "{err}"
    );
    assert!(err.contains("nothing was changed"), "{err}");
    unchanged();

    let url = serve(release(NEWER, Some(b"not an archive"), None));
    let err = stderr_of(env.update_with(&exe, &url, &["-y"]).assert().failure());
    assert!(err.contains("not a valid .tar.xz"), "{err}");
    unchanged();
}
