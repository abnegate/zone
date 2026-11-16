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
            escapes["$"] = "$$"
        }
        function assign(value) {
            values[key] = value
            defined[key] = 1
        }
        function lookup(variable) {
            if (variable in ENVIRON) {
                found = 1
                return ENVIRON[variable]
            }
            found = (variable in defined)
            return found ? values[variable] : ""
        }
        function closing(text, start,    position, depth, character) {
            depth = 1
            for (position = start; position <= length(text); position++) {
                character = substr(text, position, 1)
                if (character == "$" && substr(text, position + 1, 1) ~ /[${]/) {
                    if (substr(text, position + 1, 1) == "{") depth++
                    position++
                } else if (character == "}" && --depth == 0) {
                    return position
                }
            }
            return 0
        }
        function substitute(expression,    variable, value, set, operator, word) {
            if (!match(expression, /^[A-Za-z_][A-Za-z0-9_]*/)) return ""
            variable = substr(expression, 1, RLENGTH)
            operator = substr(expression, RLENGTH + 1)
            value = lookup(variable)
            set = found
            if (substr(operator, 1, 1) == ":") {
                set = set && value != ""
                operator = substr(operator, 2)
            }
            word = substr(operator, 2)
            operator = substr(operator, 1, 1)
            if (operator == "-") return set ? value : interpolate(word)
            if (operator == "+") return set ? interpolate(word) : ""
            return value
        }
        function interpolate(text,    result, position, character, following, end) {
            result = ""
            for (position = 1; position <= length(text); position++) {
                character = substr(text, position, 1)
                following = substr(text, position + 1, 1)
                if (character != "$") {
                    result = result character
                } else if (following == "$") {
                    result = result "$"
                    position++
                } else if (following == "{") {
                    end = closing(text, position + 2)
                    if (!end) return result substr(text, position)
                    result = result substitute(substr(text, position + 2, end - position - 2))
                    position = end
                } else if (match(substr(text, position + 1), /^[A-Za-z_][A-Za-z0-9_]*/)) {
                    result = result lookup(substr(text, position + 1, RLENGTH))
                    position += RLENGTH
                } else {
                    result = result "$"
                }
            }
            return result
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
                    assign(quote == "\"" ? interpolate(buffer) : buffer)
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
            assign(interpolate(value))
        }
        END { if (name in defined) print values[name] }
    ' "$2"
}
