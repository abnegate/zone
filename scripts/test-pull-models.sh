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

mkdir "$directory/unreachable"
cat > "$directory/unreachable/ollama" <<'OLLAMA'
#!/bin/sh
case "$1" in
    pull) printf '%s\n' "$2" >> "$PULLED" ;;
esac
exit 1
OLLAMA
printf '%s\n' '#!/bin/sh' > "$directory/unreachable/sleep"
chmod +x "$directory/unreachable/ollama" "$directory/unreachable/sleep"

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

unreachable() {
    PATH="$directory/unreachable:$PATH" \
        PULLED="$directory/pulled" \
        OLLAMA_HOST="$1" \
        OLLAMA_MODEL_FAST='fast-model' \
        OLLAMA_MODEL_REASON='reason-model' \
        OLLAMA_MODEL_EMBED='embed-model' \
        sh "$script" > "$directory/output" 2> "$directory/error"
}

: > "$directory/pulled"
unreachable 'http://models.example:11434' \
    || fail 'Expected an unreachable external Ollama to leave the stack starting'
[ ! -s "$directory/pulled" ] || fail 'Expected nothing pulled into an unreachable external Ollama'
grep -qF 'nothing pulled' "$directory/output" \
    || fail 'Expected a warning that nothing was pulled'

if unreachable 'http://ollama:11434'; then
    fail 'Expected an unreachable bundled Ollama to fail the init container'
fi
grep -qF 'failed to become ready' "$directory/error" \
    || fail 'Expected an error that the bundled Ollama never became ready'

sed '$d' "$script" > "$directory/functions.sh"
[ "$(tail -n 1 "$script")" = 'main' ] || fail 'Expected pull-models.sh to end by calling main'
leaked=$(
    PATH="$directory/bin:$PATH" \
        OLLAMA_HOST='http://ollama:11434' \
        OLLAMA_MODEL_FAST='fast-model' \
        OLLAMA_MODEL_REASON='reason-model' \
        OLLAMA_MODEL_EMBED='embed-model' \
        sh -c '
            . "$1"
            {
                validate_env
                wait_for_ollama
                pull_model fast-model FAST
                main
            } > /dev/null
            for name in missing retries model_name model_type failed; do
                eval "[ -z \"\${$name+set}\" ]" || printf "%s " "$name"
            done
            printf "%s" returned
        ' sh "$directory/functions.sh"
) || fail 'Expected the functions to succeed when sourced against a fake ollama'
[ "$leaked" = 'returned' ] \
    || fail "Expected every function to return to its caller without leaking variables, got: ${leaked:-an exit}"

printf '%s\n' 'Ollama pull script checks passed'
