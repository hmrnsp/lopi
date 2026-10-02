# lopi

[![CI](https://github.com/hmrnsp/lopi/actions/workflows/ci.yml/badge.svg)](https://github.com/hmrnsp/lopi/actions/workflows/ci.yml)

Open SSH connections from saved profiles with one short command — in bash, zsh,
PowerShell, cmd and Git Bash, on Windows and Linux. One small binary, no runtime.

```console
$ lopi add office admin@10.0.0.5 -p 2222 -i ~/.ssh/id_office
added 'office' (admin@10.0.0.5); connect with `lopi office`

$ lopi off            # a unique prefix is enough, in any letter case
```

lopi does not implement SSH itself: it runs your system's `ssh` with the right
arguments, so your keys, agent, `known_hosts` and `~/.ssh/config` keep working.

## Install

Requires the OpenSSH client (`ssh`) in `PATH`:

- Windows 10/11: *Settings › System › Optional features › OpenSSH Client*
- Debian/Ubuntu: `sudo apt install openssh-client`; Fedora: `sudo dnf install openssh-clients`

Then one of:

```sh
cargo install lopi-ssh   # the crate is lopi-ssh; the command is lopi
```

```sh
# Linux, prebuilt binary
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hmrnsp/lopi/releases/latest/download/lopi-ssh-installer.sh | sh
```

```powershell
# Windows, prebuilt binary
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/lopi/releases/latest/download/lopi-ssh-installer.ps1 | iex"
```

### From a single file (Windows)

`lopi.exe` needs nothing else: no runtime, no Visual C++ Redistributable. To install
it for your user (no admin rights), either double-click it and answer *yes*, or run:

```powershell
.\lopi.exe install            # asks whether to enable PowerShell Tab completion
.\lopi.exe install --completion
```

This copies it to `%LOCALAPPDATA%\Programs\lopi` and adds that folder to your user
`PATH`. Open a new terminal and `lopi` works in PowerShell, cmd and Git Bash.
Running `install` again updates the installed copy, even while it is in use.

Windows may warn about a downloaded, unsigned program ("Windows protected your PC"):
choose *More info › Run anyway*, or run `Unblock-File .\lopi.exe` first.

`lopi uninstall` removes the exe, the `PATH` entry and the Tab completion setup.
Your profiles, backups and history are kept.

On Linux, `lopi install` copies the binary to `~/.local/bin`.

## Usage

| Command | What it does |
| --- | --- |
| `lopi` | Choose a profile from a table (most recent first, type to filter) and connect |
| `lopi <name>` | Connect; a unique prefix works, letter case does not matter |
| `lopi <name> -L 8080:localhost:80` | Extra `ssh` options for this connection |
| `lopi <name> -- uptime` | Run a remote command (everything after `--`) |
| `lopi list [--recent]` | Show profiles, optionally most recently used first |
| `lopi add [name] [[user@]host] [options]` | Add a profile; asks for anything missing |
| `lopi edit [name] [options]` | Change given fields; without options, a guided edit |
| `lopi rm [name] [-y]` | Remove a profile after confirmation |
| `lopi connect <name>` | Connect to a profile whose name clashes with a command |
| `lopi path` | Print the location of the profiles file |
| `lopi completion <shell>` | Print a completion script (bash, zsh, powershell) |
| `lopi install` / `uninstall` | Install this exe for your user and put it on `PATH`, or remove it |
| `lopi passwd [name] [--remove]` | Save, change or delete a profile's password |
| `lopi backup [file]` | Profiles, passwords and key files in one encrypted file |
| `lopi restore [file]` | Bring a backup back; without a file, choose an automatic snapshot |
| `lopi doctor` | Check ssh, the profiles file, key files and saved passwords |

Options for `add` and `edit`: `-p/--port`, `-i/--key`, `-J/--jump`, `-f/--forward`
(repeatable), `-n/--note`; `add` also takes `--password`, and `edit` also `--host`,
`-u/--user`, `--rename` and `--auth key|password|agent`.
With `edit`, an empty value (`--note ""`) removes the field.

`rm` and `edit` only accept a full profile name (in any case), never a prefix.

Without a name, `lopi`, `rm`, `edit` and `passwd` open a full-screen table of your
profiles. Type to filter (any column, any case), move with ↑↓, PgUp/PgDn, Home/End,
press Enter to choose, or Esc to cancel.

Options typed after the profile name come before the profile's own, and ssh keeps the
first value it sees, so `lopi office -p 22` overrides the saved port for one
connection.

### Jump hosts and port forwards

```sh
lopi add bastion admin@bastion.example.com
lopi add db 10.0.0.9 --jump bastion -f L:5432:localhost:5432
lopi db                 # ssh -J admin@bastion.example.com -L 5432:localhost:5432 -- 10.0.0.9
```

A `--jump` item that names a profile is replaced by that profile's address. Its key is
not passed on (`ssh` applies `-i` to the destination only); load it with `ssh-add`.

### Passwords

For servers that only accept a password, lopi can save it and fill it in:

```console
$ lopi add vps root@203.0.113.7 --password     # or choose "password" in `lopi add`
Password for root@203.0.113.7: ********
Type it again: ********
$ lopi vps                                      # logs in without asking
```

- The password is kept in the system credential store (Windows Credential Manager, or
  the Secret Service on Linux), never in the profiles file, never on the command line.
- lopi runs as ssh's `SSH_ASKPASS` helper (needs OpenSSH 8.4 or newer). The saved
  password only answers that profile's own `user@host` prompt. A jump host's password,
  a new host key, a key passphrase or a one-time code are always asked on the terminal.
- If the password changes on the server: `lopi passwd vps`. To stop using it:
  `lopi passwd vps --remove`. Removing the profile deletes the password too.
- Any program running as your user can read your credential store; lopi does not
  add a new way in.

## The profiles file

| | Linux | Windows |
| --- | --- | --- |
| Profiles | `~/.config/lopi/profiles.toml` | `%APPDATA%\lopi\profiles.toml` |
| Backups (last 10) | `~/.local/share/lopi/backups/` | `%APPDATA%\lopi\backups\` |
| Connection history | `~/.local/share/lopi/state.toml` | `%LOCALAPPDATA%\lopi\state.toml` |

It is plain TOML and fine to edit by hand. lopi keeps your comments, ordering and
any keys it does not know when it saves:

```toml
schema_version = 1
# ssh_bin = "C:/Windows/System32/OpenSSH/ssh.exe"   # optional: which ssh to run

# office machines
[profiles.office]
host = "10.0.0.5"
user = "admin"
port = 2222
key = "~/.ssh/id_office"     # ~ is expanded by lopi, so the file is portable
jump = "bastion"
forward = ["L:8080:localhost:80"]
note = "third floor"
id = "k3v9q2m1xz"            # managed by lopi
updated_at = "2026-10-01T10:15:30Z"
```

Every save is locked against concurrent lopi processes, backed up first and written
atomically. A file that does not parse is never overwritten: lopi stops and shows
the line and column.

Saved passwords are not in this file (see [Passwords](#passwords)). Keys are referenced
by path only.

## Backup and restore

To move to another PC, or to recover this one:

```sh
lopi backup                      # writes lopi-backup-YYYYMMDD.age here
lopi restore lopi-backup-20261001.age
```

- The file holds the profiles file, the saved passwords and the private key files your
  profiles use (`--no-keys` leaves keys out), encrypted with a passphrase you choose.
  **Without the passphrase nobody can open it, including you.**
- `restore` shows what it will do and asks first. Key files that already exist and differ
  are only replaced if you say so (or with `--overwrite-keys`).
- `lopi restore` without a file lets you pick one of the automatic snapshots taken
  before every change, so a mistaken `rm` or `edit` can be undone. Restoring also
  snapshots the state it replaces. **Snapshots hold profiles only**: `rm` deletes a
  profile's saved password, so after restoring a snapshot either run
  `lopi passwd <name>` or restore from your backup file instead (lopi tells
  you which profiles need it).
- The format is open: an `age`-encrypted `tar` archive. Without lopi it can be opened
  with the [age](https://age-encryption.org) tool: `age -d file.age | tar x`. That leaves
  passwords and keys unencrypted on disk, so delete them afterwards.

## Shell completion

Completes profile names and commands. `cmd.exe` is not supported.

```sh
# bash: add to ~/.bashrc
eval "$(lopi completion bash)"

# zsh: add to ~/.zshrc, after compinit
eval "$(lopi completion zsh)"
```

```powershell
# PowerShell: add to $PROFILE
lopi completion powershell | Out-String | Invoke-Expression
```

## Git Bash on Windows

Git Bash's default window (mintty) does not give programs a real console, so the
interactive list and questions cannot run there: use `winpty lopi` or Windows
Terminal. Git for Windows also ships its own `ssh`, which may differ from Windows'
OpenSSH; set `ssh_bin` in the profiles file to pick one.

## Exit codes

`0` success · `1` error, a question was cancelled, or `doctor` found a problem · `2` usage error ·
`130` interrupted with Ctrl+C · otherwise the exit code of `ssh` (`255` means the
connection failed).

## Contributing

Bug reports and pull requests are welcome. See:

- [CONTRIBUTING.md](CONTRIBUTING.md): setup, tests and the rules the code follows
- [ARCHITECTURE.md](ARCHITECTURE.md): how the code is organized
- [SECURITY.md](SECURITY.md): how to report a vulnerability privately
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- [CHANGELOG.md](CHANGELOG.md)

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
