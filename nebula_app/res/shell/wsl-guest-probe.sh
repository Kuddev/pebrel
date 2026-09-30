# Pebrel WSL guest probe, run once per distribution and user as `sh -s` before
# the first zsh takeover: which shell a login starts here, and whether this guest
# user can read the host zsh bootstrap that WSLENV translated into
# NEBULA_ZSH_INTEGRATION. Two `name=value` lines; nothing else is written.
bootstrap=unreadable
if [ -n "${NEBULA_ZSH_INTEGRATION-}" ] &&
    [ -r "$NEBULA_ZSH_INTEGRATION/.zshenv" ] &&
    [ -r "$NEBULA_ZSH_INTEGRATION/.zprofile" ] &&
    [ -r "$NEBULA_ZSH_INTEGRATION/.zshrc" ]; then
    bootstrap=readable
fi
# wsl.exe starts the login shell named by the guest's passwd entry for this user
# (getent, or /etc/passwd where getent is missing); $SHELL is only the fallback
# when no account lookup is available.
user=$(id -un 2>/dev/null)
shell=$(getent passwd "$user" 2>/dev/null | cut -d: -f7)
if [ -z "$shell" ] && [ -n "$user" ] && [ -r /etc/passwd ]; then
    shell=$(awk -F: -v user="$user" '$1 == user { print $7; exit }' /etc/passwd 2>/dev/null)
fi
[ -n "$shell" ] || shell=${SHELL-}
printf 'shell=%s\nbootstrap=%s\n' "$shell" "$bootstrap"
