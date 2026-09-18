package io.github.kuddev.pebrel.mobile.connection

import android.content.Context
import kotlinx.coroutines.*
import org.json.JSONObject
import java.io.InputStream
import java.io.OutputStream
import java.security.MessageDigest
import java.util.Base64

enum class RelayServiceAction { STATUS, INSTALL, START, STOP, UNINSTALL }
data class RelayServiceState(val installed: Boolean, val running: Boolean, val ready: Boolean, val retained: Boolean)
data class RelayServiceResult(val state: RelayServiceState, val access: String? = null)
class RelayServiceFailure(val code: String) : java.io.IOException(code)

/** Native Linux service adapter. Only explicit actions mutate the selected SSH host.
 * A v2 access export belongs to the desktop, not to the phone's saved-PC list.
 */
object NativeRelayDeployment {
    private const val BINARY = "/opt/pebrel-relay/pebrel-relay"
    private val stages = setOf("checking", "initializing", "installing", "starting", "verifying", "ready", "stopping", "stopped", "removing", "uninstalled")
    private val failures = setOf("systemd_247_required", "linux_systemd_required", "root_required",
        "installation_conflict", "managed_file_changed", "explicit_update_required", "service_not_ready",
        "port_in_use", "permission_denied", "file_or_service_not_found", "binary_integrity_failed",
        "supported_init_required", "openrc_supervisor_required", "service_manager_changed",
        "configuration_directory_not_empty", "symlink_installation_path", "invalid_ownership_manifest",
        "service_command_failed", "service_command_timeout", "service_command_output_limit",
        "unprivileged_account_required", "privilege_drop_failed", "remote_tools_missing",
        "operation_timeout", "relay_operation_failed")

    suspend fun execute(
        context: Context, host: HostProfile, password: CharArray,
        verify: (HostProfile, String) -> Boolean, action: RelayServiceAction,
        address: String = host.address, port: Int = 443, purge: Boolean = false,
        progress: (RelayServiceProgress) -> Unit,
    ): RelayServiceResult = withTimeout(180_000) {
        require(port in 1..65535)
        val endpoint = validatedAddress(address)
        val stage: (String) -> Unit = { progress(RelayServiceProgress(it)) }
        DeploymentSsh(host, password, verify).use { ssh ->
            stage("connecting")
            val preflight = command(ssh, preflightCommand(), progress = stage, connected = { stage("checking") })
            val arch = preflight.last().getString("arch")
            val state = command(ssh, """
                set -eu
                if [ -L $BINARY ]; then printf '{"error":"managed_file_changed"}\n'; exit 1; fi
                if [ -x $BINARY ]; then $BINARY service-status
                else printf '{"installed":false,"running":false,"ready":false,"configuration_retained":false}\n'; fi
            """.trimIndent(), progress = stage).last()
            if (action == RelayServiceAction.STATUS) return@use RelayServiceResult(parseState(state))
            if (action == RelayServiceAction.INSTALL) {
                val (sha, encoded) = withContext(Dispatchers.IO) {
                    val binary = loadBinary(context, arch)
                    digest(binary) to Base64.getEncoder().encode(binary)
                }
                // The random directory is created by this command, mode 0700.
                // Cleanup names just its one file and then its empty directory.
                val install = """
                    set -eu
                    umask 077
                    stage=${'$'}(mktemp -d /tmp/pebrel-relay.XXXXXXXX)
                    trap 'rm -f "${'$'}stage/pebrel-relay"; rmdir "${'$'}stage"' EXIT
                    head -c ${encoded.size} | base64 -d > "${'$'}stage/pebrel-relay"
                    printf '{"event":"progress","stage":"installing"}\n'
                    printf '%s  %s\n' '$sha' "${'$'}stage/pebrel-relay" | sha256sum -c - >/dev/null || { printf '{"error":"binary_integrity_failed"}\n'; exit 1; }
                    chmod 700 "${'$'}stage/pebrel-relay"
                    "${'$'}stage/pebrel-relay" service-install --source "${'$'}stage/pebrel-relay" --sha256 '$sha' --address ${quote(endpoint)} --port $port
                    $BINARY export-access --directory /etc/pebrel-relay
                    printf '\n'
                    $BINARY service-status
                """.trimIndent()
                progress(RelayServiceProgress("uploading", 0, encoded.size))
                val result = command(ssh, install, encoded, stage) { sent, total ->
                    progress(RelayServiceProgress(if (sent == total) "uploaded" else "uploading", sent, total))
                }
                val access = result.firstOrNull { it.optInt("version") == 2 && it.has("desktopToken") }
                    ?: throw RelayServiceFailure("invalid_access")
                validateAccess(access)
                val final = parseState(result.last())
                if (!final.ready) throw RelayServiceFailure("service_not_ready")
                return@use RelayServiceResult(final, access.toString())
            }
            if (!state.optBoolean("installed")) throw RelayServiceFailure("file_or_service_not_found")
            val operation = when (action) {
                RelayServiceAction.START -> "service-start"
                RelayServiceAction.STOP -> "service-stop"
                RelayServiceAction.UNINSTALL -> "service-uninstall${if (purge) " --purge" else ""}"
                else -> error("unreachable")
            }
            val result = command(ssh, "$BINARY $operation" + if (action != RelayServiceAction.UNINSTALL) "\n$BINARY service-status" else "", progress = stage)
            if (action == RelayServiceAction.UNINSTALL) {
                check(result.last().optBoolean("ok"))
                RelayServiceResult(RelayServiceState(false, false, false, !purge))
            } else RelayServiceResult(parseState(result.last()))
        }
    }

