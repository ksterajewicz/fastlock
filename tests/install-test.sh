#!/usr/bin/env bash
#
# Tests for install.sh.
#
#   tests/install-test.sh          fast tests with stand-in programs, offline
#   tests/install-test.sh --build  also build the real program with Cargo and
#                                  install it into a temporary folder
#
# Every case runs in its own temporary folder, with a PATH that holds only
# the system tools install.sh needs and stand-ins for cargo, rustc, age and
# fastlock. Nothing outside the temporary folders is read or written, except
# that --build uses the Cargo build folder of this source tree.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT
readonly INSTALL="${ROOT}/install.sh"

real_build=0
if [[ "${1:-}" == "--build" ]]; then
    real_build=1
fi

passed=0
failed=0
current=""

WORK="$(mktemp -d)"
readonly WORK
trap 'rm -rf -- "${WORK}"' EXIT

# The system tools install.sh uses. Each case gets links to these only, so a
# real cargo, age or fastlock on this machine never changes a result.
readonly TOOLS=(bash env cat cp chmod mv rm mkdir mktemp grep head dirname timeout)

# --- helpers -----------------------------------------------------------------

# new_case <name>: starts a case with a fresh folder, HOME and PATH.
new_case() {
    current="$1"
    CASE="${WORK}/case$((passed + failed + 1))"
    mkdir -p "${CASE}/home" "${CASE}/sys" "${CASE}/fake"
    local tool path
    for tool in "${TOOLS[@]}"; do
        path="$(command -v "${tool}")" || {
            printf 'cannot run the tests: %s is missing\n' "${tool}" >&2
            exit 1
        }
        ln -s "${path}" "${CASE}/sys/${tool}"
    done
    CASE_PATH="${CASE}/fake:${CASE}/sys"
    PREFIX="${CASE}/prefix"
}

# fake_fastlock <file> <version>: writes a stand-in for the fastlock program.
fake_fastlock() {
    cat >"$1" <<EOF
#!/usr/bin/env bash
if [[ "\${1:-}" == version ]]; then echo "fastlock $2"; exit 0; fi
echo "stand-in fastlock" >&2
exit 2
EOF
    chmod 0755 "$1"
}

# fake_tool <name> <body>: writes a stand-in program into the case's PATH.
fake_tool() {
    printf '#!/usr/bin/env bash\n%s\n' "$2" >"${CASE}/fake/$1"
    chmod 0755 "${CASE}/fake/$1"
}

# run_install [args...]: runs install.sh in the case and keeps its results in
# CODE, OUT and ERR. Extra environment comes from the array EXTRA_ENV.
EXTRA_ENV=()
run_install() {
    env -i HOME="${CASE}/home" PATH="${CASE_PATH}" "${EXTRA_ENV[@]}" \
        bash "${INSTALL}" "$@" >"${CASE}/out" 2>"${CASE}/err"
    CODE=$?
    OUT="$(cat "${CASE}/out")"
    ERR="$(cat "${CASE}/err")"
}

check() {
    local description="$1"
    shift
    if "$@"; then
        return 0
    fi
    printf 'FAIL  %s: %s\n' "${current}" "${description}"
    printf '      exit code: %s\n      stdout: %s\n      stderr: %s\n' \
        "${CODE:-}" "${OUT:-}" "${ERR:-}"
    return 1
}

contains() { [[ "$1" == *"$2"* ]]; }
lacks() { [[ "$1" != *"$2"* ]]; }

# finish: records the result of the current case. Call it with the number of
# failed checks.
finish() {
    if (($1 == 0)); then
        passed=$((passed + 1))
        printf 'ok    %s\n' "${current}"
    else
        failed=$((failed + 1))
    fi
    EXTRA_ENV=()
}

# --- cases ---------------------------------------------------------------------

new_case "--help prints the usage and exits 0"
run_install --help
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "usage shown" contains "${OUT}" "Usage: ./install.sh" || f=$((f + 1))
finish $f

new_case "an unknown option exits 2"
run_install --frobnicate
f=0
check "exit 2" test "${CODE}" -eq 2 || f=$((f + 1))
check "names the option" contains "${ERR}" "unknown option: --frobnicate" || f=$((f + 1))
finish $f

