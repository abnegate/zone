#!/bin/sh

read_env_value() {
    if [ ! -f "$2" ]; then
        return 0
    fi
    awk -v name="$1" '
        BEGIN {
            escapes["a"] = "\a"
            escapes["b"] = "\b"
            escapes["f"] = "\f"
            escapes["n"] = "\n"
            escapes["r"] = "\r"
            escapes["t"] = "\t"
            escapes["v"] = "\v"
            escapes["\\"] = "\\"
            escapes["\""] = "\""
            escapes["$"] = "$"
        }
        function assign(value) {
            if (key == name) {
                found = value
                matched = 1
            }
        }
        function unescape(character) {
            if (quote == "\"" && character in escapes) return escapes[character]
            if (quote == "\047" && character == "\047") return character
            return "\\" character
        }
        function scan(segment,    position, character) {
            for (position = 1; position <= length(segment); position++) {
                character = substr(segment, position, 1)
                if (character == "\\" && position < length(segment)) {
                    position++
                    buffer = buffer unescape(substr(segment, position, 1))
                } else if (character == quote) {
                    assign(buffer)
                    quote = ""
                    return
                } else {
                    buffer = buffer character
                }
            }
            buffer = buffer "\n"
        }
        {
            sub(/\r$/, "")
            if (quote != "") {
                scan($0)
                next
            }
            line = $0
            sub(/^[ \t]+/, "", line)
            if (line == "" || line ~ /^#/) next
            sub(/^export[ \t]+/, "", line)
            if (!match(line, /[=:]/)) next
            key = substr(line, 1, RSTART - 1)
            sub(/[ \t]+$/, "", key)
            value = substr(line, RSTART + 1)
            sub(/^[ \t]+/, "", value)
            quote = substr(value, 1, 1)
            if (quote == "\"" || quote == "\047") {
                buffer = ""
                scan(substr(value, 2))
                next
            }
            quote = ""
            comment = index(value, " #")
            if (comment) value = substr(value, 1, comment - 1)
            sub(/[[:space:]]+$/, "", value)
            assign(value)
        }
        END { if (matched) print found }
    ' "$2"
}
