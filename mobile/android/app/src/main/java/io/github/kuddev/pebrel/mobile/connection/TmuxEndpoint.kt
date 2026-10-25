package io.github.kuddev.pebrel.mobile.connection

/** A server is identified by its actual executable and socket, not the login shell's PATH. */
data class TmuxEndpoint(val executable: String = "tmux", val socket: String = "") {
    internal fun command(): String {
        require(executable == "tmux" || validPath(executable))
        require(socket.isEmpty() || validPath(socket))
        return RemoteSessions.quote(executable) + " -u" +
            if (socket.isEmpty()) "" else " -S ${RemoteSessions.quote(socket)}"
    }

    companion object {
        private fun validPath(value: String) = value.startsWith('/') && value.length <= 4096 && value.none(Char::isISOControl)
        internal fun parse(output: String): List<TmuxEndpoint> {
            if (output.length > 64 * 1024) return emptyList()
            return output.lineSequence().mapNotNull { line ->
                val fields = line.split('\t')
                if (fields.size != 2 || !validPath(fields[0]) || !validPath(fields[1])) null
                else TmuxEndpoint(fields[0], fields[1])
            }.distinct().take(32).toList()
        }

        // Read only this user's multiplexer server metadata. No command lines, environments, or pane text.
        // Linux exposes the listening socket through fd inodes; macOS supplies it through lsof.
        internal val discoveryCommand = RemoteSessions.posixWithPath("""
            ps -e -o pid= -o uid= -o comm= | awk -v uid="$(id -u)" '
              ${'$'}2 == uid && (${ '$' }3 == "tmux" || ${ '$' }3 == "tmux:" || ${ '$' }3 ~ /\/tmux${'$'}/) { print ${ '$' }1; if (++n == 32) exit }
            ' | while IFS= read -r pid; do
              if [ -d "/proc/${'$'}pid/fd" ]; then
                exe=$(readlink "/proc/${'$'}pid/exe") || continue
                inodes=""
                for fd in /proc/"${'$'}pid"/fd/*; do
                  link=$(readlink "${'$'}fd") || continue
                  case "${'$'}link" in socket:\[*\]) inode=${'$'}{link#socket:[}; inode=${'$'}{inode%]}; inodes="${'$'}inodes ${'$'}inode";; esac
                done
                awk -v inodes="${'$'}inodes" 'BEGIN { split(inodes, a, " "); for (i in a) owned[a[i]]=1 }
                  ${'$'}7 in owned && ${'$'}8 ~ /^\// { print substr(${ '$' }0, index(${ '$' }0, ${ '$' }8)) }' /proc/net/unix |
                  while IFS= read -r socket; do
                    [ -S "${'$'}socket" ] && printf '%s\t%s\n' "${'$'}exe" "${'$'}socket"
                  done
              elif command -v lsof >/dev/null 2>&1; then
                exe=$(lsof -a -p "${'$'}pid" -d txt -Fn 2>/dev/null | sed -n 's/^n\(.*\/tmux\)$/\1/p' | head -n 1)
                [ -n "${'$'}exe" ] || continue
                lsof -a -p "${'$'}pid" -U -Fn 2>/dev/null | sed -n 's/^n\(\/.*\)$/\1/p' |
                  while IFS= read -r socket; do
                    [ -S "${'$'}socket" ] && printf '%s\t%s\n' "${'$'}exe" "${'$'}socket"
                  done
              fi
            done
            exit 0
        """.trimIndent())
    }
}
