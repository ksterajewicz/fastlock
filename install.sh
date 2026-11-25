#!/usr/bin/env bash
#
# Builds fastlock from this source tree and installs the `fastlock` program,
# or removes it again.
#
#   ./install.sh                      build, then install into ~/.local/bin
#   ./install.sh --prefix /usr/local  install into /usr/local/bin instead
#   ./install.sh --binary <file>      install a program that is already built
#   ./install.sh --uninstall          remove the installed program
#
# What it never does: it does not edit shell startup files, it does not touch
# keys or any other data, and it does not replace a file that is not fastlock
# unless --force is given. The build itself is done by Cargo, which downloads
# the dependencies listed in Cargo.lock the first time.
#
# Exit codes follow the table of the fastlock program (DOCS.md, "Exit codes"):
# 0 success, 1 failure, 2 wrong use, 5 not found, 6 refused.

set -euo pipefail

readonly MIN_RUST="1.88"
readonly MIN_AGE="1.3"
readonly NAME="fastlock"

SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SOURCE_DIR

prefix="${HOME}/.local"
binary=""
force=0
uninstall=0
temp_file=""

usage() {
    cat <<'EOF'
Usage: ./install.sh [options]

Builds fastlock from this source tree and installs the `fastlock` program.

Options:
  --prefix <dir>   Install into <dir>/bin. The default is ~/.local.
  --binary <file>  Install this already built program instead of building.
  --force          Replace a file that is not fastlock at the destination.
  --uninstall      Remove the installed program. Keys are never touched.
  -h, --help       Show this help.

To install for every user, build as yourself first, then install as root:
  cargo build --release --locked
  sudo ./install.sh --prefix /usr/local --binary target/release/fastlock
EOF
}

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
fail() {
    local code="$1"
    shift
    printf 'error: %s\n' "$*" >&2
    exit "$code"
}

cleanup() {
    if [[ -n "${temp_file}" && -e "${temp_file}" ]]; then
        rm -f -- "${temp_file}"
    fi
}
trap cleanup EXIT

