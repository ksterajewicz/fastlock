# AGENTS.md

Instructions for AI agents (Claude Code, Codex, Copilot, Cursor, and others)
working on this repository. This file is the single source of such
instructions in the repo. There is no `CLAUDE.md` or other per-tool file; if
your agent reads a different format, point it here.

Status of this file: written before the first line of code. It records what
has been decided, what is planned, and what is still open, so that no agent
has to guess. When a decision changes, update this file in the same commit.

## What fastlock is

fastlock is an open-source program for encrypting and decrypting files. It is
meant to be universal: one tool that reads and writes several established
encrypted formats, the way an archiver handles several archive formats.

It works on a simple cycle: open an encrypted file, work with the contents,
close it, and wipe what was decrypted.

The program is its commands. A person must be able to do everything from the
command line, as with other archivers and encryption tools. The terminal
interface (TUI) is an optional layer on top of the same commands: people may
use it, and nobody has to. A graphical interface for Windows is planned later
and follows the same rule.

## Language rule: English only

This is an English-language repository.

- Every commit must be English only: code, comments, identifiers,
  documentation, commit messages, file names, test data, examples.
- The maintainer writes some planning notes in Polish, including the first
  draft of `README.md`. Polish text is never committed. The procedure is:
  1. read and understand the Polish text,
  2. translate it into English,
  3. show the translation to the maintainer,
  4. commit only after the maintainer accepts it.
- If you find Polish text in the working tree, do not commit it and do not
  silently rewrite it. Translate it and ask.
- The interface text of the program is English. Translations of the interface
  into other languages are a later topic and are not decided.

## Git and approval rules

- The maintainer creates and owns the git repository. Do not run `git init`,
  do not create remotes, and do not push.
- Ask the maintainer before every commit, every time. Show the exact content
  and the commit message, and commit only after a clear yes. An earlier
  approval never covers a later commit.
- Commit messages follow Conventional Commits. See "Commit messages" below.
- Never commit secrets, private keys, passphrases, or real personal data. Use
  invented sample data in tests, examples, and screenshots.
- Do not install packages, toolchains, or system software without asking.

## Commit messages

