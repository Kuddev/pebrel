# Pebrel WSL guest probe; nebula_app/src/platform/wsl_guest_shell.rs runs it and
# parses the two `name=value` lines it prints.
bootstrap=readable
for file in .zshenv .zprofile .zshrc; do
    [ -n "${NEBULA_ZSH_INTEGRATION-}" ] && [ -r "$NEBULA_ZSH_INTEGRATION/$file" ] || bootstrap=unreadable
done
# wsl.exe starts the passwd login shell for this user, not $SHELL.
user=$(id -un 2>/dev/null)
shell=$({ getent passwd "$user" || cat /etc/passwd; } 2>/dev/null | awk -F: -v user="$user" '$1 == user { print $7; exit }')
[ -n "$shell" ] || shell=${SHELL-}
printf 'shell=%s\nbootstrap=%s\n' "$shell" "$bootstrap"
