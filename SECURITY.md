# Security policy

## Supported versions

lopi is before 1.0. Only the latest release gets security fixes.

## Reporting a vulnerability

Please do **not** open a public issue. Report privately through GitHub's private
vulnerability reporting: go to <https://github.com/hmrnsp/lopi/security> and choose
**Report a vulnerability**.

Include:

- the lopi version (`lopi --version`)
- your OS and shell, and `ssh -V`
- what an attacker can do (the impact)
- steps to reproduce, ideally with a minimal profiles file

You should get a first reply within 7 days. This is a best-effort promise from a small
project. Once a fix is released, the advisory is published with credit to you, unless
you prefer not to be named.

## What lopi promises

- **Passwords stay in the credential store.** Saved passwords are stored only in the OS
  credential store (Windows Credential Manager, the Secret Service on Linux) and inside
  backup files you encrypt with a passphrase. They are never written to the profiles
  file, never passed as command-line arguments or environment variables, and never
  logged.
- **A password goes only to its own server.** When ssh asks lopi for a password
  (askpass), lopi answers only a password prompt for the profile's own `user@host`. Other
  prompts (a jump host's password, host key confirmation, key passphrases, one-time codes)
  are asked on the terminal.
- **Backups are encrypted.** `lopi backup` encrypts profiles, passwords and private key
  files with [age](https://age-encryption.org) using your passphrase.
- **No command strings.** ssh is started with each argument passed separately, never
  through a shell, and lopi rejects host, user and profile names that start with `-`.
- **Safe writes.** The profiles file is changed under a lock, snapshotted first, and
  written atomically (temporary file, then rename), so a crash never leaves a half-written
  file.

## Scope

In scope:

- leaking a saved password (to a file, an argument, the environment, a log or the wrong
  server)
- weaknesses in the backup file format or its handling of passphrases and key files
- argument or option injection into ssh through profile values or names
- `lopi install` / `uninstall`: changes to `PATH`, the install folder or the PowerShell
  profile that go beyond what they should

Out of scope:

- bugs in OpenSSH itself
- bugs in the OS credential store
- other processes running as the same user (they can already read your files and
  credential store)
- weak backup passphrases chosen by the user
