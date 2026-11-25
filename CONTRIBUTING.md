# Contributing to fastlock

Thank you for wanting to help. This file is for people who contribute to
fastlock. It explains what to work on, how to send a change, and how commits
are written.

- Rules for AI coding agents working in this repository are in
  [AGENTS.md](AGENTS.md).
- User documentation and the design of the program are in [DOCS.md](DOCS.md).

fastlock is at an early stage. Only a first part of it exists as code.

## The one rule

You must understand every line you submit and be able to explain it. This
holds whether you typed the change yourself or an AI tool wrote it for you.

## What to work on

- Issues labeled `good first issue` or `help wanted` are open to everyone.
  You do not need permission to start. Leave a comment first, so that two
  people do not solve the same problem.
- For anything else, open an issue and describe the idea before you write
  code. A change that arrives without discussion may be closed, even when it
  is well made.
- Found a bug? Open an issue that says what you did, what you expected, what
  happened instead, which version you ran, and on which operating system.

## Using AI tools

AI tools are welcome. Much of fastlock is written with them.

- **A person is responsible.** Someone who understands the change reviews it
  before it is sent and answers questions about it afterwards. Changes from
  unsupervised agents that run in a loop with nobody reviewing are not
  accepted.
- **Say that you used one.** Name the tool in the pull request, or keep the
  `Co-Authored-By:` line it adds to the commit.
- **Check where the code comes from.** Do not submit generated code that
  copies someone else's work under a license that does not fit ours.

An agent that works inside the repository follows [AGENTS.md](AGENTS.md).

## Sending a change

1. Fork the repository and create a branch from `dev`.
2. Keep each pull request about one thing. Do not mix a bug fix with a
   refactor or a new feature.
3. Add tests. A bug fix comes with a test that fails without the fix. A new
   feature comes with tests for what it does.
4. Update the documentation in the same change. A new or changed command
   updates the command list in [README.md](README.md) and its entry in
   [DOCS.md](DOCS.md).
5. Write everything in English: code, comments, documentation, and commit
   messages.
6. Open the pull request against `dev`. Say what the change does and why.

The maintainer reviews every change. Opening a pull request does not
guarantee that it will be merged.

## Commit messages

fastlock uses [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
This section is the reference for the convention.

```
<type>(<optional scope>): <summary>

<optional body>

<optional footer>
```

| Type       | Use it for                                        |
|------------|---------------------------------------------------|
| `feat`     | a new capability                                  |
| `fix`      | a bug fix                                         |
| `docs`     | documentation only                                |
| `refactor` | a change that neither fixes a bug nor adds a feature |
| `test`     | adding or correcting tests                        |
| `perf`     | a change that makes something faster              |
| `build`    | the build system or dependencies                  |
| `ci`       | continuous integration                            |
| `chore`    | maintenance that fits none of the above           |

- **Scope** is optional. It names the part of the project that changed.
- **Summary** is in the imperative mood ("add", not "added" or "adds"),
  starts with a lowercase letter, has no period at the end, and stays within
  72 characters.
- **Body** is optional. Use it to say why the change was made.
- **Breaking changes** get a `!` after the type or scope, and a footer that
  starts with `BREAKING CHANGE:` and explains what breaks.
- One commit holds one logical change.

Examples:

```
docs: add README, AGENTS.md and LICENSE
feat(tui): add key list
fix(core): reject a file whose last chunk fails authentication
feat(cli)!: require --force to replace an existing file

BREAKING CHANGE: commands no longer overwrite an existing output file.
```

## What will not be merged

- New cryptographic primitives, home-made cryptography, or a new file format.
  fastlock uses established, reviewed libraries and published formats only.
- A change that weakens a default, or that lets people pick a weaker cipher.
- Anything that sends data over the network, including telemetry and update
  checks.
- A feature that exists only in an interface. Every action must also be a
  command.
- A large refactor that was not discussed first.
- A change with real logic in it and no tests.
- Work that its author cannot explain.

## Security problems

Do not report a security problem in a public issue. Report it privately
through GitHub: open the **Security** tab of the repository and choose
**Report a vulnerability**.

If an AI tool found the problem, say so in the report, and confirm the
problem yourself before you send it.

## Building and testing

You need Rust 1.88 or newer and a C compiler, because some of the
libraries fastlock uses are written in C. The end-to-end tests also use the `age` and
`age-keygen` programs, version 1.3 or newer; without them those tests skip
themselves.

Run these three before you send a change. All of them must pass without
warnings:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

A change to `install.sh` also needs these two, and
[ShellCheck](https://www.shellcheck.net/) for the first one. The tests use
stand-in programs, work offline, and take a few seconds;
`tests/install-test.sh --build` also builds and installs the real program.

```
shellcheck install.sh tests/install-test.sh
tests/install-test.sh
```

The code lives in three crates:

- `crates/fastlock-core`: everything that reads, checks and decrypts. No
  interface code.
- `crates/fastlock-cli`: the `fastlock` program. It parses arguments, asks
  for what is missing, and prints results.
- `crates/fastlock-tui`: the terminal interface.

Tests use invented keys and invented data, created while the test runs.
Never put a real key or real data into a test.

## License and credit

- fastlock is released under the
  [GNU General Public License v3.0 or later](LICENSE). By contributing, you
  agree that your contribution is released under the same license.
- There is no contributor agreement to sign. You keep the copyright to your
  work.
- Submit only work that you have the right to submit: your own, or work
  under a license compatible with ours.
- Your authorship stays in the history of the project. When a change is
  reworked before it is merged, the original author is kept with a
  `Co-authored-by:` line.
