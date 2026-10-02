# lopi

[![CI](https://github.com/hmrnsp/lopi/actions/workflows/ci.yml/badge.svg)](https://github.com/hmrnsp/lopi/actions/workflows/ci.yml)

Open SSH connections from saved profiles with one short command, on Windows, macOS
and Linux. One small binary, no runtime.

![lopi's profile picker: type to filter, Enter to connect](https://raw.githubusercontent.com/hmrnsp/lopi/main/docs/images/screenshot_lopi.png)

lopi does not implement SSH itself: it runs your system's `ssh` with the right
arguments, so your keys, agent, `known_hosts` and `~/.ssh/config` keep working.

## Contents

- [Features](#features)
- [Quick start](#quick-start)
- [Install](#install): [Linux](#linux) · [macOS](#macos) · [Windows](#windows) · [From source](#from-source)
- [Getting started](#getting-started)
- [Command reference](#command-reference)
- [Jump hosts and port forwards](#jump-hosts-and-port-forwards)
- [Passwords](#passwords)
- [Backup and restore](#backup-and-restore)
- [Shell completion](#shell-completion)
- [Where lopi keeps its files](#where-lopi-keeps-its-files)
- [Troubleshooting](#troubleshooting)
- [Uninstall](#uninstall)

## Features

- **Short commands**: `lopi office`, or just `lopi off`; a unique prefix is enough, in
  any letter case.
- **Pick from a table**: run `lopi` alone, type to filter, press Enter to connect.
- **Guided setup**: `lopi add` asks for anything you leave out.
- **Saved passwords** for servers without keys, kept in the system credential store
  (Windows Credential Manager, macOS Keychain, Secret Service on Linux), never in a file.
- **Jump hosts and port forwards** saved per profile.
- **Encrypted backups** of profiles, passwords and key files, plus automatic snapshots
  before every change.
- **Works in every shell**: bash, zsh, PowerShell, cmd and Git Bash, with Tab completion
  for bash, zsh and PowerShell.

## Quick start

**1. Install lopi.** On Linux or macOS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hmrnsp/lopi/releases/latest/download/lopi-ssh-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/lopi/releases/latest/download/lopi-ssh-installer.ps1 | iex"
```

Then open a new terminal. Other ways to install are under [Install](#install).

**2. Save a server as a profile.** Run `lopi add` and answer the questions, or give
everything on one line:

```console
$ lopi add office admin@10.0.0.5 -p 2222 -i ~/.ssh/id_office
added 'office' (admin@10.0.0.5); connect with `lopi office`
```

| Part | Meaning |
| --- | --- |
| `office` | A name you choose for this server |
| `admin@10.0.0.5` | The user to log in as, and the server's address |
| `-p 2222` | The SSH port (optional; without it, ssh uses port 22) |
| `-i ~/.ssh/id_office` | The private key to log in with (optional) |

**3. Connect.**

```sh
lopi office
```

lopi runs `ssh -p 2222 -i ~/.ssh/id_office -- admin@10.0.0.5` for you, so you never
have to remember the address, port or key again.

You do not have to type the whole name: any start of it that matches only one profile
works, in any letter case. `lopi off` and `lopi OFF` also connect to `office`. Run
`lopi` with no name to choose from the table shown above.

## Install

### Requirements

lopi needs the OpenSSH client (`ssh`). Check with `ssh -V`; if it is missing:

| System | How to get `ssh` |
| --- | --- |
| Windows 10/11 | *Settings › System › Optional features › OpenSSH Client*, or in an admin PowerShell: `Add-WindowsCapability -Online -Name OpenSSH.Client~~~~0.0.1.0` |
| macOS | Already installed |
| Debian, Ubuntu | `sudo apt install openssh-client` |
| Fedora | `sudo dnf install openssh-clients` |
| Arch | `sudo pacman -S openssh` |

Prebuilt binaries are available for:

| System | Architecture |
| --- | --- |
| Windows 10/11 | x86_64 |
| macOS | Apple Silicon (arm64) and Intel (x86_64) |
| Linux | x86_64 (static binary, works on any distribution) |

On other systems, [build from source](#from-source).

### Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hmrnsp/lopi/releases/latest/download/lopi-ssh-installer.sh | sh
```

The script installs `lopi` to `~/.cargo/bin` and adds that folder to your `PATH`
through your shell's startup files. Open a new terminal afterwards.

Or install by hand: download `lopi-ssh-x86_64-unknown-linux-musl.tar.xz` from the
[latest release](https://github.com/hmrnsp/lopi/releases/latest), then:

```sh
tar -xf lopi-ssh-x86_64-unknown-linux-musl.tar.xz
cd lopi-ssh-x86_64-unknown-linux-musl
./lopi install        # copies lopi to ~/.local/bin
```

If `~/.local/bin` is not on your `PATH`, `lopi install` prints the line to add to
`~/.bashrc` or `~/.zshrc`.

### macOS

The same script works on macOS and picks the right binary for Apple Silicon or Intel:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hmrnsp/lopi/releases/latest/download/lopi-ssh-installer.sh | sh
```

It installs `lopi` to `~/.cargo/bin` and adds that folder to your `PATH`. Open a new
terminal afterwards.

Or install by hand: download `lopi-ssh-aarch64-apple-darwin.tar.xz` (Apple Silicon) or
`lopi-ssh-x86_64-apple-darwin.tar.xz` (Intel) from the
[latest release](https://github.com/hmrnsp/lopi/releases/latest), then:

```sh
tar -xf lopi-ssh-aarch64-apple-darwin.tar.xz
cd lopi-ssh-aarch64-apple-darwin
xattr -d com.apple.quarantine ./lopi    # only needed for files downloaded in a browser
./lopi install                          # copies lopi to ~/.local/bin
```

lopi is not signed by Apple, so macOS blocks a copy downloaded in a browser; the `xattr`
line allows it. macOS does not put `~/.local/bin` on `PATH` by default: `lopi install`
then prints the line to add to `~/.zshrc`.

### Windows

In PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/lopi/releases/latest/download/lopi-ssh-installer.ps1 | iex"
```

The script installs `lopi.exe` to `%USERPROFILE%\.cargo\bin` and adds that folder to
your user `PATH`. Open a new terminal afterwards.

#### From a single file

`lopi.exe` needs nothing else: no runtime, no Visual C++ Redistributable. Download
`lopi-ssh-x86_64-pc-windows-msvc.zip` from the
[latest release](https://github.com/hmrnsp/lopi/releases/latest) and extract it. To
install it for your user (no admin rights), either double-click `lopi.exe` and answer
*yes*, or run:

```powershell
.\lopi.exe install            # asks whether to enable PowerShell Tab completion
.\lopi.exe install --completion
```

This copies it to `%LOCALAPPDATA%\Programs\lopi` and adds that folder to your user
`PATH`. Open a new terminal and `lopi` works in PowerShell, cmd and Git Bash.
Running `install` again updates the installed copy, even while it is in use.

Windows may warn about a downloaded, unsigned program ("Windows protected your PC"):
choose *More info › Run anyway*, or run `Unblock-File .\lopi.exe` first.

### From source

With [Rust](https://rustup.rs) 1.89 or newer, on any system:

```sh
cargo install lopi-ssh --locked   # the crate is lopi-ssh; the command is lopi
```

To build the latest development version instead:
`cargo install --git https://github.com/hmrnsp/lopi --locked`.

This needs a C linker: Visual Studio Build Tools on Windows, the Xcode Command Line
Tools on macOS (`xcode-select --install`), or `gcc`/`cc` on Linux.

### Check the install

```sh
lopi --version
lopi doctor          # checks ssh, the profiles file, key files and saved passwords
```

## Getting started

**1. Add a profile.** Give everything on one line:

```console
$ lopi add office admin@10.0.0.5 -p 2222 -i ~/.ssh/id_office
added 'office' (admin@10.0.0.5); connect with `lopi office`
```

or run `lopi add` alone and answer the questions (name, address, port, how to log in).

**2. Connect.** Use the name, or any unique start of it, in any letter case:

```sh
lopi office
lopi off
```

**3. Pick from a table.** Run `lopi` with no arguments to get the table shown at the top
of this page: most recently used first, type to filter, Enter to connect.

**4. See and change your profiles.**

```console
$ lopi list
NAME     TARGET                     KEY               AUTH   NOTE
bastion  admin@bastion.example.com                    agent
office   admin@10.0.0.5:2222        ~/.ssh/id_office  key

$ lopi edit office -p 22 -n "third floor"
updated 'office'

$ lopi rm bastion
```

**5. Pass extra options or run a command.** Anything after the name goes to `ssh`;
anything after `--` runs on the server:

```sh
lopi office -L 8080:localhost:80     # forward a port for this connection only
lopi office -- uptime                # run a remote command
```

## Command reference

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
| `lopi install` / `uninstall` | Install this binary for your user and put it on `PATH`, or remove it |
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

Run `lopi --help` or `lopi <command> --help` for every option.

## Jump hosts and port forwards

```sh
lopi add bastion admin@bastion.example.com
lopi add db 10.0.0.9 --jump bastion -f L:5432:localhost:5432
lopi db                 # ssh -J admin@bastion.example.com -L 5432:localhost:5432 -- 10.0.0.9
```

A `--jump` item that names a profile is replaced by that profile's address. Its key is
not passed on (`ssh` applies `-i` to the destination only); load it with `ssh-add`.

## Passwords

For servers that only accept a password, lopi can save it and fill it in:

```console
$ lopi add vps root@203.0.113.7 --password     # or choose "password" in `lopi add`
Password for root@203.0.113.7: ********
Type it again: ********
$ lopi vps                                      # logs in without asking
```

- The password is kept in the system credential store (Windows Credential Manager, the
  macOS Keychain, or the Secret Service on Linux), never in the profiles file, never on
  the command line.
- lopi runs as ssh's `SSH_ASKPASS` helper (needs OpenSSH 8.4 or newer). The saved
  password only answers that profile's own `user@host` prompt. A jump host's password,
  a new host key, a key passphrase or a one-time code are always asked on the terminal.
- If the password changes on the server: `lopi passwd vps`. To stop using it:
  `lopi passwd vps --remove`. Removing the profile deletes the password too.
- Any program running as your user can read your credential store; lopi does not
  add a new way in.

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

# zsh (the default shell on macOS): add to ~/.zshrc, after compinit
eval "$(lopi completion zsh)"
```

```powershell
# PowerShell: add to $PROFILE
lopi completion powershell | Out-String | Invoke-Expression
```

On Windows, `lopi install --completion` sets up PowerShell for you.

## Where lopi keeps its files

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Profiles | `~/.config/lopi/profiles.toml` | `~/Library/Application Support/lopi/profiles.toml` | `%APPDATA%\lopi\profiles.toml` |
| Snapshots (last 10) | `~/.local/share/lopi/backups/` | `~/Library/Application Support/lopi/backups/` | `%APPDATA%\lopi\backups\` |
| Connection history | `~/.local/share/lopi/state.toml` | `~/Library/Application Support/lopi/state.toml` | `%LOCALAPPDATA%\lopi\state.toml` |

`lopi path` prints where your profiles file is. The macOS path contains a space, so quote
it in a shell: `open "$(dirname "$(lopi path)")"`.

The profiles file is plain TOML and fine to edit by hand. lopi keeps your comments,
ordering and any keys it does not know when it saves:

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

## Troubleshooting

Start with `lopi doctor`: it checks ssh, the profiles file, key files and saved
passwords, and says how to fix what it finds.

**`lopi`: command not found right after installing.** Open a new terminal so it picks up
the new `PATH`. On Linux and macOS, after the install script you can also run
`. ~/.cargo/env` in the current one.

**"cannot find 'ssh'".** Install the OpenSSH client (see [Requirements](#requirements)),
or point lopi to a specific one with `ssh_bin` in the profiles file.

**A saved password is not filled in.** Saved passwords need OpenSSH 8.4 or newer; check
with `ssh -V` (`lopi doctor` reports it too). Windows 10's built-in client can be older:
update Windows, or install a newer
[Win32-OpenSSH](https://github.com/PowerShell/Win32-OpenSSH/releases) and set `ssh_bin`.

**"the system credential store is not available" (Linux).** Saved passwords use the
Secret Service, which needs a running keyring such as GNOME Keyring or KWallet. It is
usually missing on servers and minimal installs; use keys there instead.

**macOS says lopi "cannot be opened".** The binary is not signed by Apple. Run
`xattr -d com.apple.quarantine /path/to/lopi`, or allow it in *System Settings ›
Privacy & Security*. The install script does not have this problem.

**Windows says "Windows protected your PC".** Choose *More info › Run anyway*, or run
`Unblock-File .\lopi.exe` first.

**Git Bash on Windows.** Git Bash's default window (mintty) does not give programs a real
console, so the interactive table and questions cannot run there: use `winpty lopi` or
Windows Terminal. Git for Windows also ships its own `ssh`, which may differ from
Windows' OpenSSH; set `ssh_bin` in the profiles file to pick one.

## Uninstall

How to remove lopi depends on how you installed it:

| Installed with | Remove with |
| --- | --- |
| `lopi install` | `lopi uninstall` (also removes the `PATH` entry and Tab completion setup on Windows) |
| Install script, Linux or macOS | `rm ~/.cargo/bin/lopi` |
| Install script, Windows | `Remove-Item "$env:USERPROFILE\.cargo\bin\lopi.exe"` |
| `cargo install` | `cargo uninstall lopi-ssh` |

Your profiles, snapshots and history are kept. To remove them too, delete the folders
listed in [Where lopi keeps its files](#where-lopi-keeps-its-files). Saved passwords stay
in the credential store until you run `lopi passwd <name> --remove` or `lopi rm <name>`
first.

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
