# Architecture

lopi turns a saved profile into an `ssh` command line and runs the system's `ssh`. It
never speaks the SSH protocol itself, so keys, the agent, `known_hosts` and
`~/.ssh/config` keep working as usual.

It is one crate: `src/lib.rs` holds all the code, and `src/main.rs` is a thin wrapper that
calls `app::run` and turns the result into an exit code. The library exists for the binary
and the tests; it has no stable API.

## Source map

```
src/
  main.rs            askpass check, then app::run → exit code; error::Abort (Esc = 1,
                     Ctrl+C = 130) is printed without "error:"
  app.rs             parses the command line (clap) and dispatches
  cli.rs             clap definitions; AddArgs/EditArgs are also what the wizard returns
  commands/          one file per command; complete.rs is the hidden `__complete`
  config/            model.rs (schema + validation), store.rs (load, write),
                     document.rs (minimal edits with toml_edit), meta.rs (id, updated_at),
                     backup.rs (snapshots), key_path.rs, paths.rs
  resolve.rs         name lookup: exact name only; suggestions for near misses
  ssh/               args.rs (build_args, pure), jump.rs, launch_unix.rs (exec),
                     launch_windows.rs (child process)
  ui/                prompt.rs (Prompter trait, Scripted for tests), picker.rs, wizard.rs,
                     table.rs, tty.rs; mod.rs: profile_table (shared by `list` and the picker)
  ui/table_picker/   full-screen table (ratatui): state.rs (keys, pure), view.rs (drawing),
                     mod.rs (terminal setup and event loop)
  completion/        bash/zsh/PowerShell templates (include_str!)
  state.rs           connection history (not part of the profiles file)
  lock.rs            cross-process lock on `<file>.lock`
  atomic.rs          write to a temp file, then rename (with retries on Windows)
  time.rs            UTC timestamps without a date crate
  output.rs          stdout writes that report a closed pipe instead of panicking
  askpass.rs         SSH_ASKPASS mode
  secrets.rs         SecretStore trait and KeyringStore (OS credential store)
  backup_bundle.rs   backup file format: tar inside age, pure functions
  install/           path_list.rs, profile_block.rs (pure); windows.rs (registry PATH);
                     powershell.rs (completion in $PROFILE); mod.rs (copy_exe, remove_exe)
```

## Connecting

