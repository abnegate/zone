#!/bin/sh
set -eu

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
script=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)/ollama/pull-models.sh
escape=$(printf '\033')

mkdir "$directory/bin"
cat > "$directory/bin/ollama" <<'OLLAMA'
#!/bin/sh
case "$1" in
    list) printf '%s\n' 'NAME    ID    SIZE    MODIFIED' ;;
    pull) exit 0 ;;
    *) exit 1 ;;
esac
OLLAMA
chmod +x "$directory/bin/ollama"

fail() {
    printf '%s\n' "$1" >&2
    exit 1
}

assert_plain() {
    if grep -qF '\033' "$1"; then
        fail "Expected no literal \\033 escape in $1"
    fi
    if grep -qF "$escape" "$1"; then
        fail "Expected no ESC byte in $1"
    fi
}

PATH="$directory/bin:$PATH" \
    OLLAMA_HOST='http://ollama:11434' \
    OLLAMA_MODEL_FAST='fast-model' \
    OLLAMA_MODEL_REASON='reason-model' \
    OLLAMA_MODEL_EMBED='embed-model' \
    sh "$script" > "$directory/output" 2> "$directory/error" \
    || fail 'Expected pull-models.sh to succeed against a fake ollama'

assert_plain "$directory/output"
assert_plain "$directory/error"
grep -q '^\[ollama-init\] ' "$directory/output" \
    || fail 'Expected [ollama-init] lines on stdout'

if PATH="$directory/bin:$PATH" \
    OLLAMA_HOST='http://ollama:11434' \
    OLLAMA_MODEL_FAST='' \
    OLLAMA_MODEL_REASON='' \
    OLLAMA_MODEL_EMBED='' \
    sh "$script" > "$directory/output" 2> "$directory/error"; then
    fail 'Expected pull-models.sh to reject missing model variables'
fi

assert_plain "$directory/output"
assert_plain "$directory/error"
grep -q '^\[ollama-init ERROR\] ' "$directory/error" \
    || fail 'Expected [ollama-init ERROR] lines on stderr'

printf '%s\n' 'Ollama pull script plain-output checks passed'
