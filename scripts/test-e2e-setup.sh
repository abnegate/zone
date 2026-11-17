#!/bin/sh
# Drive scripts/e2e-setup.sh against a faked bun and a faked Playwright binary.
# The retry ladder is the whole point of the script, so it is checked here
# rather than left to a network that only sometimes misbehaves.
set -eu

unset BUN_LOG PLAYWRIGHT_LOG BUN_FAIL_UNTIL

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
source=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)/e2e-setup.sh
sandbox=$directory/zone
script=$sandbox/scripts/e2e-setup.sh
binaries=$sandbox/manager/frontend/node_modules/.bin

mkdir -p "$directory/bin" "$sandbox/scripts" "$binaries"
cp "$source" "$script"

# Fails while the attempt number is at or below BUN_FAIL_UNTIL, counting from
# one, so a test can say how many attempts the download takes to come good.
cat > "$directory/bin/bun" <<'BUN'
#!/bin/sh
printf '%s\n' "$*" >> "$BUN_LOG"
attempts=$(wc -l < "$BUN_LOG" | tr -d ' ')
if [ "$attempts" -le "${BUN_FAIL_UNTIL:-0}" ]; then
    echo "error: failed to download playwright-core: IntegrityCheckFailed" >&2
    exit 2
fi
exit 0
BUN

cat > "$binaries/playwright" <<'PLAYWRIGHT'
#!/bin/sh
if [ "${1:-}" = --version ]; then
    printf 'Version 1.62.1\n'
    exit 0
fi
printf '%s\n' "$*" >> "$PLAYWRIGHT_LOG"
exit 0
PLAYWRIGHT

chmod +x "$directory/bin/bun" "$binaries/playwright"

# A PATH holding every utility the script calls except bun, so the case below
# exercises the bun guard rather than a missing dirname. Built from wherever
# these live on this machine, since bun's own directory varies.
mkdir -p "$directory/nobun"
for utility in sh dirname cat; do
    ln -s "$(command -v "$utility")" "$directory/nobun/$utility"
done

PATH=$directory/bin:$PATH
export PATH

failures=0

report() {
    if [ "$1" = pass ]; then
        printf 'ok   %s\n' "$2"
    else
        printf 'FAIL %s\n' "$2"
        failures=$((failures + 1))
    fi
}

check() {
    if [ "$2" = "$3" ]; then
        report pass "$1"
    else
        report fail "$1"
        printf '     expected: %s\n' "$3"
        printf '     actual:   %s\n' "$2"
    fi
}

# Each case starts with empty logs so the fake bun counts that case's attempts.
start() {
    BUN_LOG=$directory/bun.log
    PLAYWRIGHT_LOG=$directory/playwright.log
    export BUN_LOG PLAYWRIGHT_LOG
    : > "$BUN_LOG"
    : > "$PLAYWRIGHT_LOG"
    BUN_FAIL_UNTIL=${1:-0}
    export BUN_FAIL_UNTIL
}

logged() {
    tr '\n' '|' < "$1" | sed 's/|$//'
}

# An install that works first time installs the three browsers and asks for no
# concurrency of its own.
start 0
status=0
sh "$script" > "$directory/out" 2>&1 || status=$?
check 'a clean install exits zero' "$status" 0
check 'a clean install calls bun once' "$(logged "$BUN_LOG")" 'install'
check 'a clean install installs all three browsers' \
    "$(logged "$PLAYWRIGHT_LOG")" 'install chromium firefox webkit'

# Two failed attempts, then success: the ladder has to keep going and still
# reach the browsers.
start 2
status=0
sh "$script" > "$directory/out" 2>&1 || status=$?
check 'a flaky install exits zero once it succeeds' "$status" 0
check 'a flaky install walks the concurrency ladder' "$(logged "$BUN_LOG")" \
    'install|install --network-concurrency 8|install --network-concurrency 2'
check 'a flaky install still installs the browsers' \
    "$(logged "$PLAYWRIGHT_LOG")" 'install chromium firefox webkit'

# Every attempt failing has to fail the script, and must not go on to claim it
# installed browsers.
start 99
status=0
sh "$script" > "$directory/out" 2>&1 || status=$?
check 'an install that never succeeds fails' "$status" 1
check 'an install that never succeeds tries every rung' "$(logged "$BUN_LOG")" \
    'install|install --network-concurrency 8|install --network-concurrency 2|install --network-concurrency 1'
check 'an install that never succeeds installs no browser' \
    "$(logged "$PLAYWRIGHT_LOG")" ''
if grep -q 'could not download every package' "$directory/out"; then
    report pass 'an install that never succeeds says so'
else
    report fail 'an install that never succeeds says so'
fi

# Named browsers are passed through untouched.
start 0
status=0
sh "$script" chromium > "$directory/out" 2>&1 || status=$?
check 'a named browser exits zero' "$status" 0
check 'a named browser is the only one installed' \
    "$(logged "$PLAYWRIGHT_LOG")" 'install chromium'

# A workspace whose Playwright did not land is reported rather than skipped.
start 0
mv "$binaries/playwright" "$directory/playwright.hidden"
status=0
sh "$script" > "$directory/out" 2>&1 || status=$?
check 'a missing Playwright binary fails' "$status" 1
if grep -q 'is missing even though bun install reported success' "$directory/out"; then
    report pass 'a missing Playwright binary says so'
else
    report fail 'a missing Playwright binary says so'
fi
mv "$directory/playwright.hidden" "$binaries/playwright"

# No bun at all is reported before anything else is attempted.
start 0
status=0
env PATH="$directory/nobun" sh "$script" > "$directory/out" 2>&1 || status=$?
check 'a missing bun fails' "$status" 1
if grep -q 'bun is not on PATH' "$directory/out"; then
    report pass 'a missing bun says so'
else
    report fail 'a missing bun says so'
fi

if [ "$failures" -ne 0 ]; then
    printf '\n%s check(s) failed\n' "$failures" >&2
    exit 1
fi
printf '\nAll checks passed\n'
