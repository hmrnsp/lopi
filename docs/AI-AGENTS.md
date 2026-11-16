# Using lopi with AI agents (copy and paste)

This page is for **you**, the person setting up the agent. You only need to paste one
block into the agent: the English block under "Block to paste", or the Indonesian one
(Bahasa Indonesia) under "Block to paste (Indonesian)". They say the same thing and are
kept short on purpose. The English block costs slightly fewer tokens.

## How to use

1. Pick one block and copy it.
2. Paste it into the instruction file the agent reads at the start of a session:

| Agent                    | Paste into                                                                        |
| ------------------------ | --------------------------------------------------------------------------------- |
| Claude Code              | `CLAUDE.md` in the project folder, or `~/.claude/CLAUDE.md` for all projects      |
| Other agents             | `AGENTS.md` in the project folder. Many agents read it; check your agent's docs   |
| No instruction file      | Paste it as the first message of the session                                      |

3. Make sure the agent has terminal access. Without it, the agent cannot run `lopi`.

The instruction file names used by Hermes and other agents have not been checked. If you
are unsure, use the "first message" way.

## Suggested permissions

- **Allow automatically:** `lopi list --json`, `lopi ping <name>`, `lopi doctor`,
  `lopi path`.
- **Ask every time:** `lopi <name> -- <command>`. It runs a command on a real server.
- **Do not allow:** `lopi rm`, `lopi restore`, `lopi backup`, `lopi passwd`,
  `lopi edit`, `lopi update`, `lopi uninstall`.

In Claude Code, automatic permissions are set with `permissions.allow` in
`.claude/settings.json`, for example `"Bash(lopi list:*)"` and `"Bash(lopi ping:*)"`.

## Block to paste

````markdown
## lopi (SSH profiles)

`lopi` opens SSH connections from saved profiles. It runs the system `ssh`; it has no
API. Use it only through the shell, always with every argument given.

### Safe, read-only (no need to ask)
- `lopi list --json`: all profiles as a JSON array. Keys: name, host, user, port, key,
  jump, forward, auth, note, last_used. Passwords are never included.
- `lopi ping <name>`: checks the server answers, without logging in. Exit 0 = ok,
  1 = fail. A fail line starting with "FAIL, a host key has CHANGED" means possible
  interception: stop and tell the user.
- `lopi doctor`: checks ssh, the profiles file, key files, saved passwords.

### Runs a command on a real server (ask the user first, every time)
- `lopi <name> -- <command>`: runs ONE remote command, then exits. Example:
  `lopi office -- uptime`. Everything after `--` goes to the server.
- Use the profile name exactly as `lopi list --json` shows it. Names are case-sensitive.
  lopi never guesses; a wrong name only prints suggestions.
- If the name clashes with a subcommand, use `lopi connect <name> -- <command>`.

### Never do
- Never run `lopi` with no arguments, or `ping`, `edit`, `rm`, `passwd` without a
  profile name, or `add` without all its flags. They ask questions in a terminal menu.
  Without a terminal they fail with "cannot ask questions because this is not a
  terminal".
- Never run `lopi passwd --show`, `backup`, `restore`, `rm`, `edit`, `update`, `install`
  or `uninstall` unless the user explicitly asks for that exact action.
- Never read, print or copy key files (for example `~/.ssh/id_*`), `lopi backup`
  files (`*.age`) or the profiles file contents beyond `lopi list --json`.
- Never try to get a saved password. lopi keeps it in the system credential store and
  answers ssh's password prompt itself; you do not need it.
- Do not chain destructive remote commands (`rm -rf`, `shutdown`, `reboot`, `dd`,
  `mkfs`, writing to `/etc`) without the user confirming that exact command.

### If something fails
- "host key is not known yet": tell the user to run `lopi <name>` once in their own
  terminal to accept the key. Do not retry in a loop.
- Timeout or "FAIL, no answer": report it; do not retry more than once.
- Show the user the exact error text; do not guess a fix.
````

## Block to paste (Indonesian)

The same content as the English block above, in Bahasa Indonesia. Paste one block, not
both. If you change one, change the other too.

````markdown
## lopi (profil SSH)

`lopi` membuka koneksi SSH dari profil yang tersimpan. Lopi menjalankan `ssh` sistem dan
tidak punya API. Pakai hanya lewat shell, dan selalu beri semua argumen secara lengkap.

### Aman, hanya membaca (tidak perlu bertanya)
- `lopi list --json`: semua profil sebagai array JSON. Kunci: name, host, user, port, key,
  jump, forward, auth, note, last_used. Password tidak pernah ikut.
- `lopi ping <nama>`: memeriksa apakah server menjawab, tanpa login. Exit 0 = ok,
  1 = gagal. Jika barisnya berawal "FAIL, a host key has CHANGED", ada kemungkinan
  penyadapan: berhenti dan beri tahu pengguna.
- `lopi doctor`: memeriksa ssh, berkas profil, berkas kunci, dan password tersimpan.

### Menjalankan perintah di server sungguhan (tanya pengguna dulu, setiap kali)
- `lopi <nama> -- <perintah>`: menjalankan SATU perintah di server, lalu keluar. Contoh:
  `lopi office -- uptime`. Semua yang ada setelah `--` dikirim ke server.
- Pakai nama profil persis seperti di `lopi list --json`. Huruf besar/kecil berpengaruh.
  Lopi tidak menebak; nama yang salah hanya menampilkan saran.
- Jika nama bentrok dengan subperintah, pakai `lopi connect <nama> -- <perintah>`.

### Jangan lakukan
- Jangan jalankan `lopi` tanpa argumen, atau `ping`, `edit`, `rm`, `passwd` tanpa nama
  profil, atau `add` tanpa semua flag-nya. Perintah itu bertanya lewat menu di terminal.
  Tanpa terminal, perintah gagal dengan pesan "cannot ask questions because this is not a
  terminal".
- Jangan jalankan `lopi passwd --show`, `backup`, `restore`, `rm`, `edit`, `update`,
  `install`, atau `uninstall`, kecuali pengguna meminta tindakan itu secara jelas.
- Jangan membaca, mencetak, atau menyalin berkas kunci (misalnya `~/.ssh/id_*`), berkas
  `lopi backup` (`*.age`), atau isi berkas profil di luar `lopi list --json`.
- Jangan mencoba mengambil password tersimpan. Lopi menyimpannya di penyimpanan kredensial
  sistem dan menjawab permintaan password dari ssh sendiri. Anda tidak membutuhkannya.
- Jangan merangkai perintah yang merusak di server (`rm -rf`, `shutdown`, `reboot`, `dd`,
  `mkfs`, menulis ke `/etc`) tanpa pengguna mengonfirmasi perintah persisnya.

### Jika gagal
- "host key is not known yet": minta pengguna menjalankan `lopi <nama>` satu kali di
  terminalnya sendiri untuk menerima kunci. Jangan coba ulang berulang-ulang.
- Timeout atau "FAIL, no answer": laporkan. Coba ulang paling banyak sekali.
- Tampilkan teks error persis kepada pengguna. Jangan menebak perbaikan.
````

## Not yet verified

- Password profiles: whether `lopi <name> -- <command>` runs fully without a terminal.
  From the code, lopi answers the password prompt itself, but this has not been run.
- Servers whose host key is not known yet: ssh may fail without a terminal. The blocks
  above tell the agent to stop and ask you to connect once by hand.