1. `main` first calls `askpass::run_if_requested`. If ssh started lopi as its askpass
   program, that handles the prompt and exits (see [Passwords](#passwords-and-askpass)).
2. `app::run` parses the command line. A first word that is not a subcommand is caught by
   `#[command(external_subcommand)]` as a profile name; everything after it goes to ssh.
   `lopi connect <name> ...` takes the name and the arguments in one trailing
   `Vec<OsString>`: with a separate positional name, clap would swallow a `--` right after
   it.
3. `commands::connect::run` loads the profiles (`store::load`) and finds the profile with
   `resolve::resolve`: the exact name, in the same letter case. Anything else fails with
   suggestions: the name in another letter case, names that start with it, or likely typos
   (Damerau–Levenshtein via `strsim`). Nothing near is ever used.
4. `commands::connect::connect` validates the profile and replaces a `jump` that names
   another profile exactly with that profile's address (`ssh::jump::resolve_jump`, one
   level, not recursive). An item that matches a profile only in another letter case stays
   a host name, with a warning (`ssh::jump::jump_case_mismatches`).
5. `ssh::args::build_args(&Profile, extra, home)` builds the arguments: the user's extra
   options first (ssh keeps the first value of `-p` and `-o`, so they override the
   profile), then `-p`, `-i`, `-J`, `-L`/`-R`/`-D`, `--`, `user@host`, and the remote
   command. It is a pure function and most of its behavior is unit tested.
6. Profiles with `auth = "password"` also get the askpass environment (below).
7. `state::record_use` writes the history entry *before* launching, because on Unix the
   process is replaced. It only tries the lock (`FileLock::try_acquire`) and ignores
   failures: history must never block a connection.
8. `ssh::launch` runs ssh with stdio inherited, never piped, so ssh can ask for host key
   confirmation and passphrases itself. On Unix it `exec`s ssh. On Windows it starts ssh
   as a child, ignores Ctrl+C in lopi (`SetConsoleCtrlHandler`) so only ssh reacts, and
   exits with ssh's exit code (255 means the connection failed). A missing ssh gives an
   error that says how to install it.

The ssh program is `LOPI_SSH_BIN` if set, else `ssh_bin` from the profiles file, else
`ssh` from `PATH` (`ssh::ssh_bin`).

## Writing the profiles file

All writes go through `config/store.rs`, and both entry points end in one private
function, `commit`.

`mutate_in(store, change)`, for edits:

1. lock `<profiles file>.lock` (waits up to `lock::DEFAULT_TIMEOUT`)
2. read the text and parse it twice: into `Config` (serde) and into a `toml_edit`
   `DocumentMut`
3. `meta::ensure_ids` gives every profile an `id`, also written into the document
4. run `change` on a copy of the config
5. `meta::stamp` gives new profiles an id and changed profiles a new `updated_at`;
   `validate()` must pass
6. `document::apply` edits only the keys that changed, matching profiles by `id` (so a
   rename is a key move). Comments, blank lines, key order and unknown keys survive.
7. the new text must parse back to exactly the intended config, or nothing is written
8. `commit`: if the text is unchanged, stop. Otherwise snapshot the current file
   (`config::backup::snapshot`, the last 10 are kept) and write atomically (`atomic::write`).

`replace_in(store, text)` swaps the whole file byte for byte (used by `restore`). It does
not parse the current file, so it can also replace a broken one; the old file is still
snapshotted first.

Other rules:

- A broken file is an error with line and column. It is never treated as empty, so it is
  never overwritten by accident.
- A file with a newer `schema_version` can be read but not written.
- `change` sees the config before `stamp`, so a profile it adds has no `id` yet. Callers
  that need the saved config (for example to store a password under the new id) use
  `mutate_saved` / `mutate_saved_in`.
- `store::load` prints warnings (reserved names, names differing only in case) once;
  `load_from` is silent and is used by writes and by `__complete`.
- CRLF files keep CRLF after an edit.

## User interface

Questions are only asked when both stdin and stderr are terminals (`ui::tty::interactive`);
prompts draw on stderr, so stdout stays clean for scripts. Without a terminal, a command
that would need to ask fails with a message naming the flag to use (`require_terminal`).
`list` is never interactive, so it works in pipes.

All questions go through the `ui::prompt::Prompter` trait. `TerminalPrompter` uses:

- **inquire** for selections, text, confirmations and masked password input (`*` per
  character)
- **ratatui** for `pick_row`, the full-screen profile table (`ui::table_picker`), shown by
  `lopi` without arguments and by `rm`, `edit` and `passwd` without a name. Typing
  filters every column; there is no `q` to quit because letters filter.

The wizards (`ui::wizard::add`, `ui::wizard::edit`) save nothing: they return `AddArgs` /
`EditArgs`, which then take the same path as command-line flags. The edit wizard only
returns fields that changed. A password typed in the wizard comes back separately in
`AddPlan` / `EditPlan`.

Table picker details:

- The `Restore` guard is created right after raw mode is switched on. Whatever happens
  (return, error, panic), raw mode is switched off and the alternate screen is left before
  ssh or an inquire prompt runs.
- The backend writes through `BufWriter<Stderr>`: stderr is unbuffered, and hundreds of
  small writes per frame are slow on the Windows console.
- The loop handles every queued event before redrawing, and redraws only when the state
  says `Outcome::Changed`.
- `KeyEventKind::Release` events are ignored (Windows sends them; otherwise each key
  counts twice). Ctrl+Alt+letter is treated as text, because that is AltGr on Windows.

Testing: `PickerState::handle` is pure and tested key by key; `View` is drawn into
`ratatui::backend::TestBackend`; everything else is tested with `Scripted`, a `Prompter`
that replays a list of answers.

## Passwords and askpass

A profile with `auth = "password"` gets its password from the OS credential store
(`secrets::KeyringStore`: Windows Credential Manager, the macOS Keychain, the Secret
Service on Linux), under the service `lopi` and the profile's `id`, so renaming a profile
keeps its password.

When connecting, lopi sets `SSH_ASKPASS` to its own executable,
`SSH_ASKPASS_REQUIRE=force`, `LOPI_ASKPASS_ID=<id>` and `LOPI_ASKPASS_TARGET=<user@host>`,
and adds `-o NumberOfPasswordPrompts=1` so a wrong password is not retried. ssh then runs
lopi with the prompt text as its argument. `askpass::answer` classifies the prompt:

- a password prompt that names the profile's own `user@host` gets the saved password
- anything else (a jump host's password, host key confirmation, key passphrase, one-time
  code) is asked on the console (`CONIN$` / `/dev/tty`), so a password never reaches the
  wrong server and host keys are never confirmed automatically

If the store is unavailable or has no password, lopi prints a note and ssh asks as usual.

## Backup and restore

`lopi backup` writes one file: a tar archive (`lopi-backup/manifest.toml`,
`profiles.toml`, `secrets.toml` with passwords by profile id, `keys/`) encrypted with
`age` using a passphrase. Both are open formats, so `age -d file.age | tar x` also works.
The format code in `backup_bundle.rs` is pure (pack, unpack, encrypt, decrypt) and tested
without files.

`lopi restore` accepts:

- an `.age` backup: asks the passphrase, unpacks, then restores profiles (`replace_in`),
  passwords and key files (existing key files are kept unless `--overwrite-keys`)
- a plain TOML profiles file: profiles only
- no file: choose one of the automatic snapshots (there is no separate `undo`)

Passwords of profiles that are not part of the restored file are left in the store, so
restoring an old snapshot and then going back loses nothing.

## Install and uninstall

`lopi install` copies the running exe to a per-user folder and, on Windows, puts that
folder on the user `PATH`. No admin rights are needed.

- Windows folder: `%LOCALAPPDATA%\Programs\lopi`. Linux and macOS: `~/.local/bin`
  (`dirs::executable_dir()`, which is `None` on macOS, falling back to `~/.local/bin`).
- The user `PATH` is read and written raw in `HKCU\Environment\Path` (`RRF_NOEXPAND`,
  keeping `REG_EXPAND_SZ`). `env::var("PATH")` is never used for this: it is the system
  and user values combined and already expanded.
- A running exe cannot be overwritten on Windows, so it is renamed to `.old` first. A
  running exe is deleted with `self_replace`.
- PowerShell completion is added as a marked block (`# >>> lopi >>>`) in `$PROFILE`
  (`install::profile_block`, `install::powershell`).
- Double-clicking `lopi.exe` (no arguments and a console of its own, checked with
  `GetConsoleProcessList`) offers to install, and keeps the window open until Enter.

## Shell completion

`lopi completion bash|zsh|powershell` prints a small template from `src/completion/`.
Subcommand names are filled in from the clap definition when printed
(`completion::script`); profile names come from the hidden `lopi __complete` each time Tab
is pressed, most recently used first. They are offered as the first word and after the
subcommands listed in `completion::PROFILE_SUBCOMMANDS` (those that take an existing
profile name).

## Files on disk

| What | Linux | macOS | Windows | Override |
| --- | --- | --- | --- | --- |
| Profiles | `~/.config/lopi/profiles.toml` | `~/Library/Application Support/lopi/profiles.toml` | `%APPDATA%\lopi\profiles.toml` | `LOPI_CONFIG` |
| Write lock | `profiles.toml.lock`, next to the profiles file | same | same | follows the profiles file |
| Snapshots | `~/.local/share/lopi/backups/` | `~/Library/Application Support/lopi/backups/` | `%APPDATA%\lopi\backups\` | `LOPI_DATA_DIR` |
| History | `~/.local/share/lopi/state.toml` | `~/Library/Application Support/lopi/state.toml` | `%LOCALAPPDATA%\lopi\state.toml` | `LOPI_DATA_DIR` |
| Passwords | Secret Service, service `lopi` | Keychain, service `lopi` | Credential Manager, service `lopi` | `LOPI_KEYRING_SERVICE` |
| Installed exe | `~/.local/bin/lopi` | `~/.local/bin/lopi` | `%LOCALAPPDATA%\Programs\lopi\lopi.exe` | `LOPI_INSTALL_DIR` |

## Testing strategy

- **Unit tests** cover the pure parts: `resolve`, `build_args`, `document` (comments,
  order and renames byte for byte), `store` (snapshots, locking, concurrent writers, BOM
  and CRLF), `backup_bundle`, `askpass::answer`, the wizards and pickers through
  `Scripted`, and the table picker through `PickerState::handle` and `TestBackend`.
- **Integration tests** (`tests/cli.rs`) run the binary against a temporary profiles file
  and a fake ssh that prints its arguments.
- **Manual tests** marked `#[ignore]` use the real credential store.

See [CONTRIBUTING.md](CONTRIBUTING.md#testing) for the rules tests must follow.
