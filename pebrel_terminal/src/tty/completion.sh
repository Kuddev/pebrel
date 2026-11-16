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
        # Ownership must also hold in every keymap reachable within one prompt,
        # and it must be complete: vi mode switches vi-insert <-> vi-command
        # mid-line via ESC, and an unbound multi-byte chord is NOT consumed as
        # a whole there — vi-command leaves the tail "~" bound to the
        # invert-case command, so sending our redraw into a keymap where the
        # chord is merely unbound still rewrites the user's line (review
        # reproduction: "original" became "originaL"). Every reachable keymap
        # must therefore carry exactly our clear-screen binding — nothing else
        # on the chord counts, unbound included. Entries are anchored to the
        # key's complete line, so a lookalike chord sequence ending in \e[46~
        # never trips the probe; each keymap is restored right after its dump
        # and the probe only reads, so the user's bindings stay untouched.
        local current dump keymap
        current=$(bind -v 2>/dev/null | grep -E '^set keymap ' | cut -d ' ' -f3)
        [ -n "$current" ] || return 1
        for keymap in emacs-standard emacs-ctlx emacs-meta vi-insert vi-command vi-move; do
            [ "$keymap" = "$current" ] && continue
            bind "set keymap $keymap" 2>/dev/null || continue
            dump=$({ bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null; } |
                grep -E '^"\\e\[46~":')
            bind "set keymap $current" 2>/dev/null
            [ "$dump" = '"\e[46~": clear-screen' ] || return 1
        done
    else
        # The bound widget must be the built-in original: bindkey reports the
        # value space-separated, so a space-anchored ".clear-screen" tail only
        # matches the built-in widget. A bindkey -s macro reporting a quoted
        # body (even one reading ".clear-screen") ends in a quote, and a user
        # widget shadowing the name reports the bare name — both revoke.
        [[ $(bindkey '\e[46~' 2>/dev/null) == *' .clear-screen' ]] || return 1
        # One prompt can switch keymaps mid-line (viins -> vicmd via ESC), and
        # the check above only sees the current one. Ownership is re-verified
        # against the exact keymap set the installation recorded: every keymap
        # in it must still resolve the chord to the built-in widget. An
        # unbound keymap is NOT safe — a multi-byte sequence is not consumed
        # as a whole there, and vicmd leaves the trailing "~" bound to the
        # invert-case command, so sending the redraw would rewrite the user's
        # line (review reproduction: "original" became "originaL"). A string
        # macro, a user widget, an unbound keymap and one the probe cannot
        # query all revoke ownership. The probe only reads, so the user's
        # bindings stay untouched.
        [[ -n ${__pebrel_redraw_keymaps:-} ]] || return 1
        local keymap report owned=1
        for keymap in ${=__pebrel_redraw_keymaps}; do
            report=$(bindkey -M "$keymap" '\e[46~' 2>/dev/null)
            case $report in
                *' .clear-screen') ;;
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
        # The redraw chord is all-or-nothing across keymaps: the capability is
        # only announced when the sequence is fully consumed everywhere it can
        # be reached, and an unbound tail byte still runs Vi commands (see
        # __pebrel_redraw_owned). Scan every reachable keymap first — a user
        # binding in any one of them leaves the whole capability off and the
        # key untouched — then install the built-in clear-screen into each of
        # them, vi users included. The startup keymap is restored afterwards.
        __pebrel_clear_free=1
        __pebrel_start_keymap=$(bind -v 2>/dev/null | grep -E '^set keymap ' | cut -d ' ' -f3)
        for __pebrel_keymap in emacs-standard emacs-ctlx emacs-meta vi-insert vi-command vi-move; do
            bind "set keymap $__pebrel_keymap" 2>/dev/null || continue
            while IFS= read -r __pebrel_binding; do
                [[ $__pebrel_binding == '"\e[46~":'* ]] && __pebrel_clear_free=0
            done < <(bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null)
        done
        if [[ -n "$__pebrel_start_keymap" ]]; then
            bind "set keymap $__pebrel_start_keymap" 2>/dev/null
        fi
        if [[ $__pebrel_clear_free == 1 ]]; then
            for __pebrel_keymap in emacs-standard emacs-ctlx emacs-meta vi-insert vi-command vi-move; do
                bind -m "$__pebrel_keymap" '"\e[46~": clear-screen' 2>/dev/null || {
                    __pebrel_clear_free=0
                    break
                }
            done
            [[ $__pebrel_clear_free == 1 ]] && __pebrel_editor_redraw=1
        fi
        unset __pebrel_clear_free __pebrel_start_keymap __pebrel_keymap __pebrel_binding
    fi
    unset __pebrel_editor_bound __pebrel_clear_bound __pebrel_binding
elif [ -n "${ZSH_VERSION-}" ]; then
    if [[ $(bindkey '\e[45~') == *undefined-key ]]; then
        zle -N __pebrel_editor_report
        bindkey '\e[45~' __pebrel_editor_report
        __pebrel_editor_ready=1
    fi
    # The redraw chord is all-or-nothing across keymaps (see
    # __pebrel_redraw_owned): the capability is announced only when the chord
    # is ours in every keymap that can be reached while editing. Scan every
    # keymap first — a user binding anywhere (a widget, or a bindkey -s macro
    # whose quoted body ends in a quote, even one reading "undefined-key")
    # keeps the key untouched and the whole capability off. All free → install
    # the built-in into each keymap and record the set for the per-prompt
    # re-verification. zsh refuses binding into protected keymaps (like
    # .safe); such a keymap can never carry our binding and is not reachable
    # through a typed chord, so it is excluded from the contract instead of
    # killing it.
    __pebrel_clear_free=1
    __pebrel_redraw_keymaps=''
    __pebrel_keymaps=(${(f)"$(bindkey -l 2>/dev/null)"})
    if (( ${#__pebrel_keymaps} )); then
        for __pebrel_keymap in "${__pebrel_keymaps[@]}"; do
            [[ $(bindkey -M "$__pebrel_keymap" '\e[46~' 2>/dev/null) == *' undefined-key' ]] ||
                __pebrel_clear_free=0
        done
        if [[ $__pebrel_clear_free == 1 ]]; then
            for __pebrel_keymap in "${__pebrel_keymaps[@]}"; do
                if bindkey -M "$__pebrel_keymap" '\e[46~' .clear-screen 2>/dev/null; then
                    __pebrel_redraw_keymaps="$__pebrel_redraw_keymaps $__pebrel_keymap"
                fi
            done
            [[ -n $__pebrel_redraw_keymaps ]] && __pebrel_editor_redraw=1
        fi
    fi
    unset __pebrel_clear_free __pebrel_keymaps __pebrel_keymap
fi
