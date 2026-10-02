# Contributing to lopi

Thanks for helping. Bug reports, fixes, documentation and ideas are all welcome.
For how the code fits together, read [ARCHITECTURE.md](ARCHITECTURE.md) first.

## Reporting a bug

Open an issue with the bug report form and include:

- what you ran, what you expected and what happened instead
- `lopi --version` and the output of `lopi doctor`
- your OS and version, and the shell (bash, zsh, PowerShell, cmd, Git Bash)
- `ssh -V`

Replace real host names, IP addresses and user names with placeholders such as
`server.example.com` and `admin`. Never paste passwords, private keys or backup files.

Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Setup

You need Rust 1.89 or newer (`rust-version` in `Cargo.toml`) and, to try connections, the
OpenSSH client (`ssh`).

```sh
git clone https://github.com/hmrnsp/lopi
cd lopi
cargo build
cargo run -- list
```

A development build uses your real profiles file unless told otherwise. To keep your own
data out of the way, point it at a scratch folder:

```sh
export LOPI_CONFIG=/tmp/lopi-dev/profiles.toml
export LOPI_DATA_DIR=/tmp/lopi-dev/data
cargo run -- add test admin@server.example.com
```

Useful commands:

```sh
cargo test                     # all tests
cargo test <substring>         # tests whose name matches
cargo test --test cli          # only the integration tests in tests/cli.rs
cargo fmt
cargo clippy --all-targets -- -D warnings
```

## What CI checks

Every pull request must pass:

