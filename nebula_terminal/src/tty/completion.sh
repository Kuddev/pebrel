# Private editor query and prompt redraw; existing user bindings retain
# ownership of F24 and the clear-screen redraw key.
__pebrel_editor_ready_report() {
    if [ "${__pebrel_editor_ready:-0}" = 1 ] && [ -n "${__pebrel_shell_token:-}" ]; then
        printf '\033]1337;SetUserVar=pebrel_editor_ready=%s\007' "$__pebrel_shell_token"
        if typeset -f __pebrel_pointer_ready_report >/dev/null; then __pebrel_pointer_ready_report; fi
    fi
}

__pebrel_editor_report() {
    local encoded owner cursor line prefix
    local argument=${READLINE_ARGUMENT-${NUMERIC-}}
    if [ -n "$argument" ] && typeset -f __pebrel_pointer_action >/dev/null; then
        __pebrel_pointer_action "$argument"
        return
    fi
    owner=$(printf '%s' "$__pebrel_shell_token" | base64 -d)
    if [ -n "${BASH_VERSION-}" ]; then
        line=$READLINE_LINE
        prefix=${READLINE_LINE:0:READLINE_POINT}
    else
        line=$BUFFER
        if ((CURSOR == 0)); then prefix=''
        else prefix=${BUFFER[1,CURSOR]}; fi
    fi
    # Native offsets follow the locale's characters; the wire contract is bytes.
    local LC_ALL=C
    cursor=${#prefix}
    encoded=$(printf '%s\nutf8\n%s\n%s' "$owner" "$cursor" "$line" | base64 | tr -d '\r\n')
    printf '\033]1337;SetUserVar=pebrel_editor=%s\007' "$encoded"
}

# Bash 3 does not expose the native buffer/caret variables used by bind -x.
if [ -n "${BASH_VERSION-}" ] && (( BASH_VERSINFO[0] >= 4 )) && [[ $- == *i* ]]; then
    __pebrel_editor_bound=0
    __pebrel_clear_bound=0
    while IFS= read -r __pebrel_binding; do
        [[ $__pebrel_binding == '"\e[45~":'* ]] && __pebrel_editor_bound=1
        [[ $__pebrel_binding == '"\e[46~":'* ]] && __pebrel_clear_bound=1
    done < <(bind -p 2>/dev/null; bind -X 2>/dev/null)
    # The redraw key is a readline function binding, not a shell command: the
    # editor clears the screen and repaints prompt, buffer and caret itself.
    if [[ $__pebrel_editor_bound == 0 ]] && bind -x '"\e[45~":__pebrel_editor_report' 2>/dev/null &&
        [[ $__pebrel_clear_bound == 0 ]] && bind '"\e[46~": clear-screen' 2>/dev/null; then
        __pebrel_editor_ready=1
    else
        # The channel is advertised as one unit; an unusable redraw key also
        # withdraws the query binding instead of half-installing.
        bind -r '"\e[45~"' 2>/dev/null
    fi
    unset __pebrel_editor_bound __pebrel_clear_bound __pebrel_binding
elif [ -n "${ZSH_VERSION-}" ]; then
    if [[ $(bindkey '\e[45~') == *undefined-key ]] && [[ $(bindkey '\e[46~') == *undefined-key ]]; then
        zle -N __pebrel_editor_report
        bindkey '\e[45~' __pebrel_editor_report
        bindkey '\e[46~' clear-screen
        __pebrel_editor_ready=1
    fi
fi
