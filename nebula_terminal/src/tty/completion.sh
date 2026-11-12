# Private editor query and prompt redraw, advertised as separate capabilities;
# existing user bindings retain ownership of F24 and the clear-screen redraw key.
# The redraw advertisement is re-verified against the live binding every prompt:
# a same-shell rebind revokes it with an empty value, which lets the view forget
# the ownership, while a binding the integration still owns keeps announcing.
__pebrel_redraw_owned() {
    if [ -n "${BASH_VERSION-}" ]; then
        local tables
        tables=$(bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null)
        # Match the entry's tail: the key's ESC escape renders differently
        # across readline versions, but our key's entry always ends with
        # 46~": followed by the clear-screen function name.
        [[ $tables == *"46~\": clear-screen"* ]]
    else
        [[ $(bindkey '\e[46~') == *clear-screen* ]]
    fi
}

__pebrel_editor_ready_report() {
    if [ "${__pebrel_editor_ready:-0}" = 1 ] && [ -n "${__pebrel_shell_token:-}" ]; then
        printf '\033]1337;SetUserVar=pebrel_editor_ready=%s\007' "$__pebrel_shell_token"
        if typeset -f __pebrel_pointer_ready_report >/dev/null; then __pebrel_pointer_ready_report; fi
    fi
    # Only an integration that actually owns the redraw key reports it, and it
    # keeps reporting only while the binding is still ours. The query capability
    # alone says nothing about the redraw key: an upgraded client facing an older
    # integration (query-only) must never send the key into a live shell where it
    # is simply unbound.
    if [ "${__pebrel_editor_redraw:-0}" = 1 ] && [ -n "${__pebrel_shell_token:-}" ]; then
        if __pebrel_redraw_owned; then
            printf '\033]1337;SetUserVar=pebrel_editor_redraw=%s\007' "$__pebrel_shell_token"
        else
            printf '\033]1337;SetUserVar=pebrel_editor_redraw=\007'
        fi
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
    # bind -p and bind -X do not list string macros; bind -s does. A user macro
    # on a private key must keep the key user-owned instead of being replaced.
    while IFS= read -r __pebrel_binding; do
        [[ $__pebrel_binding == '"\e[45~":'* ]] && __pebrel_editor_bound=1
        [[ $__pebrel_binding == '"\e[46~":'* ]] && __pebrel_clear_bound=1
    done < <(bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null)
    # Query and redraw are independent capabilities, each probed before its own
    # binding is installed: a user-owned redraw key leaves query, completion and
    # pointer support working exactly as in integrations that shipped no redraw.
    if [[ $__pebrel_editor_bound == 0 ]]; then
        if bind -x '"\e[45~":__pebrel_editor_report' 2>/dev/null; then
            __pebrel_editor_ready=1
        fi
    fi
    if [[ $__pebrel_clear_bound == 0 ]]; then
        if bind '"\e[46~": clear-screen' 2>/dev/null; then
            __pebrel_editor_redraw=1
        fi
    fi
    unset __pebrel_editor_bound __pebrel_clear_bound __pebrel_binding
elif [ -n "${ZSH_VERSION-}" ]; then
    if [[ $(bindkey '\e[45~') == *undefined-key ]]; then
        zle -N __pebrel_editor_report
        bindkey '\e[45~' __pebrel_editor_report
        __pebrel_editor_ready=1
    fi
    # A bindkey -s string macro also reports a non-widget body, so the
    # undefined-key probe treats it as user-owned and leaves it in place.
    if [[ $(bindkey '\e[46~') == *undefined-key ]]; then
        bindkey '\e[46~' clear-screen
        __pebrel_editor_redraw=1
    fi
fi