- `cargo fmt --check`
- `cargo clippy --all-targets --locked -- -D warnings`, on Linux, macOS and Windows
- `cargo test --locked`, on Linux, macOS and Windows
- `cargo check --locked` with Rust 1.89 (the MSRV)
- the bash, zsh and PowerShell completion scripts parse (`bash -n`, `zsh -n`, PowerShell's parser)

Run the first three locally before you push.

## Testing

Unit tests live next to the code; `tests/cli.rs` runs the real binary with `assert_cmd`.
Its `Env` helper creates a temporary profiles file and a fake `ssh` (a shell script on Unix,
a `.cmd` file on Windows) that prints each argument on its own line, so tests check
exactly what lopi would pass to ssh.

Environment variables used by tests:

| Variable | Purpose |
| --- | --- |
| `LOPI_CONFIG` | Path of the profiles file |
| `LOPI_DATA_DIR` | Folder for snapshots and connection history. **Every test must set it**, or it writes into your real data folder |
| `LOPI_SSH_BIN` | The ssh program to run, for example the fake ssh |
| `LOPI_INSTALL_DIR` | Target folder of `lopi install` |
| `LOPI_KEYRING_SERVICE` | Credential store service name; tests use `lopi-test-…` |

`LOPI_ASKPASS_ID` and `LOPI_ASKPASS_TARGET` are set by lopi itself when it runs ssh with a
saved password; only the askpass tests set them directly.

Rules for tests:

- **Credential store.** Regular tests only read from a `lopi-test-…` service and never
  write, so they pass on CI machines without a credential store (Linux CI has no Secret
  Service; the macOS and Windows runners have one). Tests that use the real store are
  `#[ignore]` and clean up after themselves. Run them by hand on a desktop session:

  ```sh
  cargo test real_keyring -- --ignored
  cargo test --test cli askpass_real -- --ignored
  ```

- **Askpass needs a timeout.** Every askpass call in a test uses `.timeout(..)`: a prompt
  that is not answered from the store reads the console and would wait forever.
- **Install tests are Unix-only.** On Windows, `install` changes the registry and the real
  PowerShell profile.
- **Interactive UI.** Tests run without a terminal. Wizards, confirmations and pickers are
  tested through `ui::prompt::scripted::Scripted`, a `Prompter` that replays answers. The
  full-screen table picker is split so it can be tested too: key handling through
  `PickerState::handle` (pure), drawing through `ratatui::backend::TestBackend`.

## Design rules

These hold everywhere; a change that breaks one needs a very good reason.

1. **One binary, no runtime**, on Windows, macOS and Linux.
2. **No SSH implementation of our own.** lopi always runs the system's `ssh`.
3. **One write path for the profiles file.** All writes go through `config/store.rs`:
   `mutate_in` (field changes) or `replace_in` (whole file, used by `restore`). Both end in
   the single `commit` function (lock, validate, snapshot, atomic write).
4. **No passwords in files, arguments, environment variables or logs.** Passwords live
   only in the OS credential store (`secrets.rs`, keyed by profile `id`) and inside
   encrypted backup files. In memory they are wrapped in `Zeroizing`. They never enter the
   clap structs (`AddArgs`, `EditArgs`); the wizard returns them separately (`AddPlan`,
   `EditPlan`). The store is checked before the user is asked to type a password.
5. **Never build a command string.** Pass arguments one by one with `Command::arg()`, as
   `OsString`/`PathBuf` rather than `String`, so Windows paths stay intact.
6. **Destructive commands take exact names only.** `rm` and `edit` accept a full profile
   name (any letter case), never a prefix. Prefixes are only for connecting.

## Edge cases to keep in mind

- **Reserved names.** Profile names that clash with subcommands are listed in
  `RESERVED_NAMES` (`src/config/model.rs`). Add every new subcommand there. Such names are
  rejected by `add`, cause a warning on load, and can still be reached with
  `lopi connect <name>`.
- **Name matching.** Exact match first, then a match ignoring letter case, then a unique
  prefix ignoring case (prefixes only when connecting). An ambiguous prefix lists the
  candidates and fails. Names that differ only in case are rejected
  (`Config::check_unique`).
- **Option injection.** Host, user and profile name must not start with `-`, and lopi
  always puts `--` before the destination. clap already rejects such values, but the
  checks in `config/model.rs` stay: values can arrive through `-- -x`, `--host=-x` or a
  hand-edited file.
- **Paths.** lopi expands `~` itself and stores key paths as `~/.ssh/...` so the file
  stays portable.
- **Key passphrases** are left to ssh: never pipe ssh's stdio and never force `BatchMode`.
- **Git Bash** ships its own `ssh`, which may differ from Windows' OpenSSH.
- **No ssh on PATH** must give a clear error that says how to install it.

## Pull requests

- Keep each pull request to one topic, and each commit to one logical change.
- Add an entry under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for anything a
  user would notice.
- Update [README.md](README.md) when commands, options or behavior change.
- Explain in the pull request why a new dependency is needed and what it costs (binary
  size, platforms, licenses).
- Shell completion templates in `src/completion/` must keep LF line endings
  (`.gitattributes` enforces this for bash and zsh; `completion::fill` also normalizes CRLF).
- Print large output (`list`, `completion`, `path`) with `output::print`, not `print!`, so a
  closed pipe (`lopi list | head -1`) is not a panic.
- Ask questions only when stdin and stderr are terminals. Without one, fail with a message
  that names the flag to use instead (`require_terminal` in `src/commands/mod.rs`).

## Releases (maintainers)

1. In `CHANGELOG.md`, move the `Unreleased` entries under a new `## [X.Y.Z] - YYYY-MM-DD`
   heading and update the links at the bottom.
2. Bump `version` in `Cargo.toml` and run `cargo build` to update `Cargo.lock`.
3. Commit, then push a `vX.Y.Z` tag. The release workflow (cargo-dist) builds the Windows,
   macOS and Linux archives and installers and creates the GitHub release.
4. `cargo publish` (the crate is `lopi-ssh`).

The release workflow is generated. To change it, edit `dist-workspace.toml` (or
`[profile.dist]` in `Cargo.toml`) and run `dist generate`; never edit
`.github/workflows/release.yml` by hand. `dist plan` shows what a release would build.

## License

By contributing, you agree that your contributions are dual licensed under the
[MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses, as described in the README,
without any additional terms or conditions.