new_case "--prefix without a folder exits 2"
run_install --prefix
f=0
check "exit 2" test "${CODE}" -eq 2 || f=$((f + 1))
finish $f

new_case "--uninstall and --binary together exit 2"
run_install --uninstall --binary /nonexistent
f=0
check "exit 2" test "${CODE}" -eq 2 || f=$((f + 1))
finish $f

new_case "installs a built program into <prefix>/bin"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "file installed" test -x "${PREFIX}/bin/fastlock" || f=$((f + 1))
check "mode 755" test "$(stat -c %a "${PREFIX}/bin/fastlock")" = 755 || f=$((f + 1))
check "reports the version" contains "${OUT}" "Installed fastlock 0.1.0 at ${PREFIX}/bin/fastlock." || f=$((f + 1))
check "no temporary file left" test -z "$(find "${PREFIX}/bin" -name '.fastlock.*')" || f=$((f + 1))
finish $f

new_case "the default prefix is ~/.local"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "file in ~/.local/bin" test -x "${CASE}/home/.local/bin/fastlock" || f=$((f + 1))
finish $f

new_case "a prefix with spaces works"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --prefix "${CASE}/my programs" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "file installed" test -x "${CASE}/my programs/bin/fastlock" || f=$((f + 1))
finish $f

new_case "installing again replaces the older fastlock"
fake_fastlock "${CASE}/old" 0.1.0
fake_fastlock "${CASE}/new" 0.2.0
run_install --prefix "${PREFIX}" --binary "${CASE}/old"
run_install --prefix "${PREFIX}" --binary "${CASE}/new"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "says what it replaced" contains "${OUT}" "Replaced fastlock 0.1.0 with fastlock 0.2.0" || f=$((f + 1))
check "new version in place" test "$("${PREFIX}/bin/fastlock" version)" = "fastlock 0.2.0" || f=$((f + 1))
finish $f

new_case "a file that is not fastlock is refused with exit 6 and kept"
mkdir -p "${PREFIX}/bin"
printf '#!/bin/sh\necho something else\n' >"${PREFIX}/bin/fastlock"
chmod 0755 "${PREFIX}/bin/fastlock"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 6" test "${CODE}" -eq 6 || f=$((f + 1))
check "explains" contains "${ERR}" "is not fastlock. Nothing was changed" || f=$((f + 1))
check "file kept" grep -q "something else" "${PREFIX}/bin/fastlock" || f=$((f + 1))
finish $f

new_case "--force replaces a file that is not fastlock"
mkdir -p "${PREFIX}/bin"
printf '#!/bin/sh\necho something else\n' >"${PREFIX}/bin/fastlock"
chmod 0755 "${PREFIX}/bin/fastlock"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock" --force
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "says it replaced a file" contains "${OUT}" "replacing the file that was there" || f=$((f + 1))
check "fastlock in place" test "$("${PREFIX}/bin/fastlock" version)" = "fastlock 0.1.0" || f=$((f + 1))
finish $f

new_case "a missing --binary file exits 5"
run_install --prefix "${PREFIX}" --binary "${CASE}/nothing-here"
f=0
check "exit 5" test "${CODE}" -eq 5 || f=$((f + 1))
check "nothing installed" test ! -e "${PREFIX}/bin/fastlock" || f=$((f + 1))
finish $f

new_case "a --binary that is not fastlock exits 1 and installs nothing"
printf '#!/bin/sh\necho hello\n' >"${CASE}/other"
chmod 0755 "${CASE}/other"
run_install --prefix "${PREFIX}" --binary "${CASE}/other"
f=0
check "exit 1" test "${CODE}" -eq 1 || f=$((f + 1))
check "explains" contains "${ERR}" "is not the fastlock program" || f=$((f + 1))
check "nothing installed" test ! -e "${PREFIX}/bin/fastlock" || f=$((f + 1))
finish $f

new_case "warns when the folder is not on PATH and prints the line to add"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "warning" contains "${ERR}" "${PREFIX}/bin is not on your PATH" || f=$((f + 1))
check "line to add" contains "${OUT}" "export PATH=\"${PREFIX}/bin:\$PATH\"" || f=$((f + 1))
check "no startup file written" test -z "$(ls -A "${CASE}/home")" || f=$((f + 1))
finish $f

