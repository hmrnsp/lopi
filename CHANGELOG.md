# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

First public release.

### Added

- Connect with `lopi <name>`: a unique prefix works and letter case does not matter;
  extra ssh options and a remote command after `--` are passed on. Typos get suggestions.
- `lopi connect <name>` for profile names that clash with a subcommand.
- Profile management: `list` (with `--recent`), `add`, `edit` (including `--rename`),
  `rm`, and `path`.
- Guided `add` and `edit` when run in a terminal without all options.
- A full-screen table to choose a profile when `lopi`, `rm`, `edit` or `passwd` is run
  without a name, filtered as you type.
- Jump hosts (`--jump`, which may name another profile) and port forwards (`--forward`).
- Saved passwords for profiles with `auth = "password"`, kept in the OS credential store
  and given to ssh through askpass only for the profile's own `user@host`; `lopi passwd`
  to save, change or remove one, and `add --password` / `edit --auth`.
- `lopi backup` and `lopi restore`: profiles, saved passwords and key files in one
  passphrase-encrypted file (age + tar).
- An automatic snapshot of the profiles file before every change (the last 10 are kept);
  `lopi restore` without a file chooses one.
- Edits keep comments, blank lines, key order and unknown keys in the profiles file.
- Connection history, used to sort `list --recent`, the picker and completion.
- `lopi doctor` checks ssh, the profiles file, key files and saved passwords.
- `lopi install` and `lopi uninstall`: a per-user install without admin rights, on the
  user `PATH` on Windows, with optional PowerShell Tab completion. Double-clicking
  `lopi.exe` offers to install.
- Tab completion for bash, zsh and PowerShell (`lopi completion <shell>`).
- `ssh_bin` in the profiles file to choose which ssh to run.
- Prebuilt binaries for Windows (x86_64, MSVC) and Linux (x86_64, musl) with shell and
  PowerShell installers.

[Unreleased]: https://github.com/hmrnsp/lopi/commits/main