This repository uses [Conventional Commits](https://www.conventionalcommits.org/).
Every commit message, from people and agents alike, has this shape:

```
<type>(<optional scope>): <summary>

<optional body>

<optional footer>
```

- **Type** is one of: `feat` (a new capability), `fix` (a bug fix), `docs`,
  `refactor`, `test`, `perf`, `build`, `ci`, `chore`.
- **Scope** is optional and names the part of the project that changed, for
  example `docs: add README` or `feat(tui): add key list`.
- **Summary** is in English, in the imperative mood ("add", not "added" or
  "adds"), starts with a lowercase letter, has no period at the end, and stays
  within 72 characters.
- **Body** is optional. Use it to say why the change was made when the
  summary cannot.
- **Breaking changes** are marked with `!` after the type or scope, and
  explained in a footer that starts with `BREAKING CHANGE:`.
- One commit holds one logical change. Do not mix a refactor with a feature.

`CONTRIBUTING.md` will carry the same rules for human contributors. It is not
written yet; this section is the reference until it is.

## Decided

Each item below was decided by the maintainer on 2026-11-24.

- **Name:** `fastlock`. It is also the command typed in a terminal.
- **Language:** Rust, chosen for speed and reliability.
- **Repository:** a separate, standalone repository.
- **Command line first:** every action available in an interface must also be
  available as a command. Interfaces call the same core; they contain no
  encryption logic of their own.
- **First interface:** a TUI for Linux.
- **TUI toolkit:** `ratatui` with `crossterm`.
- **TUI look:** header line with the program name, the current screen and the
  session status; a list on the left; a live preview of the selected item on
  the right; a line of key hints at the bottom. Groups are separated by a
  blank line and have bold headings. Each item shows a shortcut letter in the
  accent color, a label, and its current state in a quieter color. The
  selected row is bold white text on blue.
- **TUI interaction:** everything happens on one screen with no "press Enter
  to continue" steps. The preview updates while the selection moves. The mouse
  works: click, click again to open, wheel to move. Search and forms are
  in place. Long operations show progress.
- **Keys follow Neovim:** `j`/`k` move, `h`/`l` go back and open, `gg`/`G`
  jump to first and last, `/` searches, `:` opens a command line, `?` shows
  help, `Esc` goes back, `q` quits. Vim keys take priority; shortcut letters
  for menu items are chosen only from letters that these keys do not use.
  Arrow keys work as well.
- **Colors come from a theme file:** a small file with about eight named
  colors (background, text, secondary text, accent, selection, ok, warning,
  error). Without a theme file the program uses the colors of the terminal.
  This lets a desktop theme ship one fastlock theme file per theme.
- **Key types:** age keys, both classic (X25519) and post-quantum
  (ML-KEM-768 + X25519), OpenPGP keys, and SSH keys.
- **SSH keys are managed inside the program.** What exactly "managed" covers
  is open (see below).
- **Keys live in the standard locations.** fastlock stores everything in the
  default folders of each tool (SSH, GnuPG, and so on), so a person can always
  open their keys with another program. fastlock must never be the only
  program that can read a key.
- **Documentation layout:** `README.md` is short, tidy and to the point. It
  links to `DOCS.md` (the documentation), `CONTRIBUTING.md` (the rules for
  contributors, including the commit convention) and `LICENSE`. It also holds
  the installation instructions and the full list of commands. Both sections
  are placeholders until the commands are designed with the maintainer; fill
  them in then.
- **Contributions from other people are welcome.**
- **License:** GNU General Public License v3.0 or later
  (`GPL-3.0-or-later`). It was chosen to meet three requirements from the
  maintainer: anyone can contribute, authorship is credited, and the software
  can never be turned into a closed product. It must stay open source
  forever. The full text is in `LICENSE`. Contributions are accepted under
  the same license. Do not add code or dependencies whose license is not
  compatible with it.
- **Key export and import, and TOTP keys (planned).** A person can export the
  keys they have saved and import them again, and can add TOTP keys. The
  details are open (see below).
- **Compression and encryption in one step.** fastlock packs files and
  folders, compresses them, and encrypts them in a single operation. The
  compression is modern and lossless: it shrinks data strongly and every file
  comes back identical. The encryption is modern.
- **Compression methods:** Zstandard is the default. LZMA is offered as the
  choice for the smallest possible result. Compression can also be turned
  off.
- **Choosing encryption means choosing who can open the file:** a
  passphrase, an age key (post-quantum by default), an SSH key, or a PGP key.
  The cipher itself is fixed by the format. fastlock does not offer a list of
  ciphers to pick from.
- **TOTP keys:** fastlock stores them encrypted, shows the current codes, and
  can export and import them.
- **Quick message encryption (planned for later):** a fast way to encrypt
  and decrypt short text messages with PGP keys, without creating a file. A
  message is encrypted for the public key of the person who should read it,
  and decrypted with the reader's own private key. This is an idea for a
  later stage; do not start it before the file features exist.
- **Platforms:** fastlock must run on Linux, Windows, and macOS. Work starts
  on Linux.
- **A desktop GUI is planned** after the Linux TUI, built with Electron, for
  all three platforms.

## Assumed so far, not yet confirmed one by one

These come from the planning research and nothing has contradicted them, but
the maintainer has not confirmed each of them explicitly. Treat them as the
working plan and ask before building on any of them in a way that is hard to
undo.

- Layout: a core library with no interface code, a command-line program, and
  interfaces as thin layers over the core.
- Formats are compiled-in modules behind one common interface, not plugins
  loaded at run time.
- New files are encrypted in the age format by default.
- A "session" is an opened encrypted file with a lifetime. Closing a session
  removes everything that was decrypted and clears keys from memory. A session
  also closes by itself after a period without activity, when the screen
  locks, and before the machine sleeps.
- Verifying a file decrypts it in memory and writes nothing to disk.
- The original file is never removed before the encrypted result has been
  verified.

## Rules that are not negotiable

- **No home-made cryptography and no new file format.** Use established,
  reviewed libraries and published formats only.
- Passphrases and private keys never appear in command-line arguments,
  environment variables, logs, or error messages.
- A result is reported as successful only after the whole stream has been
  authenticated to its last byte.
- Never claim that deleting a file erases it from an SSD. Say what the
  program actually does.
- The program does not send anything over the network.

## Open questions

Do not settle these on your own. If a task depends on one of them, stop and
ask the maintainer.

1. **Key export, import and TOTP.** What an export file looks like and how
   it is protected, which key types it covers, and where TOTP keys are
   stored.
2. **Post-quantum age keys in Rust.** Two options are on the table: calling
   the official `age` program for this one key type in the first version, or
   a small module built on standard building blocks and checked against the
   official test vectors.
3. **SSH key management.** Which actions exist (list, create, import, export
   the public key, delete). Keys live in `~/.ssh` (decided). The limits of
   SSH keys for file encryption must be visible to the person using them.
4. **Compression details.** Which compression levels are offered and which
   one is the default. Whether fastlock also reads archives made by other
   tools is not decided.
5. **Closing and auto-closing.** Exact timings and what happens when the
   terminal is closed or the connection drops.
6. **Theme creation.** Whether the program has its own screen for creating a
   theme file or only reads one.
7. **The list of commands.** Names and arguments of the command-line
   interface are not designed yet.
8. **Desktop GUI.** The direction is decided (see the next section). Still
   open: the final look, how the window talks to the core, and how a
   passphrase is entered so that it stays out of the web layer.
9. **Standard locations in detail.** The age format has no single default
   key folder, so one must be chosen. GnuPG keeps its keys in its own
   keyring format, so how fastlock reads and writes there safely is not
   settled. The same question exists for Windows and macOS paths.
10. **`CONTRIBUTING.md` and `DOCS.md`.** Both are linked from `README.md` and
    neither is written yet.
11. **Quick messages.** Whether signing and verifying messages is part of
    it, and where the text comes from and goes to (terminal, clipboard).

## Planned: desktop GUI

A graphical interface is planned after the Linux TUI.

Decided by the maintainer on 2026-11-24:

- The GUI is built with Electron and ships for Windows, macOS, and Linux.
  Electron was chosen so that the interface looks the same on all three.
- The backend stays native and fast. The GUI is only a front end.
- The look is that of a modern, calm, very polished desktop app: a sidebar,
  a centered content column, soft cards, generous spacing. Colors stay cold
  white and blue.

What already follows from the decisions above:

- The GUI is another thin layer over the same core and the same commands.
- Anything that depends on one operating system must sit behind an interface
  in the core so that each platform can provide its own version: in-memory
  storage for decrypted files, locking memory, clearing memory, atomic file
  replacement, and the signals for screen lock and sleep.
- The commands and the TUI should run on Windows and macOS as well, because
  `crossterm` supports their terminals. This is a cheap first step before
  any GUI exists.

The final look and the link between the window and the core are being planned
with the maintainer. Do not start GUI work until this section says what was
chosen.

## How to work here

1. Read `README.md` and this file before changing anything.
2. If a task touches an open question, ask instead of choosing.
3. Check facts against the source: library documentation, format
   specifications, the code in this repository. Do not rely on memory for
   versions, licenses, or security properties.
4. Keep the public repository free of anything private: no real file names,
   paths, hosts, or data from anyone's machines.
5. When you finish a step, say what you verified and how.