new_case "no PATH warning when the folder is on PATH"
fake_fastlock "${CASE}/fastlock" 0.1.0
CASE_PATH="${PREFIX}/bin:${CASE_PATH}"
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "no PATH warning" lacks "${ERR}" "not on your PATH" || f=$((f + 1))
finish $f

new_case "warns when another fastlock comes first on PATH"
fake_fastlock "${CASE}/fastlock" 0.1.0
fake_fastlock "${CASE}/fake/fastlock" 0.0.1
CASE_PATH="${CASE_PATH}:${PREFIX}/bin"
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "warning" contains "${ERR}" "another fastlock comes first on your PATH: ${CASE}/fake/fastlock" || f=$((f + 1))
finish $f

new_case "a missing age program is a warning, not an error"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "warning" contains "${ERR}" "the age program was not found" || f=$((f + 1))
finish $f

new_case "an age older than 1.3 is a warning"
fake_fastlock "${CASE}/fastlock" 0.1.0
fake_tool age 'echo v1.2.1'
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "warning" contains "${ERR}" "is version 1.2.1; files that open with a key need age 1.3 or newer" || f=$((f + 1))
finish $f

new_case "age 1.3 or newer is reported"
fake_fastlock "${CASE}/fastlock" 0.1.0
fake_tool age 'echo v1.3.2'
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "found" contains "${OUT}" "Found age 1.3.2 at ${CASE}/fake/age." || f=$((f + 1))
check "no age warning" lacks "${ERR}" "age" || f=$((f + 1))
finish $f

new_case "FASTLOCK_AGE names the age program to check"
fake_fastlock "${CASE}/fastlock" 0.1.0
printf '#!/usr/bin/env bash\necho 1.3.0\n' >"${CASE}/my-age"
chmod 0755 "${CASE}/my-age"
EXTRA_ENV=(FASTLOCK_AGE="${CASE}/my-age")
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "found" contains "${OUT}" "Found age 1.3.0 at ${CASE}/my-age." || f=$((f + 1))
finish $f

new_case "without Cargo the build exits 5 and says what to install"
run_install --prefix "${PREFIX}"
f=0
check "exit 5" test "${CODE}" -eq 5 || f=$((f + 1))
check "explains" contains "${ERR}" "Cargo was not found. Install Rust 1.88 or newer" || f=$((f + 1))
finish $f

new_case "a Rust older than 1.88 exits 1 before building"
fake_tool rustc 'echo "rustc 1.87.0 (17067e9ac 2025-05-09)"'
fake_tool cargo "echo called >\"${CASE}/cargo-called\""
run_install --prefix "${PREFIX}"
f=0
check "exit 1" test "${CODE}" -eq 1 || f=$((f + 1))
check "explains" contains "${ERR}" "needs Rust 1.88 or newer, and this is 1.87.0" || f=$((f + 1))
check "cargo not run" test ! -e "${CASE}/cargo-called" || f=$((f + 1))
finish $f

new_case "builds with cargo build --release --locked and installs the result"
fake_tool rustc 'echo "rustc 1.100.0 (0ef1bc386 2026-11-09)"'
fake_tool cargo "printf '%s\n' \"\$*\" >\"${CASE}/cargo-args\"
mkdir -p \"\${CARGO_TARGET_DIR}/release\"
cat >\"\${CARGO_TARGET_DIR}/release/fastlock\" <<'EOS'
#!/usr/bin/env bash
echo 'fastlock 0.3.0'
EOS
chmod 0755 \"\${CARGO_TARGET_DIR}/release/fastlock\""
EXTRA_ENV=(CARGO_TARGET_DIR="${CASE}/target")
run_install --prefix "${PREFIX}"
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "cargo arguments" test "$(cat "${CASE}/cargo-args" 2>/dev/null)" = "build --release --locked --package fastlock-cli" || f=$((f + 1))
check "installed" test "$("${PREFIX}/bin/fastlock" version 2>/dev/null)" = "fastlock 0.3.0" || f=$((f + 1))
finish $f