    internal fun preflightCommand() = """
        set -eu
        fail() { printf '{"error":"%s"}\n' "${'$'}1"; exit 1; }
        [ "${'$'}(uname -s)" = Linux ] || fail linux_systemd_required
        [ "${'$'}(id -u)" = 0 ] || fail root_required
        for tool in head base64 sha256sum mktemp chmod; do
            command -v "${'$'}tool" >/dev/null 2>&1 || fail remote_tools_missing
        done
        if [ -d /run/systemd/system ]; then
            command -v systemctl >/dev/null 2>&1 || fail supported_init_required
        elif [ -d /run/openrc ] && [ -x /sbin/rc-service ] && [ -x /sbin/openrc-run ]; then
            [ -x /sbin/supervise-daemon ] && [ -x /sbin/rc-update ] || fail openrc_supervisor_required
        else
            fail supported_init_required
        fi
        printf '{"arch":"%s"}\n' "${'$'}(uname -m)"
    """.trimIndent()

    internal fun checkedMessages(exit: Int, messages: List<JSONObject>, errors: List<JSONObject>): List<JSONObject> {
        val failure = (messages + errors).firstOrNull { it.has("error") }?.optString("error")
        if (exit != 0 || failure != null) throw RelayServiceFailure(failure?.takeIf { it in failures } ?: "service_failed")
        if (messages.isEmpty()) throw RelayServiceFailure("invalid_response")
        return messages
    }

