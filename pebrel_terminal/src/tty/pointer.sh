# Native line-editor operations reuse the owned query binding and numeric argument.
# 0=query; 1..4097=move; 4098..8194=range start; 8195..12291=delete to endpoint.
# 20000..999999=query with a host nonce, echoed in the response unit.
# Readline rejects arguments above 1,000,000, so endpoints travel separately.
__pebrel_pointer_prompt() {
    [ "${__pebrel_editor_ready:-0}" = 1 ] || return 0
    local boundary
    if [ -n "${BASH_VERSION-}" ]; then
        [[ $(declare -p PS1 2>/dev/null) =~ ^declare\ -[^[:space:]]*r ]] && return 0
        boundary='\[\e]133;B\a\]'
    else
        [[ ${(t)PS1} == *readonly* ]] && return 0
        boundary=$'%{\e]133;B\a%}'
    fi
    # Keep user prompt text and width accounting. Reapply after dynamic prompt
    # hooks; a redisplay emits the same boundary without guessing its width.
    [[ $PS1 == *"$boundary" ]] || PS1+=$boundary
    return 0
}

if [ -n "${ZSH_VERSION-}" ]; then
    # The existing precmd reporter runs before the user's prompt hooks.
    # Append only our boundary after those hooks have finished changing PS1.
    precmd_functions=(${precmd_functions:#__pebrel_pointer_prompt} __pebrel_pointer_prompt)
fi

__pebrel_pointer_supported() {
    [ "${__pebrel_editor_ready:-0}" = 1 ] || return 1
    local bindings digit
    if [ -n "${BASH_VERSION-}" ]; then
        [[ -o emacs ]] || return 1
        bindings=$(bind -X 2>/dev/null)
        [[ $bindings == *'"\e[45~": "__pebrel_editor_report"'* ]] || return 1
        bindings=$(bind -p 2>/dev/null)
        for digit in 0 1 2 3 4 5 6 7 8 9; do
            [[ $bindings == *'"\e'"$digit"'": digit-argument'* ]] || return 1
        done
    else
        [[ $(bindkey -lL main) == 'bindkey -A emacs main' ]] || return 1
        [[ $(bindkey '\e[45~') == *' __pebrel_editor_report' ]] || return 1
        for digit in 0 1 2 3 4 5 6 7 8 9; do
            [[ $(bindkey "\e$digit") == *' digit-argument' ]] || return 1
        done
    fi
}

__pebrel_pointer_ready_report() {
    unset __pebrel_pointer_line __pebrel_pointer_point __pebrel_pointer_anchor
    if __pebrel_pointer_supported; then
        if [ -n "${BASH_VERSION-}" ]; then __pebrel_pointer_prompt; fi
        printf '\033]1337;SetUserVar=pebrel_pointer_ready=%s\007' "$__pebrel_shell_token"
    fi
}

__pebrel_pointer_emit() {
    local encoded
    encoded=$(printf '%s' "$2" | base64 | tr -d '\r\n')
    printf '\033]1337;SetUserVar=%s=%s\007' "$1" "$encoded"
}

__pebrel_pointer_action() {
    local argument=$1 line point owner prefix bytes target first last unit=utf8
    case $argument in ''|*[!0-9]*) return ;; esac
    [ ${#argument} -le 6 ] && [ "$argument" -le 999999 ] || return
    [ "$argument" -le 12291 ] || [ "$argument" -ge 20000 ] || return
    owner=$(printf '%s' "$__pebrel_shell_token" | base64 -d)
    if [ -n "${BASH_VERSION-}" ]; then line=$READLINE_LINE; point=$READLINE_POINT
    else line=$BUFFER; point=$CURSOR; fi
    if [ "$argument" = 0 ] || [ "$argument" -ge 20000 ]; then
        [ "$argument" = 0 ] || unit=utf8:$argument
        unset __pebrel_pointer_line __pebrel_pointer_point __pebrel_pointer_anchor
        if ! __pebrel_pointer_supported; then
            __pebrel_pointer_emit pebrel_pointer "$owner"$'\n'"${unit/utf8/unsupported}"
            return
        fi
        # Store the native editor state, not terminal echo or a keystroke mirror.
        __pebrel_pointer_line=$line
        __pebrel_pointer_point=$point
        __pebrel_pointer_token=$__pebrel_shell_token
        if [ -n "${BASH_VERSION-}" ]; then prefix=${line:0:point}
        elif (( point == 0 )); then prefix=''
        else prefix=${line[1,point]}; fi
        local LC_ALL=C
        bytes=${#prefix}
        if [ ${#line} -gt 4096 ]; then
            unset __pebrel_pointer_line __pebrel_pointer_point
            __pebrel_pointer_emit pebrel_pointer "$owner"$'\n'"${unit/utf8/unsupported}"
        else
            __pebrel_pointer_emit pebrel_pointer "$owner"$'\n'"$unit"$'\n'"$bytes"$'\n'"$line"
        fi
        return
    fi
    # Revalidate in the editor callback: host revisions alone cannot catch a
    # native history widget, cursor movement or other input queued in the PTY.
    if [ "${__pebrel_pointer_token-}" != "$__pebrel_shell_token" ] ||
       [ "${__pebrel_pointer_point-}" != "$point" ] ||
       [ "${__pebrel_pointer_line-}" != "$line" ] || ! __pebrel_pointer_supported; then
        unset __pebrel_pointer_line __pebrel_pointer_point __pebrel_pointer_anchor
        __pebrel_pointer_emit pebrel_pointer_status "$owner"$'\nrejected'
        return
    fi
    target=$(( (argument - 1) % 4097 ))
    [ "$target" -le ${#line} ] || return
    if [ "$argument" -le 4097 ]; then
        if [ -n "${BASH_VERSION-}" ]; then READLINE_POINT=$target
        else CURSOR=$target; fi
        __pebrel_pointer_point=$target
    elif [ "$argument" -le 8194 ]; then
        __pebrel_pointer_anchor=$target
    elif [ -n "${__pebrel_pointer_anchor-}" ]; then
        first=$__pebrel_pointer_anchor; last=$target
        if [ "$first" -gt "$last" ]; then first=$target; last=$__pebrel_pointer_anchor; fi
        if [ -n "${BASH_VERSION-}" ]; then
            READLINE_LINE=${line:0:first}${line:last}
            READLINE_POINT=$first
        else
            if (( first == 0 )); then prefix=''; else prefix=${line[1,first]}; fi
            BUFFER=$prefix${line[$((last + 1)),-1]}
            CURSOR=$first
        fi
        # One native buffer mutation preserves the line editor's undo record.
        unset __pebrel_pointer_line __pebrel_pointer_point __pebrel_pointer_anchor
    fi
}
