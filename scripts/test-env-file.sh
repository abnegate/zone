#!/bin/sh
set -eu

unset KEY COMPOSE_PROFILES ZONE_VPN ZONE_ENV_FILE

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
file=$directory/test.env

# shellcheck source=SCRIPTDIR/env-file.sh
. "$root/scripts/env-file.sh"

cat > "$directory/compose.yml" <<'COMPOSE'
x-value: "${KEY-}"
services: {}
COMPOSE

if docker compose version >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    against_compose=1
else
    against_compose=0
    printf '%s\n' 'docker compose or python3 not found: checking expectations without comparing against Compose' >&2
fi

fail() {
    printf '%s\n expected: [%s]\n      got: [%s]\n.env:\n' "$1" "$2" "$3" >&2
    cat -v "$file" >&2
    exit 1
}

write() {
    printf '%s\n' "$@" > "$file"
}

compose_value() {
    docker compose -f "$directory/compose.yml" --env-file "$file" config --format json \
        | python3 -c 'import json, sys; sys.stdout.write(json.load(sys.stdin)["x-value"].replace("$$", "$"))'
}

check() {
    message=$1
    expected=$2
    actual=$(read_env_value KEY "$file")
    [ "$actual" = "$expected" ] || fail "$message" "$expected" "$actual"
    if [ "$against_compose" -eq 1 ]; then
        actual=$(compose_value)
        [ "$actual" = "$expected" ] || fail "Compose disagrees: $message" "$expected" "$actual"
    fi
}

write 'KEY=value # comment'
check 'An unquoted value ends at a space before #' 'value'

write 'KEY=value#hash'
check 'A # without a space before it stays in the value' 'value#hash'

printf 'KEY=value\t# tab\n' > "$file"
check 'Only a space, not a tab, starts an inline comment' "$(printf 'value\t# tab')"

write 'KEY= # comment'
check 'The space after = is not the space that starts a comment' '# comment'

write 'KEY="value # not comment"'
check 'A double-quoted value keeps a #' 'value # not comment'

write "KEY='value'"
check 'Single quotes are removed' 'value'

write "KEY='value # kept' # comment"
check 'A single-quoted value keeps a # and drops the comment after it' 'value # kept'

write 'export KEY=value'
check 'export is dropped' 'value'

write '  export KEY  =   value   '
check 'Whitespace around export, the name, = and the value is dropped' 'value'

write 'KEY: value'
check 'A YAML-style colon assigns like =' 'value'

printf 'KEY=value\r\n' > "$file"
check 'A CRLF line ending is dropped' 'value'

printf 'KEY="value"\r\nOTHER=x\r\n' > "$file"
check 'A CRLF line ending after a quoted value is dropped' 'value'

write 'KEY=first' 'OTHER=x' 'KEY=second'
check 'The last assignment wins' 'second'

write 'KEY=first' 'KEY='
check 'A later empty assignment clears an earlier value' ''

write '# KEY=commented' 'OTHER=x'
check 'A commented-out assignment is ignored' ''

write 'KEY_SUFFIX=suffix' 'PREFIX_KEY=prefix'
check 'Only the exact name matches' ''

write 'KEY=http://host:1/path?a=b'
check 'Only the first = separates the name' 'http://host:1/path?a=b'

cat > "$file" <<'ENV'
KEY="a\"b\\c\nd\te\$f\x"
ENV
check 'Double quotes honour escapes and keep unknown ones' "$(printf 'a"b\\c\nd\te')\$f\\x"

write "KEY='a\\'b\\nc'"
check 'Single quotes only unescape a quote' 'a'\''b\nc'

write 'KEY=real' 'OTHER="first line' 'KEY=hidden' 'last line"'
check 'An assignment inside a multi-line quoted value is not read' 'real'

write 'KEY="first line' '# not a comment' 'last line" # comment'
check 'A double-quoted value spans lines' "$(printf 'first line\n# not a comment\nlast line')"

rm -f "$file"
[ -z "$(read_env_value KEY "$file")" ] || fail 'A missing file reads as empty' '' "$(read_env_value KEY "$file")"

write 'COMPOSE_PROFILES=dev,monitoring # saved by make' 'ZONE_VPN="1" # tunnel on'
profiles=$(ZONE_ENV_FILE=$file sh "$root/scripts/compose.sh" print-profiles)
[ "$profiles" = 'dev,vpn,monitoring' ] || fail 'compose.sh reads profiles and ZONE_VPN past comments and quotes' 'dev,vpn,monitoring' "$profiles"

printf 'COMPOSE_PROFILES=dev\r\nZONE_VPN=1\r\nCOMPOSE_PROFILES=monitoring\r\n' > "$file"
profiles=$(ZONE_ENV_FILE=$file sh "$root/scripts/compose.sh" print-profiles)
[ "$profiles" = 'vpn,monitoring' ] || fail 'compose.sh reads CRLF lines and the last COMPOSE_PROFILES' 'vpn,monitoring' "$profiles"

printf '%s\n' 'env-file tests passed'