new_case "Cargo in ~/.cargo/bin is found when it is not on PATH"
mkdir -p "${CASE}/home/.cargo/bin"
printf '#!/usr/bin/env bash\necho "rustc 1.88.0"\n' >"${CASE}/home/.cargo/bin/rustc"
printf '#!/usr/bin/env bash\nexit 101\n' >"${CASE}/home/.cargo/bin/cargo"
chmod 0755 "${CASE}/home/.cargo/bin/rustc" "${CASE}/home/.cargo/bin/cargo"
run_install --prefix "${PREFIX}"
f=0
check "Cargo was used (its failure is reported)" contains "${ERR}" "the build failed" || f=$((f + 1))
check "exit 1" test "${CODE}" -eq 1 || f=$((f + 1))
finish $f

if ((EUID == 0)); then
    new_case "building under sudo is refused with exit 2"
    fake_tool rustc 'echo "rustc 1.100.0"'
    fake_tool cargo "echo called >\"${CASE}/cargo-called\""
    EXTRA_ENV=(SUDO_USER=someone)
    run_install --prefix "${PREFIX}"
    f=0
    check "exit 2" test "${CODE}" -eq 2 || f=$((f + 1))
    check "explains" contains "${ERR}" "do not build with sudo" || f=$((f + 1))
    check "cargo not run" test ! -e "${CASE}/cargo-called" || f=$((f + 1))
    finish $f
else
    printf 'skip  building under sudo is refused (needs root to test)\n'
fi

new_case "--uninstall removes fastlock and nothing else"
fake_fastlock "${CASE}/fastlock" 0.1.0
run_install --prefix "${PREFIX}" --binary "${CASE}/fastlock"
touch "${PREFIX}/bin/other-program"
mkdir -p "${CASE}/home/.ssh"
touch "${CASE}/home/.ssh/id_ed25519"
run_install --prefix "${PREFIX}" --uninstall
f=0
check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
check "removed" test ! -e "${PREFIX}/bin/fastlock" || f=$((f + 1))
check "other file kept" test -e "${PREFIX}/bin/other-program" || f=$((f + 1))
check "keys kept" test -e "${CASE}/home/.ssh/id_ed25519" || f=$((f + 1))
check "says keys were not touched" contains "${OUT}" "Your keys and files were not touched." || f=$((f + 1))
finish $f

new_case "--uninstall when nothing is installed exits 5"
run_install --prefix "${PREFIX}" --uninstall
f=0
check "exit 5" test "${CODE}" -eq 5 || f=$((f + 1))
finish $f

new_case "--uninstall refuses a file that is not fastlock"
mkdir -p "${PREFIX}/bin"
printf '#!/bin/sh\necho something else\n' >"${PREFIX}/bin/fastlock"
chmod 0755 "${PREFIX}/bin/fastlock"
run_install --prefix "${PREFIX}" --uninstall
f=0
check "exit 6" test "${CODE}" -eq 6 || f=$((f + 1))
check "file kept" test -e "${PREFIX}/bin/fastlock" || f=$((f + 1))
finish $f

# The real program, when it has been built: install it and run it.
real="${CARGO_TARGET_DIR:-${ROOT}/target}/release/fastlock"
if [[ -x "${real}" ]]; then
    new_case "the real program installs and runs"
    run_install --prefix "${PREFIX}" --binary "${real}"
    f=0
    check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
    check "runs" contains "$("${PREFIX}/bin/fastlock" version 2>&1)" "fastlock " || f=$((f + 1))
    check "help works" contains "$("${PREFIX}/bin/fastlock" --help 2>&1)" "Usage:" || f=$((f + 1))
    finish $f
else
    printf 'skip  the real program installs and runs (build it first: cargo build --release)\n'
fi

if ((real_build)); then
    new_case "--build: install.sh builds the real program and installs it"
    # This case uses the real toolchain and the machine's own PATH.
    env HOME="${HOME}" PATH="${PATH}" bash "${INSTALL}" --prefix "${PREFIX}" \
        >"${CASE}/out" 2>"${CASE}/err"
    CODE=$?
    OUT="$(cat "${CASE}/out")"
    ERR="$(cat "${CASE}/err")"
    f=0
    check "exit 0" test "${CODE}" -eq 0 || f=$((f + 1))
    check "runs" contains "$("${PREFIX}/bin/fastlock" version 2>&1)" "fastlock " || f=$((f + 1))
    finish $f
fi

printf '\n%d passed, %d failed\n' "${passed}" "${failed}"
((failed == 0))
