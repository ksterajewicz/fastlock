# AGENTS.md

Rules for AI coding agents (Claude Code, Codex, Copilot, Cursor, and others)
working in this repository. This is the only file of its kind here. There is
no `CLAUDE.md` or other per-tool file. If your agent reads a different
format, point it at this file.

## Where things live

| File | Owns |
|---|---|
| `AGENTS.md` (this file) | The rules an agent follows, the decisions it must not contradict, and the questions it must not settle alone. |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | How changes are sent, the commit convention, what is not accepted, how to report a security problem. |
| [`DOCS.md`](DOCS.md) | What the program does, how each interface behaves, and the security model. |
| [`README.md`](README.md) | The short front page: goals, installation, the list of commands. |

One topic has one home. Link to it; do not copy it into another file.

Only the first part of fastlock exists as code: `inspect`, `verify` and
`decrypt`, and a terminal interface for them. Where a file here disagrees
with the code, the code is right: fix the file in the same change.

The code is a Cargo workspace of three crates. `crates/fastlock-core` reads,
checks and decrypts, and has no interface code. `crates/fastlock-cli` is the
`fastlock` program. `crates/fastlock-tui` is the terminal interface. The two
interfaces only call the core.

## What fastlock is

fastlock is an open-source program for encrypting and decrypting files and
for managing keys. It works on a simple cycle: open an encrypted file, work
with the contents, close it, and wipe what was decrypted.

The program is its commands. Everything a person can do, they can do from the
command line. The terminal interface and the planned desktop app are optional
layers over the same commands. [DOCS.md](DOCS.md) describes all of it.

## Language: English only

- Every commit is English only: code, comments, identifiers, documentation,
  commit messages, file names, test data, examples.
- The maintainer sometimes writes planning notes in Polish. Polish text is
  never committed. Read it, write the English version, show it to the
  maintainer, and commit only after the maintainer accepts it.
- If you find Polish text in the working tree, do not commit it and do not
  silently rewrite it. Translate it and ask.
- The interface text of the program is English.

## Git and approval

- Ask the maintainer before every commit, every time. Show the files, the
  branch and the full commit message, and commit only after a clear yes. An
  earlier approval never covers a later commit.
