# Private editor query; existing user bindings retain ownership of F24.
__pebrel_editor_ready_report() {
    if [ "${__pebrel_editor_ready:-0}" = 1 ] && [ -n "${__pebrel_shell_token:-}" ]; then
        printf '\033]1337;SetUserVar=pebrel_editor_ready=%s\007' "$__pebrel_shell_token"
    fi
}

__pebrel_editor_report() {
    local encoded owner cursor line prefix
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
    while IFS= read -r __pebrel_binding; do
        [[ $__pebrel_binding == '"\e[45~":'* ]] && __pebrel_editor_bound=1
    done < <(bind -p 2>/dev/null; bind -X 2>/dev/null)
    if [[ $__pebrel_editor_bound == 0 ]] && bind -x '"\e[45~":__pebrel_editor_report' 2>/dev/null; then
        __pebrel_editor_ready=1
    fi
    unset __pebrel_editor_bound __pebrel_binding
elif [ -n "${ZSH_VERSION-}" ]; then
    if [[ $(bindkey '\e[45~') == *undefined-key ]]; then
        zle -N __pebrel_editor_report
        bindkey '\e[45~' __pebrel_editor_report
        __pebrel_editor_ready=1
    fi
fi
