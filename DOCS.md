# fastlock documentation

> **Status, 2026-11-25:** fastlock is at an early stage. Three commands
> work: `inspect`, `verify` and `decrypt`, plus a first terminal interface
> for them. Everything else in this file describes how the program is meant
> to work. Nothing has been released and nothing has been audited. Do not
> rely on fastlock as your only way to open your files.

This file is the documentation of fastlock: what the program does, how its
interfaces behave, and what it does and does not protect.

- To contribute, read [CONTRIBUTING.md](CONTRIBUTING.md).
- Rules for AI coding agents, including the rules for editing this file, are
  in [AGENTS.md](AGENTS.md).

## Contents

- [What fastlock does](#what-fastlock-does)
- [What fastlock does not do](#what-fastlock-does-not-do)
- [Concepts](#concepts)
- [Interfaces](#interfaces)
- [Security model](#security-model)
- [Glossary](#glossary)

## What fastlock does

fastlock brings several encryption tools together in one program.

- It encrypts and decrypts files in established formats: age and OpenPGP.
- It packs, compresses and encrypts files and folders in one step.
- It manages keys: age keys, OpenPGP keys and SSH keys. You can export the
  keys you have saved and import them again, and import keys from `~/.ssh`
  and from GnuPG.
- It stores TOTP keys and shows the current codes.
- Later, it will encrypt and decrypt short text messages with PGP keys,
  without creating a file.

There are three ways to use it:

1. **Commands.** Everything fastlock can do is a command.
2. **Terminal interface.** An optional interface inside the terminal. It
   runs the same commands.
3. **Desktop app.** Planned for later. It also runs the same commands.

fastlock is written in Rust. The encryption, compression and key handling
come from established libraries, written in Rust or in C. It is built for
Linux first, then for Windows and macOS.

## What fastlock does not do

- **It does not invent cryptography or file formats.** It uses published
  formats and established libraries, so a file written by fastlock can be
  opened with the standard tool for that format.
- **It does not offer a menu of ciphers.** You choose who can open a file.
  The format decides the cipher.
- **It does not use the network.** No telemetry and no update checks.
- **It does not promise to erase files.** See
  [Limits](#what-fastlock-does-not-protect).

## Concepts

### Sessions

Opening an encrypted file starts a session. While the session is open you
work with the contents. Closing the session removes what was decrypted and
clears the keys from memory.

A session is a temporary copy. Changes you make in it are not written back
to the encrypted file, and fastlock warns you when you close a session that
has changes. To keep them, encrypt the changed files again.

A session is meant to close by itself as well: after a time without
activity, when the screen locks, and before the computer sleeps. The exact
timings are not decided.

### Who can open a file

When you encrypt, you do not pick a cipher. You pick who will be able to
open the file.

| You choose | Who can open it | Good to know |
|---|---|---|
| A passphrase | Anyone who knows the passphrase | The strength of the file is the strength of the passphrase. |
| An age key | The holder of that key | Post-quantum age keys need the separate `age` program for now; see [Keys and where they live](#keys-and-where-they-live). |
| An SSH key | The holder of that SSH key | Useful when someone already has an SSH key. No post-quantum protection. Prefer ed25519 keys to RSA keys; see [N10](#what-fastlock-does-not-protect). |
| A PGP key | The holder of that PGP key | For people who use PGP tools. |

### Compression

fastlock can pack, compress and encrypt in a single operation. Compression
always happens before encryption.

| Method | Use it when |
|---|---|
| Zstandard (default) | You want a small file quickly. |
| LZMA | You want the smallest file and can wait longer. |
| None | The data is already compressed, such as photos and video. |

All compression in fastlock is lossless: every file comes back byte for
byte.

Compression is on by default. A single file becomes `name.zst.age`, and a
folder becomes `name.tar.zst.age`. With compression turned off, a single
file becomes a plain `name.age` that any age tool opens directly.

### Keys and where they live

fastlock is never the only program that can read a key. Every key is kept
in a standard format, so if you stop using fastlock, your keys still work
everywhere else.

- **SSH keys** live in `~/.ssh`, as OpenSSH files. Both ed25519 and RSA
  keys work; ed25519 keys are the better choice (see
  [N10](#what-fastlock-does-not-protect)).
- **PGP keys** are kept by fastlock as standard OpenPGP files, in a folder of
  its own. fastlock does not write into the GnuPG folder `~/.gnupg`. It does
  its PGP work itself, so GnuPG does not have to be installed.
- **Importing.** fastlock can import keys from the standard folders of other
  tools: SSH keys from `~/.ssh`, and PGP keys from GnuPG. An imported SSH
  key stays where it is; fastlock only adds it to its list. It can also export
  PGP keys to GnuPG. Private keys held by GnuPG can only be read through the
  `gpg` program, so importing those needs GnuPG installed.
- **Post-quantum age keys** are made and opened by the separate `age`
  program, version 1.3 or newer, until fastlock can do it itself. They work
  only when that program is installed. Until then new age keys are classic
  keys by default, and `fastlock key new --pq` makes a post-quantum one.
  Once fastlock can make them itself, post-quantum keys become the default.

Where age keys live, and which folder holds the PGP keys, is not decided
yet.

### TOTP

fastlock stores TOTP keys encrypted and shows the current codes. TOTP keys
can be exported and imported together with your other keys. Read
[N5](#what-fastlock-does-not-protect) before you keep them on the same
device as your passwords.

## Interfaces

### Commands

The commands below were designed on 2026-11-24. Details can still change
before the first release.

**Working now:** `inspect`, `verify`, `decrypt`, `tui`, `help` and
`version`. Each has a full entry under
[Commands that work now](#commands-that-work-now). Everything else in the
tables below is designed and not implemented yet.

Every command has the shape `fastlock <command> [arguments] [flags]`.

- Actions on files are single verbs, such as `encrypt` and `open`. Things
  fastlock keeps for you are groups, such as `key list` and `totp add`.
- There is no flag for choosing a format. The format follows from who can
  open the file: age keys, SSH keys and passphrases give an age file, and
  PGP keys give an OpenPGP file. One file cannot mix the two kinds.
- A passphrase is never an argument. fastlock asks for it, or reads it from
  a file given with `--passphrase-file`.
- Nothing is overwritten silently. A command refuses to replace an existing
  file unless you add `--force`.
- Every command can answer in JSON with `--json`.
- Running `fastlock` with no arguments prints a short help.

#### Files

| Command | What it does |
|---|---|
| `fastlock encrypt <path>...` | Encrypts a file. Given a folder or several paths, packs, compresses and encrypts them into one file. |
| `fastlock decrypt <file>` | Decrypts to disk, and unpacks if the file is an archive. |
| `fastlock inspect <file>` | Shows what a file is without needing a key: format, who can open it, post-quantum or not, size. With `--contents` it asks for the key and lists the files inside, writing nothing. |
| `fastlock verify <file>` | Decrypts in memory, checks the file to its last byte, and writes nothing. |

Main flags of `encrypt`:

| Flag | Meaning |
|---|---|
| `--to <key>` | Who can open the file: the name of a saved key, a public key, or a path to one. Can be repeated. |
| `-p`, `--passphrase` | Protect the file with a passphrase. |
| `-o`, `--output <path>` | Where to write the result. |
| `--compress zstd\|lzma\|none` | The compression method. The default is `zstd`. |
| `-a`, `--armor` | Write text instead of binary data. |

Main flags of `decrypt`: `-o`, `--output <path>` and `--key <name or path>`.

#### Sessions

| Command | What it does |
|---|---|
| `fastlock open <file>` | Starts a session: decrypts into memory and makes the contents available until the session is closed. |
| `fastlock close [<session>]` | Closes a session and wipes what was decrypted. `--all` closes every session. |
| `fastlock status` | Lists open sessions and the time left before each closes by itself. |

`fastlock open` keeps running in the terminal that started it. It shows
where the contents are and how much time is left. Press Ctrl+C there, or
run `fastlock close` in another terminal, to close the session. Closing the
terminal, or losing the connection, closes the session too. With `--detach`
the session runs in the background instead.

#### Keys

| Command | What it does |
|---|---|
| `fastlock key list` | Lists keys. `--type age\|ssh\|pgp` narrows the list. |
| `fastlock key new <name>` | Creates a key: an age key by default, classic for now (see [Keys and where they live](#keys-and-where-they-live)). `--pq` makes a post-quantum age key. `--type ssh` and `--type pgp` create the other kinds. |
| `fastlock key show <name>` | Shows one key: type, public key, fingerprint, and where it is stored. |
| `fastlock key import <file>` | Adds a key from a file: your own, or someone's public key. It can also import keys from `~/.ssh` and from GnuPG; how that is asked for is not decided. |
| `fastlock key export <name>` | Writes the public key, to give to someone. `--secret` writes the private key. |
| `fastlock key remove <name>` | Removes a key after confirmation. |

#### TOTP

| Command | What it does |
|---|---|
| `fastlock totp list` | Lists TOTP keys. |
| `fastlock totp add <name>` | Adds a TOTP key. The secret is read from a prompt or a file, never from an argument. |
| `fastlock totp code <name>` | Prints the current code. |
| `fastlock totp remove <name>` | Removes a TOTP key after confirmation. |

#### Backup

| Command | What it does |
|---|---|
| `fastlock backup -o <file>` | Saves all your keys and TOTP keys into one encrypted file. The file is an ordinary fastlock archive, so standard tools can open it. |
| `fastlock restore <file>` | Brings them back from such a file. |

#### The program

| Command | What it does |
|---|---|
| `fastlock tui` | Opens the terminal interface. |
| `fastlock help [<command>]` | Shows help. `-h` and `--help` work on every command. |
| `fastlock version` | Prints the version. |
| `fastlock completions <shell>` | Prints shell completions. |

Reserved for later: `fastlock message encrypt` and
`fastlock message decrypt`, for quick messages with PGP keys.

#### Commands that work now

Opening a file that uses a key needs the `age` program, version 1.3 or
newer, installed on the computer. fastlock hands the key to it over a pipe.
A file protected by a passphrase is opened by fastlock itself. This is
planned to change: fastlock will open classic age keys and SSH ed25519 keys
itself, and only post-quantum keys will need the `age` program.

##### `fastlock inspect`

```
fastlock inspect <file> [--json]
```

Shows what a file is, without a key: its format, who can open it, whether
it is post-quantum, its size, and whether a checksum file (`<file>.sha256`)
sits next to it. A file that is not recognized is reported as such; that is
not an error. The `--contents` flag is not implemented yet.

```
$ fastlock inspect backup.tar.zst.age
backup.tar.zst.age
  format         age v1
  opens with     post-quantum age key
  post-quantum   yes
  size           2.9 MiB
  checksum file  backup.tar.zst.age.sha256
```

Exit codes: 0 on success, 4 when the file starts like an age file but its
header is damaged, 5 when the file does not exist.

##### `fastlock verify`

```
fastlock verify <file> [--key <path>] [--passphrase-file <path>] [--json]
```

Decrypts the file in memory and checks it. Nothing is written to disk.

1. **Checksum file.** When `<file>.sha256` exists, the encrypted file is
   compared with it.
2. **Decryption.** The whole file is decrypted and authenticated to its last
   byte.
3. **Compression.** Zstandard data is decompressed to its end.
4. **Archive.** A tar archive is read to its end, and every file in it is
   hashed.
5. **Checksums inside.** When the archive holds a checksum list (a file
   named `*.sha256` or `SHA256SUMS` in the `sha256sum` format), every listed
   file is compared with it.

A check that has nothing to work on is shown as skipped.

`--key` names the key file for a file that opens with a key. When the key
file is itself protected by a passphrase, fastlock asks for it, or reads it
from `--passphrase-file`.

```
$ fastlock verify backup.tar.zst.age --key ~/keys/backup-key.age
Passphrase for the key backup-key.age:
✓ checksum file     matches backup.tar.zst.age.sha256
✓ decryption        authenticated to the last byte, 2.9 MiB
✓ compression       Zstandard, decompressed without errors
✓ archive           3 files read to the end
✓ checksums inside  2 files match the list inside

  CHECKSUMS.sha256                         185 B
  sample.dump                              2.9 MiB
  store.db                                 3.8 MiB

OK: backup.tar.zst.age is intact. Nothing was written.
```

Exit codes: 0 when every check passed, 2 when a key or a passphrase is
missing, 3 for a wrong key or passphrase, 4 when the file is damaged or a
checksum does not match, 5 when a file does not exist, 7 when you cancel.

##### `fastlock decrypt`

```
fastlock decrypt <file> [-o <path>] [--key <path>] [--passphrase-file <path>] [--force] [--json]
```

Decrypts the file to disk and runs the same checks as `verify`.

- An archive is unpacked into a folder. Other data is written as one file.
- Without `-o`, the output goes next to the encrypted file, under its name
  without the endings `.age`, `.zst` and `.tar`.
- The output is written into a temporary folder first and moved into place
  only after the whole file has been authenticated. A damaged file, a wrong
  key, or Ctrl+C leaves nothing behind.
- An existing file is replaced only with `--force`. An existing folder is
  never replaced.
- Folders and files are created readable by you only. Links and special
  files inside an archive are listed, but not written.

```
$ fastlock decrypt backup.tar.zst.age -o restored --key ~/keys/backup-key.age
```

Exit codes: as for `verify`, and 6 when the output already exists.

##### `fastlock tui`

```
fastlock tui [<folder>]
```

Opens the terminal interface in the given folder, or in the current one.

##### Environment variables

| Variable | Meaning |
|---|---|
| `FASTLOCK_KEY` | The path of a key file. The terminal interface offers it as the key file. It holds a path, never a key. |
| `FASTLOCK_AGE` | The path of the `age` program to use, instead of the one found on `PATH`. |
| `NO_COLOR` | When set and not empty, output has no colors. |

#### Flags that work everywhere

| Flag | Meaning |
|---|---|
| `--json` | Machine-readable output. |
| `-q`, `--quiet` | Print only errors. |
| `-v`, `--verbose` | Print more detail. |
| `--no-color` | No colors. The `NO_COLOR` environment variable does the same. |
| `--no-input` | Never prompt. Fail instead, and name the missing input. |
| `--yes` | Answer yes to confirmations. It never replaces a passphrase. |
| `--passphrase-file <path>` | Read the passphrase from a file instead of a prompt. |
| `--force` | Allow an existing file to be replaced. |

`--yes` is not implemented yet, because no command asks for a confirmation
yet.

#### Exit codes

These codes are fixed from the first release on and are never renumbered.

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | A failure not listed below. |
| 2 | The command was used wrongly. |
| 3 | No key fits, or the passphrase is wrong. |
| 4 | The file is damaged or was changed: authentication failed. |
| 5 | Not found: a file, a key, or a session. |
| 6 | Refused: the output exists, or a confirmation is needed. |
| 7 | Cancelled by you. |

### Terminal interface

The terminal interface is optional. It shows everything on one screen and
never asks you to "press Enter to continue".

- **Top line:** the program name, the current screen, and the session
  status.
- **Left:** a list. Groups have bold headings and a blank line between them.
  Each item shows a shortcut letter, a label, and its current state.
- **Right:** a live preview of the selected item. It changes as you move.
- **Bottom line:** the keys you can press now.

Search and forms open in place. Long operations show their progress. The
mouse works: click to select, click again to open, and use the wheel to
move.

Today the terminal interface offers `verify` and `decrypt`. You pick a file
from a list, see what it is before any key is asked for, enter the key file
and the passphrase in place, and watch the checks come in.

The keys follow Neovim:

| Key | Action |
|---|---|
| `j` `k` | Move down and up |
| `h` `l` | Go back and open |
| `gg` `G` | Jump to the first and the last item |
| `/` | Search the current list |
| `:` | Open a command line |
| `?` | Show help |
| `Esc` | Go back or cancel |
| `q` | Quit |

Arrow keys work too. Shortcut letters for menu items never collide with the
keys above.

### Desktop app

A desktop app is planned after the terminal interface, for Windows, macOS
and Linux. It is built with Electron so that it looks the same on all three.
It is only a front end: the work is done by the same native core that the
commands use. It will be packaged with `electron-builder`, and the
installers for Windows and macOS come with it.

Its look is that of a modern, calm desktop app: a sidebar, a centered
column, soft cards and generous spacing, in cold white and blue.

### Themes

Colors come from a theme file with about eight named colors: background,
text, secondary text, accent, selection, ok, warning and error. Without a
theme file, the terminal interface uses the colors of your terminal.
Today only the terminal's own colors are used: secondary text is the
terminal's text color, dimmed.

The format of the theme file is not decided yet.

## Security model

This section states intentions. It becomes a description of behavior only
when the code exists and has been tested. Each item has a number, so that
issues, reviews and commits can refer to it. Numbers are never reused.

### What fastlock is meant to guarantee

- **G1.** fastlock uses only published formats and established, reviewed
  libraries. It contains no cryptography of its own.
- **G2.** An operation is reported as successful only after the whole stream
  has been authenticated to its last byte.
- **G3.** Passphrases and private keys never appear in command-line
  arguments, environment variables, logs or error messages.
- **G4.** fastlock sends nothing over the network.
- **G5.** A key is never stored only in a form that fastlock alone can read.

### What fastlock does not protect

- **N1. That a file exists, and how big it is.** Encryption hides the
  contents. It does not hide the file itself, its size, or when it changed.
- **N2. Data while a session is open.** Decrypted contents and keys are in
  memory then. Other software running under your account, or with
  administrator rights, can read them.
- **N3. Deleted files on an SSD.** Deleting a file does not guarantee that
  the data is gone from the drive. fastlock does not claim to erase
  anything. Use full-disk encryption for that.
- **N4. A weak or reused passphrase.**
- **N5. TOTP keys kept next to your passwords.** A second factor that lives
  on the same device as the first one protects less.
- **N6. Classic keys against a future quantum computer.** Only post-quantum
  age keys are designed for that. SSH keys and classic age keys are not.
  Until fastlock can make post-quantum age keys itself, new age keys are
  classic by default. `fastlock key new --pq` makes a post-quantum one, and
  needs the `age` program for it.
- **N7. Text typed into the desktop app.** The desktop app is a web-based
  window. Text typed into such a window passes through memory that fastlock
  cannot clear. How passphrases are entered in the desktop app is not
  decided yet.
- **N8. A replaced `age` program.** Files that open with a key are decrypted
  by the `age` program found on your computer, and that program receives
  your key. fastlock does not check that it is the genuine one. Install
  `age` from a source you trust.
- **N9. A replaced `gpg` program.** Private keys are imported from GnuPG
  through the `gpg` program found on your computer, and that program hands
  your keys to fastlock. fastlock does not check that it is the genuine one.
  Install GnuPG from a source you trust.
- **N10. RSA keys and timing.** The RSA code that fastlock uses for SSH and
  PGP keys has a known timing weakness, the Marvin attack
  (RUSTSEC-2023-0071). Someone who can make fastlock decrypt many prepared
  files and measure exactly how long each one takes could learn the private
  key. fastlock only decrypts when you ask it to, which makes this hard, but
  RSA keys are weaker here than ed25519 and X25519 keys. Prefer those.

### Audit status

fastlock has not been audited. Audits of the libraries it will use are
audits of those libraries, not of fastlock.

### Reporting a problem

See [Security problems](CONTRIBUTING.md#security-problems) in
CONTRIBUTING.md. Do not open a public issue.

## Glossary

- **age:** a modern, simple file encryption format and tool.
- **Lossless:** compression that gives back exactly the original data.
- **LZMA:** a compression method that makes very small files and takes
  longer.
- **OpenPGP (PGP):** a long-established standard for encrypting and signing
  files and messages. GnuPG is its best-known implementation.
- **Passphrase:** a password, usually a long one, that protects a file or a
  key.
- **Post-quantum:** designed to stay secure against a future quantum
  computer.
- **Session:** the time between opening an encrypted file and closing it.
- **TOTP:** time-based one-time passwords, the six-digit codes used as a
  second factor when you log in.
- **Zstandard (zstd):** a fast compression method with strong results.