- Do not push, do not create remotes, and do not rewrite history.
- Commit messages follow Conventional Commits: `<type>(<scope>): <summary>`,
  in English, imperative, lowercase, no period, at most 72 characters. The
  full convention is in [CONTRIBUTING.md](CONTRIBUTING.md#commit-messages).
- One commit holds one logical change.
- Never commit secrets, private keys, passphrases, or real personal data.
  Use invented sample data in tests, examples and screenshots.
- Keep the repository free of anything private: no real file names, paths,
  hosts, or data from anyone's machines.

## Hard rules

These are the guarantees G1 to G5 in the
[security model](DOCS.md#security-model). If one of them ever changes, change
it here and there in the same commit.

- No home-made cryptography and no new file format. Use established,
  reviewed libraries and published formats only.
- A result is reported as successful only after the whole stream has been
  authenticated to its last byte.
- Passphrases and private keys never appear in command-line arguments,
  environment variables, logs, or error messages.
- The program sends nothing over the network.
- A key is never stored only in a form that fastlock alone can read.

Also: never claim that deleting a file erases it from an SSD. Say what the
program actually does.

## Ask first

Stop and ask the maintainer before you:

- do anything that depends on an open question (listed below),
- add a dependency, or install a package, toolchain or system software,
- change a decision, a hard rule, the license, or this file's rules,
- restructure the repository or rename files that other files link to,
- start work on the desktop app.

## Before you call a task done

1. Run the checks, all three, and show their output. They must pass without
   warnings:
   `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
   `cargo test --workspace`.
2. Show evidence for what you claim: the command you ran and its output, or
   the file and line. Do not rely on memory for versions, licenses, or
   security properties; check the source.
3. Update the documentation in the same change. See "Writing documentation".
4. Check that nothing private and no Polish text is in the change.
5. If a check fails, find out why. Do not assume it was already failing.
6. Ask before you commit.

## Decisions

Decided by the maintainer on 2026-11-24. Do not contradict them. Where a
decision describes behavior, the full description is in
[DOCS.md](DOCS.md).

- **Name:** `fastlock`, also the command typed in a terminal.
- **Language:** Rust, chosen for speed and reliability.
- **Commands first:** every action in any interface is also a command.
  Interfaces contain no encryption logic of their own.
- **Command list:** designed and listed in DOCS.md, "Commands", with the
  shared flags and exit codes. The main verbs are `encrypt` and `decrypt`.
  The whole key collection is saved with `backup` and `restore`. Do not
  rename, add or remove a command without asking.
- **Sessions:** `open` keeps running in its terminal until the session is
  closed; `--detach` puts it in the background. A session is a temporary
  copy: changes are never written back to the encrypted file.
- **Platforms:** Linux, Windows and macOS. Work starts on Linux.
- **First interface:** a terminal interface for Linux, built with `ratatui`
  and `crossterm`. Its layout, mouse behavior and Neovim-style keys are in
  DOCS.md, "Terminal interface".
- **Themes:** colors come from a theme file with about eight named colors.
  Without one, the terminal's own colors are used.
- **Desktop app:** planned after the terminal interface, built with
  Electron, for all three platforms. It is only a front end. Its look is in
  DOCS.md, "Desktop app".
- **Installers for Windows and macOS** (2026-11-25): made with
  `electron-builder` together with the desktop app, not before it.
- **Order of work** (2026-11-25): first installation on Linux
  (`install.sh`), then the core features as commands, then the terminal
  interface for all of them.
- **Key types:** age keys (classic and post-quantum), OpenPGP keys, and SSH
  keys. SSH keys are managed inside the program.
- **Keys live in the standard locations** of each tool, such as `~/.ssh` and
  `~/.gnupg`.
- **Encryption choice:** a person chooses who can open a file (passphrase,
  age key, SSH key, PGP key). There is no list of ciphers.
- **Compression:** pack, compress and encrypt in one step. Zstandard is the
  default, LZMA gives the smallest result, and compression can be off.
- **Key export and import:** planned.
- **TOTP keys:** stored encrypted, codes shown, export and import.
- **Quick messages with PGP keys:** planned for later. Do not start before
  the file features exist.
- **License:** `GPL-3.0-or-later`. Anyone can contribute, authorship is
  credited, and the software can never become a closed product. Do not add
  code or dependencies whose license is not compatible with it.
- **Documentation layout:** as in "Where things live". `README.md` stays
  short. Its "Installation" section is a placeholder until there is
  something to install.

## Working assumptions

These come from planning research. Nothing contradicts them, but the
maintainer has not confirmed each one. Ask before you build on any of them in
a way that is hard to undo.

- Layout: a core library with no interface code, a command-line program, and
  interfaces as thin layers over the core. The code is built this way.
- How files are opened today: a file that uses a key is decrypted by the
  official `age` program, which receives the key over a pipe. A file
  protected by a passphrase, and a key file protected by one, are decrypted
  with the `age` crate. This answers open question 2 for now, but the
  maintainer has not chosen it as the long-term way.
- Tests never use real keys or real data. They create both while they run.
- Formats are compiled-in modules behind one common interface, not plugins
  loaded at run time.
- New files are encrypted in the age format by default.
- A session closes by itself after a time without activity, when the screen
  locks, and before the machine sleeps.
- Verifying a file decrypts it in memory and writes nothing to disk.
- The original file is never removed before the encrypted result has been
  verified.
- Anything that depends on one operating system sits behind an interface in
  the core, so each platform can provide its own version: in-memory storage
  for decrypted files, locking and clearing memory, atomic file replacement,
  and the signals for screen lock and sleep.

## Open questions

Do not settle these on your own.

1. **Command details.** The full flags of each command, how sessions are
   named, and the shape of the JSON output.
2. **Post-quantum age keys in Rust.** Either call the official `age` program
   for this key type in the first version, or use a small module built on
   standard building blocks and checked against the official test vectors.
3. **SSH key management.** Which actions exist: list, create, import, export
   the public key, delete.
4. **Standard locations in detail.** Where age keys live, how the GnuPG
   keyring is read and written safely, and the paths on Windows and macOS.
5. **Key export, import and TOTP.** What an export file looks like, how it
   is protected, which key types it covers, and where TOTP keys are stored.
6. **Compression details.** Levels and the default level. Whether fastlock
   reads archives made by other tools.
7. **Sessions.** Auto-close timings, and where decrypted files are held on
   Windows and macOS.
8. **Themes.** The file format, and whether the program can create a theme
   file or only reads one.
9. **Desktop app.** The final look, how the window talks to the core, and
   how a passphrase is entered so that it stays out of the web layer.
10. **Quick messages.** Whether signing and verifying are part of it, and
    where the text comes from and goes to.
11. **Checks.** Whether the three check commands get one wrapper command,
    and the CI setup.
12. **Command reference.** Whether it is generated from the code, and
    whether it lives inside `DOCS.md` or in a separate generated file.

## Writing documentation

- Add a section only when there is real content for it. No empty headings.
- A new idea a user must understand goes under "Concepts" in `DOCS.md`. The
  behavior of an interface goes under "Interfaces".
- A change to what fastlock protects, or does not protect, updates the
  security model in the same change. Give a new item the next free number.
  Never reuse or renumber G and N items.
- When a command is implemented, it gets a full entry under "Commands" in
  `DOCS.md`: synopsis, what it does, options, examples, exit codes. Its line
  in the tables there and in `README.md` stays in step with the code.
- Exit codes live in one table and are never renumbered.
- Do not rename headings in `DOCS.md`. Other files link to them.
- State only what is decided or what the code does. Mark anything else as
  not decided.
