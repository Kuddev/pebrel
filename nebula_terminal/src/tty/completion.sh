# Private editor query and prompt redraw, advertised as separate capabilities;
# existing user bindings retain ownership of F24 and the clear-screen redraw key.
# The redraw advertisement is re-verified against the live binding every prompt:
# a same-shell rebind revokes it with an empty value, which lets the view forget
# the ownership, while a binding the integration still owns keeps announcing.
__pebrel_redraw_owned() {
    if [ -n "${BASH_VERSION-}" ]; then
        # grep -x matches the key's complete binding line without any pattern
        # escaping: [[ ]] patterns mangle the entry's "\" and "[" combination,
        # a tail-only match would also accept a binding on \e[146~, and a
        # plain fixed-string search would match a lookalike line that merely
        # contains the entry (a chord sequence ending in \e[46~ bound to
        # clear-screen). Only our own binding's full line proves ownership.
        # An unrecognized table counts as not owned.
        { bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null; } |
            grep -qxF '"\e[46~": clear-screen' || return 1
        # Ownership must also hold in every keymap reachable within one prompt:
        # vi mode switches vi-insert <-> vi-command mid-line via ESC, so a
        # clear-screen in vi-insert next to a user macro waiting in vi-command
        # would still send the byte into user code. The other keymaps may carry
        # our binding or leave the chord unbound (an unbound key is inert, no
        # user code runs), but any user function or macro revokes. Entries are
        # anchored to the key's complete line, so a lookalike chord sequence
        # ending in \e[46~ never trips the probe; the enumeration runs in one
        # subshell and the original keymap is restored before returning. The
        # probe only reads, so the user's bindings stay untouched.
        local current rest keymap
        current=$(bind -v 2>/dev/null | grep -E '^set keymap ' | cut -d ' ' -f3)
        [ -n "$current" ] || return 1
        rest=$(for keymap in emacs-standard emacs-ctlx emacs-meta vi-insert vi-command vi-move; do
            [ "$keymap" = "$current" ] && continue
            bind "set keymap $keymap" 2>/dev/null || continue
            { bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null; }
        done 2>/dev/null | grep -E '^"\\e\[46~":' | grep -vxF '"\e[46~": clear-screen')
        bind "set keymap $current" 2>/dev/null
        [ -z "$rest" ]
    else
        # The bound widget must be the built-in original: bindkey reports the
        # value space-separated, so a space-anchored ".clear-screen" tail only
        # matches the built-in widget. A bindkey -s macro reporting a quoted
        # body (even one reading ".clear-screen") ends in a quote, and a user
        # widget shadowing the name reports the bare name — both revoke.
        [[ $(bindkey '\e[46~' 2>/dev/null) == *' .clear-screen' ]] || return 1
        # One prompt can switch keymaps mid-line (viins -> vicmd via ESC), and
        # the check above only sees the current one: a viins announcement
        # would still send the byte into a user widget waiting in vicmd. The
        # chord must therefore resolve to the built-in (or be unbound — an
        # unbound key is inert, no user code runs) in every keymap; a string
        # macro or a user widget anywhere revokes ownership. The queried key
        # renders before the value, so an unbound keymap reports a bare
        # space-anchored "undefined-key" tail while a macro always ends in a
        # quote — one whose body merely contains the word (e.g.
        # 'aundefined-key') revokes. A keymap the probe cannot query reports
        # nothing, which revokes too. The probe only reads, so the user's
        # vicmd binding stays untouched.
        local -a keymaps
        local keymap report owned=1
        keymaps=(${(f)"$(bindkey -l 2>/dev/null)"})
        (( ${#keymaps} )) || return 1
        for keymap in "${keymaps[@]}"; do
            report=$(bindkey -M "$keymap" '\e[46~' 2>/dev/null)
            case $report in
                *' undefined-key'|*' .clear-screen') ;;
                *) owned=0; break ;;
            esac
        done
        (( owned ))
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
        # Bind the built-in widget's canonical name: a user widget may shadow
        # `clear-screen`, and binding the original keeps the redraw independent
        # of such shadowing.
        bindkey '\e[46~' .clear-screen
        __pebrel_editor_redraw=1
    fi
fi
