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

Not available yet.

## Documentation

See [DOCS.md](DOCS.md).

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md)
first. It explains the rules, including how commits are written.

## License

fastlock is free software, released under the
[GNU General Public License v3.0 or later](LICENSE). It is open source and
will stay that way.
