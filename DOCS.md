# fastlock documentation

> **Status, 2026-11-24:** fastlock is being designed. No code exists yet,
> nothing can be installed, and nothing has been audited. This file describes
> how the program is meant to work. Do not rely on it to protect anything
> until a release says otherwise.

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
  keys you have saved and import them again.
- It stores TOTP keys and shows the current codes.
- Later, it will encrypt and decrypt short text messages with PGP keys,
  without creating a file.

There are three ways to use it:

1. **Commands.** Everything fastlock can do is a command.
2. **Terminal interface.** An optional interface inside the terminal. It
   runs the same commands.
3. **Desktop app.** Planned for later. It also runs the same commands.

fastlock is written in Rust. It is built for Linux first, then for Windows
and macOS.

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
| An age key | The holder of that key | New age keys are post-quantum by default. |
| An SSH key | The holder of that SSH key | Useful when someone already has an SSH key. No post-quantum protection. |
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

fastlock keeps keys in the standard locations of each tool, such as `~/.ssh`
for SSH and `~/.gnupg` for GnuPG. fastlock is never the only program that can
read a key. If you stop using fastlock, your keys still work everywhere else.

Where age keys live, and how the GnuPG keyring is read and written, is not
decided yet.

### TOTP

fastlock stores TOTP keys encrypted and shows the current codes. TOTP keys
can be exported and imported together with your other keys. Read
[N5](#what-fastlock-does-not-protect) before you keep them on the same
device as your passwords.

## Interfaces

### Commands

The commands below were designed on 2026-11-24. None of them is implemented
yet, and details can still change before the first release. A full entry for
each command, with its synopsis, options and examples, is added when the
command is implemented.

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
| `fastlock key new <name>` | Creates a key. The default is a post-quantum age key. `--type ssh` and `--type pgp` create the other kinds, and `--classic` makes a classic age key. |
| `fastlock key show <name>` | Shows one key: type, public key, fingerprint, and where it is stored. |
| `fastlock key import <file>` | Adds a key from a file: your own, or someone's public key. |
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
commands use.

Its look is that of a modern, calm desktop app: a sidebar, a centered
column, soft cards and generous spacing, in cold white and blue.

### Themes

Colors come from a theme file with about eight named colors: background,
text, secondary text, accent, selection, ok, warning and error. Without a
theme file, the terminal interface uses the colors of your terminal.

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
- **N7. Text typed into the desktop app.** The desktop app is a web-based
  window. Text typed into such a window passes through memory that fastlock
  cannot clear. How passphrases are entered in the desktop app is not
  decided yet.

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
