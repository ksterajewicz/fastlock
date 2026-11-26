# fastlock

Fast, open-source file encryption and key management from the command line.

> **Status:** early development. Nothing is released yet, and nothing has
> been audited.

fastlock brings several encryption tools together in one program. It is
written in Rust, which makes it fast and reliable. It builds on established
libraries, written in Rust or in C, and on established formats instead of
inventing its own.

## Goals

- **One tool, several formats.** age, OpenPGP and SSH keys in one place.
- **Compress and encrypt in one step.** Modern lossless compression that
  shrinks your data without changing a byte of it, combined with the modern
  encryption you choose.
- **Nothing else to install.** fastlock is one program file, with its
  libraries built in, so it does not need GnuPG or other tools. For now,
  post-quantum age keys are the one exception: they need the separate `age`
  program.
- **Your keys stay yours.** fastlock keeps every key in a standard format,
  SSH keys in `~/.ssh` and PGP keys as OpenPGP files, so you can open them
  with any other program at any time. It can import your keys from `~/.ssh`
  and from GnuPG.
- **Take your keys with you.** Export and import the keys you have saved.
  Store your TOTP keys in the same place and read your codes from it.
- **Quick messages.** Encrypt and decrypt short text messages with PGP keys,
  without creating a file.
- **Commands first.** Everything is a command. A terminal interface is
  optional, and a desktop app is planned for Linux, Windows and macOS.

## Installation

There is no release yet. On Linux, fastlock is installed from source. You
need:

- [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer, and a C
  compiler such as `gcc`, because the Zstandard library is built from C.
- [age](https://github.com/FiloSottile/age) 1.3 or newer, to open files that
  use a key. Files protected by a passphrase work without it. This is
  temporary: later, only post-quantum keys will need it.

```
git clone https://github.com/ksterajewicz/fastlock
cd fastlock
./install.sh
```

`install.sh` builds fastlock and installs it into `~/.local/bin`. When that
folder is not on your `PATH`, it prints the line to add. It does not edit
your shell files and never touches your keys. The first build downloads the
dependencies and takes a few minutes.

| Command | What it does |
|---|---|
| `./install.sh --prefix /usr/local` | Install into `/usr/local/bin` instead. |
| `./install.sh --binary <file>` | Install a program you have already built. |
| `./install.sh --uninstall` | Remove the program. |
| `./install.sh --help` | Show all options. |

Running `./install.sh` again updates fastlock in place. A file that is not
fastlock is never replaced unless you add `--force`. To install for every
user, build as yourself, then install as root:

```
cargo build --release --locked
sudo ./install.sh --prefix /usr/local --binary target/release/fastlock
```

Windows and macOS are not supported yet.

## Commands

Working now: `inspect`, `verify`, `decrypt`, `tui`, `help` and `version`.
The rest is designed and not implemented yet. Details are in
[DOCS.md](DOCS.md#commands).

| Command | What it does |
|---|---|
| `fastlock encrypt` | Encrypt a file, or pack, compress and encrypt a folder |
| `fastlock decrypt` | Decrypt a file to disk and unpack it |
| `fastlock inspect` | Show what a file is, without needing a key |
| `fastlock verify` | Check a file in memory, writing nothing |
| `fastlock open` | Open a session: work with the contents in memory |
| `fastlock close` | Close a session and wipe what was decrypted |
| `fastlock status` | List open sessions |
| `fastlock key list` | List your keys |
| `fastlock key new` | Create a key |
| `fastlock key show` | Show one key |
| `fastlock key import` | Add a key from a file |
| `fastlock key export` | Write a public key to give to someone |
| `fastlock key remove` | Remove a key |
| `fastlock totp list` | List your TOTP keys |
| `fastlock totp add` | Add a TOTP key |
| `fastlock totp code` | Print the current code |
| `fastlock totp remove` | Remove a TOTP key |
| `fastlock backup` | Save all keys and TOTP keys into one encrypted file |
| `fastlock restore` | Bring them back from such a file |
| `fastlock tui` | Open the terminal interface |
| `fastlock help` | Show help |
| `fastlock version` | Print the version |
| `fastlock completions` | Print shell completions |

## Documentation

See [DOCS.md](DOCS.md).

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md)
first. It explains the rules, including how commits are written.

## License

fastlock is free software, released under the
[GNU General Public License v3.0 or later](LICENSE). It is open source and
will stay that way.

The libraries built into fastlock, their licenses and their authors are
listed in [THIRD-PARTY-LICENSES](THIRD-PARTY-LICENSES).
