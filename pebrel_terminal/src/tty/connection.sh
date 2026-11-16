# Bash/zsh connection boundaries. Arguments have already been expanded by the
# shell; they are transported as NUL-separated fields, never evaluated again.
__pebrel_connection() {
    local token="${__pebrel_shell_token:-}" report=0 encoded result
    if [ -t 1 ] && [ -n "$token" ]; then
        encoded=$(
            {
                printf '%s' "$token" | base64 -d
                printf '\n%s' "$1"
                shift
                if [ "$#" -gt 0 ]; then printf '\0%s' "$@"; fi
            } | base64 | tr -d '\r\n'
        )
        printf '\033]1337;SetUserVar=pebrel_shell=%s\007' "$token"
        printf '\033]1337;SetUserVar=pebrel_connection=%s\007' "$encoded"
        report=1
    fi
    command "$@"
    result=$?
    if [ "$report" = 1 ]; then
        printf '\033]1337;SetUserVar=pebrel_shell=%s\007' "$token"
    fi
    return "$result"
}

# Preserve functions and aliases installed by the user's own shell config.
if ! typeset -f ssh >/dev/null 2>&1 && ! alias ssh >/dev/null 2>&1; then
    function ssh { __pebrel_connection ssh "$@"; }
fi
if command -v wsl.exe >/dev/null 2>&1; then
    if ! typeset -f wsl >/dev/null 2>&1 && ! alias wsl >/dev/null 2>&1; then
        function wsl { __pebrel_connection wsl.exe "$@"; }
    fi
fi
