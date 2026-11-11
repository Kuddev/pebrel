# Accepted command text only. It is never executed, expanded again or written to disk.
__pebrel_report_command() {
    local owner encoded
    [ -n "${__pebrel_shell_token-}" ] && [ "${#1}" -le 4096 ] || return 0
    owner=$(printf '%s' "$__pebrel_shell_token" | base64 -d)
    encoded=$(printf '%s\n%s' "$owner" "$1" | base64 | tr -d '\r\n')
    printf '\033]1337;SetUserVar=pebrel_command=%s\007' "$encoded"
}

__pebrel_bash_command() {
    local current="$1" previous="$2" entry number line
    # HISTCONTROL/HISTIGNORE may omit this command. Never borrow the previous
    # history entry: unmatched/ignored input must keep its ordinary VT output.
    (( current > previous )) || return 0
    entry=$(HISTTIMEFORMAT= builtin history 1)
    entry="${entry#"${entry%%[![:space:]]*}"}"
    number="${entry%%[[:space:]]*}"
    [[ $number =~ ^[0-9]+$ ]] && (( number + 1 == current )) || return 0
    line="${entry#"$number"}"
    line="${line#"${line%%[![:space:]]*}"}"
    __pebrel_report_command "$line"
}

if [ -n "${BASH_VERSION-}" ] && (( BASH_VERSINFO[0] > 4 || BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4 )); then
    if [[ ${PS0-} != *'__pebrel_bash_command'* ]]; then
        __pebrel_history_seen=$HISTCMD
        # The zero-length expansion updates the parent shell, unlike $(...).
        # Prefix the existing PS0 so the command arrives before its OSC 133 C.
        PS0='$(__pebrel_bash_command "$HISTCMD" "$__pebrel_history_seen")${__pebrel_history_seen:$((__pebrel_history_seen=HISTCMD,0)):0}'"${PS0-}"
    fi
elif [ -n "${ZSH_VERSION-}" ]; then
    # zsh supplies the complete accepted line, including pipes and redirections.
    # Keep existing hooks, but publish before their execution-start markers.
    if (( ! ${preexec_functions[(Ie)__pebrel_report_command]} )); then
        preexec_functions=(__pebrel_report_command "${preexec_functions[@]}")
    fi
fi