# version_at_least <found> <wanted>: true when found >= wanted. Both are
# dotted numbers; missing parts count as zero.
version_at_least() {
    local -a found wanted
    local i a b
    IFS=. read -r -a found <<<"$1"
    IFS=. read -r -a wanted <<<"$2"
    for i in 0 1 2; do
        a="${found[i]:-0}"
        b="${wanted[i]:-0}"
        [[ "$a" =~ ^[0-9]+$ ]] || a=0
        [[ "$b" =~ ^[0-9]+$ ]] || b=0
        if ((10#$a > 10#$b)); then return 0; fi
        if ((10#$a < 10#$b)); then return 1; fi
    done
    return 0
}

# first_version <text>: prints the first dotted number found in the text.
first_version() {
    grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?' <<<"$1" | head -n 1 || true
}

# fastlock_version <file>: prints "fastlock X.Y.Z" when the file is the
# fastlock program, and nothing otherwise.
fastlock_version() {
    local out
    local -a runner=()
    # A file that is not fastlock may not understand `version` and keep
    # running; give it a few seconds at most.
    if command -v timeout >/dev/null 2>&1; then
        runner=(timeout 10)
    fi
    out="$("${runner[@]}" "$1" version 2>/dev/null </dev/null | head -n 1)" || return 0
    if [[ "$out" =~ ^${NAME}\ [0-9] ]]; then
        printf '%s\n' "$out"
    fi
}

parse_args() {
    while (($# > 0)); do
        case "$1" in
            --prefix)
                (($# >= 2)) || fail 2 "--prefix needs a folder"
                prefix="$2"
                shift 2
                ;;
            --prefix=*)
                prefix="${1#--prefix=}"
                shift
                ;;
            --binary)
                (($# >= 2)) || fail 2 "--binary needs a file"
                binary="$2"
                shift 2
                ;;
            --binary=*)
                binary="${1#--binary=}"
                shift
                ;;
            --force)
                force=1
                shift
                ;;
            --uninstall)
                uninstall=1
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *)
                usage >&2
                fail 2 "unknown option: $1"
                ;;
        esac
    done
    [[ -n "${prefix}" ]] || fail 2 "--prefix needs a folder"
    if ((uninstall)) && [[ -n "${binary}" ]]; then
        fail 2 "--uninstall and --binary cannot be used together"
    fi
}

# Finds Cargo. A Rust toolchain installed by rustup lives in ~/.cargo/bin,
# which is not always on PATH, so that folder is tried as well.
find_cargo() {
    if ! command -v cargo >/dev/null 2>&1 && [[ -x "${HOME}/.cargo/bin/cargo" ]]; then
        PATH="${HOME}/.cargo/bin:${PATH}"
        export PATH
    fi
    command -v cargo >/dev/null 2>&1 ||
        fail 5 "Cargo was not found. Install Rust ${MIN_RUST} or newer: https://www.rust-lang.org/tools/install"
    command -v rustc >/dev/null 2>&1 ||
        fail 5 "rustc was not found next to Cargo. Install Rust ${MIN_RUST} or newer."
}

build() {
    if ((EUID == 0)) && [[ -n "${SUDO_USER:-}" ]]; then
        fail 2 "do not build with sudo. Build as yourself, then install with sudo and --binary target/release/${NAME}"
    fi
    find_cargo

    local rust_version
    rust_version="$(first_version "$(rustc --version)")"
    if [[ -z "${rust_version}" ]] || ! version_at_least "${rust_version}" "${MIN_RUST}"; then
        fail 1 "fastlock needs Rust ${MIN_RUST} or newer, and this is ${rust_version:-an unknown version}. Run: rustup update"
    fi
    command -v cc >/dev/null 2>&1 ||
        warn "no C compiler (cc) was found; the Zstandard library needs one to build"

    say "Building ${NAME} with Rust ${rust_version}. The first build takes a few minutes."
    (cd "${SOURCE_DIR}" && cargo build --release --locked --package fastlock-cli) ||
        fail 1 "the build failed; the messages above say why"

    binary="${CARGO_TARGET_DIR:-${SOURCE_DIR}/target}/release/${NAME}"
}

# Warns when the program will not be found by name, or when another program
# of the same name comes first.
check_path() {
    local bindir="$1" found
    case ":${PATH}:" in
        *":${bindir}:"*) ;;
        *)
            warn "${bindir} is not on your PATH, so typing \`${NAME}\` will not find it yet."
            say "  Add this line to your shell startup file (for bash: ~/.bashrc), then open a new terminal:"
            # The line is printed for the reader to copy; it must stay literal.
            # shellcheck disable=SC2016
            say "    export PATH=\"${bindir}:\$PATH\""
            return 0
            ;;
    esac
    found="$(command -v "${NAME}" 2>/dev/null || true)"
    if [[ -n "${found}" && "${found}" != "${bindir}/${NAME}" ]]; then
        warn "another ${NAME} comes first on your PATH: ${found}"
    fi
}

# Files that open with a key need the official age program (DOCS.md,
# "Commands that work now"). Its absence is not an error: passphrase files
# work without it.
check_age() {
    local age_program="" age_version
    if [[ -n "${FASTLOCK_AGE:-}" ]]; then
        age_program="${FASTLOCK_AGE}"
    else
        age_program="$(command -v age 2>/dev/null || true)"
    fi
    if [[ -z "${age_program}" || ! -x "${age_program}" ]]; then
        warn "the age program was not found. Files protected by a passphrase work without it;"
        warn "files that open with a key need age ${MIN_AGE} or newer: https://github.com/FiloSottile/age"
        return 0
    fi
    age_version="$(first_version "$("${age_program}" --version 2>/dev/null || true)")"
    if [[ -z "${age_version}" ]] || ! version_at_least "${age_version}" "${MIN_AGE}"; then
        warn "${age_program} is version ${age_version:-unknown}; files that open with a key need age ${MIN_AGE} or newer"
        return 0
    fi
    say "Found age ${age_version} at ${age_program}."
}

do_install() {
    local bindir="${prefix}/bin" target new_version old_version

    if [[ -z "${binary}" ]]; then
        build
    fi
    [[ -e "${binary}" ]] || fail 5 "no such file: ${binary}"
    [[ -f "${binary}" && -x "${binary}" ]] || fail 1 "not an executable file: ${binary}"
    new_version="$(fastlock_version "${binary}")"
    [[ -n "${new_version}" ]] || fail 1 "${binary} is not the fastlock program"

    target="${bindir}/${NAME}"
    old_version=""
    local replaced_other=0
    if [[ -e "${target}" || -L "${target}" ]]; then
        old_version="$(fastlock_version "${target}")"
        if [[ -z "${old_version}" ]]; then
            ((force)) ||
                fail 6 "${target} exists and is not fastlock. Nothing was changed. Use --force to replace it."
            replaced_other=1
        fi
    fi

    mkdir -p -- "${bindir}" || fail 1 "cannot create ${bindir}"
    # Copy next to the target first, then rename: the rename is atomic, so the
    # target is never a half-written file.
    temp_file="$(mktemp "${bindir}/.${NAME}.XXXXXX")" || fail 1 "cannot write into ${bindir}"
    cp -- "${binary}" "${temp_file}"
    chmod 0755 -- "${temp_file}"
    mv -f -- "${temp_file}" "${target}"
    temp_file=""

    [[ "$(fastlock_version "${target}")" == "${new_version}" ]] ||
        fail 1 "${target} was written but does not run as expected"

    if [[ -n "${old_version}" ]]; then
        say "Replaced ${old_version} with ${new_version} at ${target}."
    elif ((replaced_other)); then
        say "Installed ${new_version} at ${target}, replacing the file that was there."
    else
        say "Installed ${new_version} at ${target}."
    fi
    check_age
    check_path "${bindir}"
    say "Run \`${NAME} help\` to start."
}

do_uninstall() {
    local target="${prefix}/bin/${NAME}"
    if [[ ! -e "${target}" && ! -L "${target}" ]]; then
        fail 5 "${NAME} is not installed in ${prefix}/bin"
    fi
    if [[ -z "$(fastlock_version "${target}")" ]] && ((!force)); then
        fail 6 "${target} is not fastlock. Nothing was removed. Use --force to remove it."
    fi
    rm -f -- "${target}" || fail 1 "cannot remove ${target}"
    say "Removed ${target}."
    say "Your keys and files were not touched."
}

main() {
    parse_args "$@"
    if ((uninstall)); then
        do_uninstall
    else
        do_install
    fi
}

main "$@"
