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
# wsl.exe starts the login shell named by the guest's passwd entry for this user;
# $SHELL is only the fallback when the account lookup is unavailable.
shell=$(getent passwd "$(id -un 2>/dev/null)" 2>/dev/null | cut -d: -f7)
[ -n "$shell" ] || shell=${SHELL-}
printf 'shell=%s\nbootstrap=%s\n' "$shell" "$bootstrap"
