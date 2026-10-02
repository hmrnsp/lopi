# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Tab completion now offers profile names after `lopi passwd`, as it already did after
  `rm`, `edit` and `connect`.

## [0.3.1] - 2026-10-02

### Changed

- **Breaking:** profile names must be typed in full and with the same letter case
  everywhere (`lopi <name>`, `connect`, `rm`, `edit`, `passwd` and jump hosts). A shortened
  name or another letter case no longer connects to a profile, so a typo can never reach
  the wrong server; lopi suggests the right name instead. To avoid typing, run `lopi`
  without a name and pick from the table, or use Tab completion.
- A jump host that equals a profile name only in another letter case is used as a host
  name, with a warning (also reported by `lopi doctor`).

## [0.3.0] - 2026-10-02

### Added

- macOS support: prebuilt binaries for Apple Silicon and Intel Macs, also picked up by
  the shell installer. Saved passwords are kept in the macOS Keychain.

## [0.2.1] - 2026-10-02

### Fixed

- Password and passphrase prompts (`add`, `edit`, `passwd`, `backup`, `restore`) now
  place the cursor after the prompt instead of at the start of the line, and show
  each typed character as `*`.

## [0.2.0] - 2026-10-02

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

[Unreleased]: https://github.com/hmrnsp/lopi/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/hmrnsp/lopi/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/hmrnsp/lopi/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/hmrnsp/lopi/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/hmrnsp/lopi/releases/tag/v0.2.0