    internal fun validatedAddress(value: String): String {
        val host = value.trim().removePrefix("[").removeSuffix("]")
        if (host.isEmpty() || host.length > 253 || !Regex("[A-Za-z0-9.:-]+").matches(host) || host.startsWith('-'))
            throw RelayServiceFailure("invalid_address")
        return host
    }
    private fun quote(value: String) = "'${value.replace("'", "'\"'\"'")}'"
    private fun digest(bytes: ByteArray) = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
    private fun loadBinary(context: Context, arch: String): ByteArray {
        if (arch !in setOf("x86_64", "aarch64")) throw RelayServiceFailure("unsupported_arch")
        try {
            val prefix = "native-relay/$arch"
            val manifest = context.assets.open("$prefix/manifest.json").use { JSONObject(String(it.readBytes())) }
            val bytes = context.assets.open("$prefix/pebrel-relay").use { it.readBytes() }
            if (bytes.size !in 64..64 * 1024 * 1024 || manifest.getString("sha256") != digest(bytes) ||
                manifest.getString("arch") != arch || manifest.getInt("protocol") != 2)
                throw RelayServiceFailure("binary_integrity_failed")
            return bytes
        } catch (error: RelayServiceFailure) { throw error }
        catch (_: Exception) { throw RelayServiceFailure("asset_missing") }
    }
    internal fun parseState(json: JSONObject) = RelayServiceState(json.getBoolean("installed"),
        json.getBoolean("running"), json.getBoolean("ready"), json.getBoolean("configuration_retained"))
    internal fun validateAccess(json: JSONObject) {
        val url = runCatching { java.net.URI(json.optString("url")) }.getOrNull()
        if (json.optInt("version") != 2 || url == null || url.scheme != "wss" || url.host.isNullOrEmpty() ||
            url.userInfo != null || url.query != null || url.fragment != null || url.path !in setOf("", "/") ||
            !Regex("[A-Za-z0-9_-]{1,64}").matches(json.optString("room")) ||
            !Regex("sha256/[A-Za-z0-9+/]{43}=").matches(json.optString("tlsPin")) ||
            listOf("desktopToken", "mobileToken").any { !Regex("[A-Za-z0-9_-]{43}").matches(json.optString(it)) } ||
            json.optString("desktopToken") == json.optString("mobileToken")) throw RelayServiceFailure("invalid_access")
    }

    private suspend fun command(ssh: DeploymentSsh, text: String, input: ByteArray? = null,
        progress: (String) -> Unit, connected: () -> Unit = {}, upload: (Int, Int) -> Unit = { _, _ -> }): List<JSONObject> = withTimeout(120_000) {
        val connection = ssh.open(30_000)
        try {
            connected()
            ssh.blocking(connection) { connection.openExec(text) }
            coroutineScope {
                val output = async(Dispatchers.IO) { ssh.blocking(connection) { readMessages(connection.input(false), progress) } }
                val errors = async(Dispatchers.IO) { ssh.blocking(connection) { readMessages(connection.input(true), {}) } }
                val writer = async(Dispatchers.IO) { if (input != null) ssh.blocking(connection) {
                    writeUpload(connection.output(), input, upload)
                } }
                val code = async(Dispatchers.IO) { ssh.blocking(connection) { connection.awaitExit() } }
                val exit = code.await()
                writer.await()
                val messages = output.await()
                checkedMessages(exit, messages, errors.await())
            }
        } finally { ssh.release(connection) }
    }

    internal fun writeUpload(output: OutputStream, bytes: ByteArray, progress: (Int, Int) -> Unit) {
        var sent = 0
        while (sent < bytes.size) {
            val count = minOf(32 * 1024, bytes.size - sent)
            output.write(bytes, sent, count)
            sent += count
            progress(sent, bytes.size)
        }
        output.flush()
    }

    internal fun readMessages(input: InputStream, progress: (String) -> Unit): List<JSONObject> {
        // Russh's unbuffered single-byte read allocates and crosses JNI for every byte.
        val buffered = input.buffered(8192)
        val result = mutableListOf<JSONObject>()
        var total = 0
        // export-access is pretty JSON; keep it as one bounded object.
        val objectText = StringBuilder()
        while (true) {
            val line = readBoundedFrame(buffered, 8192)?.toString(Charsets.UTF_8)?.trimEnd() ?: break
            total += line.length
            if (total > 32 * 1024 || line.length > 8192) throw RelayServiceFailure("output_limit")
            if (objectText.isNotEmpty() || line.startsWith('{')) {
                objectText.append(line)
                val json = runCatching { JSONObject(objectText.toString()) }.getOrNull()
                if (json != null) {
                    objectText.clear()
                    if (json.optString("event") == "progress") {
                        json.optString("stage").takeIf { it in stages }?.let(progress)
                    } else result += json
                }
            }
        }
        return result
    }
}
