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

The commands are not designed yet. The rule is already fixed: every action
in any interface is also a command.

When commands exist, each one gets an entry here with its synopsis, what it
does, its options, examples, and exit codes.

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
