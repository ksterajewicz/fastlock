# fastlock

Fast, open-source file encryption and key management from the command line.

> **Status:** early development. Nothing is released yet.

fastlock brings several encryption tools together in one program. It is
written in Rust, which makes it fast and reliable, and it works with
established formats instead of inventing its own.

## Goals

- **One tool, several formats.** age, OpenPGP and SSH keys in one place.
- **Compress and encrypt in one step.** Modern lossless compression that
  shrinks your data without changing a byte of it, combined with the modern
  encryption you choose.
- **Your keys stay yours.** fastlock keeps keys in the standard locations of
  each tool, such as `~/.ssh` and `~/.gnupg`, so you can open them with any
  other program at any time.
- **Take your keys with you.** Export and import the keys you have saved.
  Store your TOTP keys in the same place and read your codes from it.
- **Quick messages.** Encrypt and decrypt short text messages with PGP keys,
  without creating a file.
- **Commands first.** Everything is a command. A terminal interface is
  optional, and a desktop app is planned for Linux, Windows and macOS.

## Installation

Not available yet.

## Commands

Designed, not implemented yet. Details are in [DOCS.md](DOCS.md#commands).

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
