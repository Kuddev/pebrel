package io.github.kuddev.pebrel.mobile.connection

import android.content.Context
import android.util.Base64
import io.github.kuddev.pebrel.ssh.NativeSshException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.job
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.IDN
import java.nio.charset.StandardCharsets
import java.util.Locale
import java.util.UUID
import java.io.Closeable

/** User-controlled values used to install one relay project on one SSH host. */
data class RelayDeploymentRequest(
    val domain: String,
    val httpsPort: Int = 443,
    val httpChallengePort: Int = 80,
    val installDirectory: String = "~/.pebrel-relay",
    val computerName: String = "Pebrel computer",
    val allowInput: Boolean = false,
) {
    /** Alias used by callers that call the public HTTPS endpoint a service port. */
    val servicePort: Int get() = httpsPort
}

enum class RelayDeploymentStage {
    VALIDATING,
    CONNECTING,
    CHECKING_PREREQUISITES,
    PREPARING,
    UPLOADING,
    EXTRACTING,
    INITIALIZING,
    STARTING,
    HEALTH_CHECK,
    COMPLETE,
}

/** Progress deliberately contains no remote output, paths or credentials. */
data class RelayDeploymentProgress(
    val stage: RelayDeploymentStage,
    val step: Int,
    val totalSteps: Int = 10,
    val bytesSent: Long = 0,
    val bytesTotal: Long = 0,
)

enum class RelayDeploymentErrorCode {
    ASSET_MISSING,
    ASSET_INVALID,
    INVALID_INPUT,
    MISSING_CREDENTIALS,
    SSH_TIMEOUT,
    SSH_TRUST_REJECTED,
    SSH_HOST_KEY_CHANGED,
    SSH_AUTH,
    SSH_FAILED,
    DOCKER_MISSING,
    DOCKER_UNAVAILABLE,
    COMPOSE_MISSING,
    TAR_MISSING,
    BASE64_MISSING,
    FIND_MISSING,
    REMOTE_PERMISSION,
    INSTALL_DIRECTORY_NOT_EMPTY,
    UNSUPPORTED_PROJECT_LAYOUT,
    UPLOAD_FAILED,
    EXTRACT_FAILED,
    INITIALIZE_FAILED,
    START_FAILED,
    HEALTH_CLIENT_MISSING,
    HEALTH_FAILED,
    RESULT_INVALID,
    CANCELLED,
    UNKNOWN,
}

/** Typed deployment failures. The message is a stable code, never server output. */
class RelayDeploymentException(
    val code: RelayDeploymentErrorCode,
    val safeOutput: String = "",
    cause: Throwable? = null,
) : IOException(code.name.lowercase(Locale.ROOT), cause)

/**
 * Result of a completed and externally health-checked deployment.
 *
 * The two configuration strings are intentionally returned to the caller so the UI can
 * provide an explicit copy/share action. They are never sent to progress callbacks.
 */
data class RelayDeploymentResult(
    val mobileProfile: RelayProfile,
    val mobileInvitation: String,
    val desktopConfig: String,
    val mobileInvitationFileName: String,
    val desktopConfigFileName: String,
    val desktopCommand: String,
    val healthUrl: String,
    val installDirectory: String,
)

/**
 * The native russh adapter owns one exec channel per session. Deployment therefore
 * opens a fresh authenticated session for every remote command while retaining the
 * fingerprint accepted during the first session.
 */
private class DeploymentSsh(
    private val host: HostProfile,
    private val password: CharArray,
    private val verifyHost: (HostProfile, String) -> Boolean,
) : Closeable {
    private val guard = Any()
    @Volatile private var active: SshConnection? = null
    @Volatile private var closed = false
    @Volatile private var trustedFingerprint = host.fingerprint

    suspend fun open(timeoutMs: Long): SshConnection {
        val connection = synchronized(guard) {
            check(!closed) { "closed" }
            SshConnection(host.copy(fingerprint = trustedFingerprint), password.copyOf(), ::verify)
                .also { active = it }
        }
        return try {
            withTimeout(timeoutMs) {
                cancellableBlocking(connection) { connection.connect() }
            }
            connection
        } catch (error: Throwable) {
            release(connection)
            throw error
        }
    }

    /** Run one blocking native call without making coroutine cancellation wait for JNI. */
    private suspend fun <T> cancellableBlocking(
        connection: SshConnection,
        action: () -> T,
    ): T = withContext(Dispatchers.IO) {
        suspendCancellableCoroutine { continuation ->
            continuation.invokeOnCancellation { connection.close() }
            try {
                continuation.resume(action())
            } catch (error: Throwable) {
                continuation.resumeWithException(error)
            }
        }
    }

    suspend fun <T> blocking(connection: SshConnection, action: () -> T): T =
        cancellableBlocking(connection, action)

    fun release(connection: SshConnection) {
        synchronized(guard) {
            if (active === connection) active = null
        }
        connection.close()
    }

    override fun close() {
        val connection = synchronized(guard) {
            closed = true
            active.also { active = null }
        }
        connection?.close()
    }

    private fun verify(profile: HostProfile, fingerprint: String): Boolean {
        val known = trustedFingerprint
        if (known.isNotEmpty()) return known == fingerprint
        val accepted = verifyHost(host, fingerprint)
        if (!accepted) return false
        synchronized(guard) {
            if (trustedFingerprint.isEmpty()) trustedFingerprint = fingerprint
            return trustedFingerprint == fingerprint
        }
    }
}

/** Android adapter for deploying the existing user-hosted relay over password SSH. */
object RelayDeployment {
    const val ASSET_NAME = "relay-kit.tar.gz"

    private const val MAX_ARCHIVE_BYTES = 16 * 1024 * 1024
    private const val MAX_COMMAND_OUTPUT_BYTES = 48 * 1024
    private const val MAX_SMALL_OUTPUT_BYTES = 8 * 1024
    private const val CONNECT_TIMEOUT_MS = 90_000L
    private const val COMMAND_TIMEOUT_MS = 120_000L
    private const val UPLOAD_TIMEOUT_MS = 10 * 60_000L
    private const val INIT_TIMEOUT_MS = 240_000L
    private const val BUILD_TIMEOUT_MS = 15 * 60_000L
    private const val HEALTH_TIMEOUT_MS = 45_000L
    private const val CLEANUP_TIMEOUT_MS = 8_000L
    private const val TOTAL_STEPS = 10

    /** Loads the build-generated relay kit from the APK and deploys it on [host]. */
    suspend fun deploy(
        context: Context,
        host: HostProfile,
        password: CharArray,
        verify: (HostProfile, String) -> Boolean,
        request: RelayDeploymentRequest,
        onProgress: (RelayDeploymentProgress) -> Unit = {},
    ): RelayDeploymentResult {
        val archive = try {
            withContext(Dispatchers.IO) {
                context.assets.open(ASSET_NAME).use { readBounded(it, MAX_ARCHIVE_BYTES) }
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: IOException) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.ASSET_MISSING, cause = error)
        } catch (error: Exception) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.ASSET_MISSING, cause = error)
        }
        return deploy(host, password, verify, request, archive, onProgress)
    }

    /** Entry point useful to tests and to build tooling that already has the asset bytes. */
    suspend fun deploy(
        host: HostProfile,
        password: CharArray,
        verify: (HostProfile, String) -> Boolean,
        request: RelayDeploymentRequest,
        archive: ByteArray,
        onProgress: (RelayDeploymentProgress) -> Unit = {},
    ): RelayDeploymentResult {
        if (archive.isEmpty() || archive.size > MAX_ARCHIVE_BYTES) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.ASSET_INVALID)
        }
        val config = validate(request)
        emit(onProgress, RelayDeploymentStage.VALIDATING, 1)

        val ssh = DeploymentSsh(host, password, verify)
        val parentJob = currentCoroutineContext().job
        val closeOnCancel = parentJob.invokeOnCompletion { cause ->
            if (cause is CancellationException) ssh.close()
        }
        val deploymentId = UUID.randomUUID().toString().replace("-", "").take(16)
        val project = config.installDirectory
        val projectExpression = remotePathExpression(project)
        val stage = ".pebrel-relay-stage-$deploymentId"
        val archivePath = ".pebrel-relay-kit-$deploymentId.tar.gz"
        var outputTail = ""
        return try {
            emit(onProgress, RelayDeploymentStage.CONNECTING, 2)

            emit(onProgress, RelayDeploymentStage.CHECKING_PREREQUISITES, 3)
            val prerequisites = checkPrerequisites(ssh)

            val identity = runChecked(
                ssh,
                "identity",
                "printf 'uid=%s\\ngid=%s\\n' \"\$(id -u)\" \"\$(id -g)\"",
                MAX_SMALL_OUTPUT_BYTES,
                COMMAND_TIMEOUT_MS,
            )
            outputTail = safeRemoteOutput(identity)
            val uid = marker(identity.stdout, "uid") ?: throw deploymentFailure(RelayDeploymentErrorCode.REMOTE_PERMISSION, identity)
            val gid = marker(identity.stdout, "gid") ?: throw deploymentFailure(RelayDeploymentErrorCode.REMOTE_PERMISSION, identity)

            runChecked(
                ssh,
                "project",
                "mkdir -p $projectExpression",
                MAX_SMALL_OUTPUT_BYTES,
                COMMAND_TIMEOUT_MS,
            )
            emit(onProgress, RelayDeploymentStage.PREPARING, 4)
            try {
                runChecked(
                    ssh,
                    "prepare",
                    inProject(projectExpression, prepareCommand()),
                    MAX_SMALL_OUTPUT_BYTES,
                    COMMAND_TIMEOUT_MS,
                )
            } catch (error: RelayDeploymentException) {
                if (error.safeOutput.contains("install_directory_not_empty")) {
                    throw error.copyWith(RelayDeploymentErrorCode.INSTALL_DIRECTORY_NOT_EMPTY)
                }
                if (error.safeOutput.contains("unsupported_project_layout")) {
                    throw error.copyWith(RelayDeploymentErrorCode.UNSUPPORTED_PROJECT_LAYOUT)
                }
                throw error
            }

            emit(onProgress, RelayDeploymentStage.UPLOADING, 5, 0, archive.size.toLong())
            uploadArchive(ssh, projectExpression, archivePath, archive, onProgress)

            emit(onProgress, RelayDeploymentStage.EXTRACTING, 6)
            runChecked(
                ssh,
                "extract",
                inProject(projectExpression, extractCommand(stage, archivePath)),
                MAX_SMALL_OUTPUT_BYTES,
                COMMAND_TIMEOUT_MS,
            )
            runChecked(
                ssh,
                "install_source",
                inProject(projectExpression, installSourceCommand(stage)),
                MAX_SMALL_OUTPUT_BYTES,
                COMMAND_TIMEOUT_MS,
            )
            runChecked(
                ssh,
                "compose_config",
                inProject(projectExpression, composeConfigCommand(config, uid, gid)),
                MAX_SMALL_OUTPUT_BYTES,
                COMMAND_TIMEOUT_MS,
            )

            emit(onProgress, RelayDeploymentStage.INITIALIZING, 7)
            val init = try {
                runChecked(
                    ssh,
                    "initialize",
                    inProject(projectExpression, initializeCommand(config, uid, gid)),
                    MAX_COMMAND_OUTPUT_BYTES,
                    INIT_TIMEOUT_MS,
                )
            } catch (error: RelayDeploymentException) {
                throw error.copyWith(RelayDeploymentErrorCode.INITIALIZE_FAILED)
            }
            outputTail = safeRemoteOutput(init)
            val deviceId = marker(init.stdout, "PEBREL_DEVICE")
                ?: throw RelayDeploymentException(RelayDeploymentErrorCode.RESULT_INVALID, safeRemoteOutput(init))
            val invitation = markerBlock(init.stdout, "PEBREL_INVITATION_BEGIN", "PEBREL_INVITATION_END")
                ?: throw RelayDeploymentException(RelayDeploymentErrorCode.RESULT_INVALID, safeRemoteOutput(init))
            val desktop = markerBlock(init.stdout, "PEBREL_DESKTOP_BEGIN", "PEBREL_DESKTOP_END")
                ?: throw RelayDeploymentException(RelayDeploymentErrorCode.RESULT_INVALID, safeRemoteOutput(init))
            val resultData = parseResult(config, project, deviceId, invitation, desktop)

            emit(onProgress, RelayDeploymentStage.STARTING, 8)
            try {
                val compose = composeCommand(prerequisites.compose)
                val start = runChecked(
                    ssh,
                    "start",
                    inProject(projectExpression, "$compose up -d --build relay tls"),
                    MAX_COMMAND_OUTPUT_BYTES,
                    BUILD_TIMEOUT_MS,
                )
                outputTail = safeRemoteOutput(start)
            } catch (error: RelayDeploymentException) {
                throw error.copyWith(RelayDeploymentErrorCode.START_FAILED)
            }

            emit(onProgress, RelayDeploymentStage.HEALTH_CHECK, 9)
            val health = try {
                runCommand(
                    ssh,
                    healthCommand(resultData.healthUrl),
                    MAX_SMALL_OUTPUT_BYTES,
                    HEALTH_TIMEOUT_MS,
                )
            } catch (error: RelayDeploymentException) {
                throw error.copyWith(RelayDeploymentErrorCode.HEALTH_FAILED)
            }
            outputTail = safeRemoteOutput(health)
            if (health.stdout.contains("health_client_missing") || health.stderr.contains("health_client_missing")) {
                throw RelayDeploymentException(RelayDeploymentErrorCode.HEALTH_CLIENT_MISSING, outputTail)
            }
            if (health.exitCode != 0 || health.stdout.trim() != "ok") {
                throw RelayDeploymentException(RelayDeploymentErrorCode.HEALTH_FAILED, outputTail)
            }

            runChecked(
                ssh,
                "marker",
                inProject(projectExpression, markerCommand()),
                MAX_SMALL_OUTPUT_BYTES,
                COMMAND_TIMEOUT_MS,
            )
            emit(onProgress, RelayDeploymentStage.COMPLETE, 10)
            resultData
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: RelayDeploymentException) {
            throw error
        } catch (error: Exception) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.UNKNOWN, outputTail, error)
        } finally {
            runCatching {
                withContext(kotlinx.coroutines.NonCancellable) {
                    runCommand(
                        ssh,
                        inProject(projectExpression, "rm -rf -- ${shellQuote(stage)} ${shellQuote(archivePath)}"),
                        MAX_SMALL_OUTPUT_BYTES,
                        CLEANUP_TIMEOUT_MS,
                    )
                }
            }
            closeOnCancel.dispose()
            ssh.close()
        }
    }

    private data class ValidatedRequest(
        val domain: String,
        val httpsPort: Int,
        val httpChallengePort: Int,
        val installDirectory: String,
        val computerName: String,
        val allowInput: Boolean,
    )

    private data class Prerequisites(val compose: ComposeKind)

    private enum class ComposeKind { PLUGIN, LEGACY }

    private data class CommandResult(
        val exitCode: Int,
        val stdout: String,
        val stderr: String,
    )

    private fun validate(request: RelayDeploymentRequest): ValidatedRequest {
        val domain = request.domain.trim().trimEnd('.')
        val asciiDomain = try { IDN.toASCII(domain).lowercase(Locale.ROOT) } catch (_: Exception) { "" }
        val labels = asciiDomain.split('.')
        val validDomain = domain.isNotEmpty() && asciiDomain.length <= 253 && labels.all { label ->
            label.isNotEmpty() && label.length <= 63 && label.first().isLetterOrDigit() &&
                label.last().isLetterOrDigit() && label.all { it.isLetterOrDigit() || it == '-' }
        }
        if (!validDomain || domain.any { it.isWhitespace() || it == '/' || it == ':' || it == '\u0000' }) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.INVALID_INPUT)
        }
        if (request.httpsPort !in 1..65535 || request.httpChallengePort !in 1..65535 ||
            request.httpsPort == request.httpChallengePort) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.INVALID_INPUT)
        }
        val directory = request.installDirectory.trim()
        if (directory.isEmpty() || directory.length > 240 || directory == "/" || directory == "~" ||
            directory.split('/').any { it == "." || it == ".." } || directory.contains(':') ||
            !(directory.startsWith("/") || directory == "~" || directory.startsWith("~/")) ||
            directory.any { it == '\u0000' || it == '\r' || it == '\n' }) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.INVALID_INPUT)
        }
        val name = request.computerName.trim()
        if (name.isEmpty() || name.length > 80 || name.any { it == '\u0000' || it == '\r' || it == '\n' }) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.INVALID_INPUT)
        }
        return ValidatedRequest(asciiDomain, request.httpsPort, request.httpChallengePort, directory, name, request.allowInput)
    }

    private suspend fun checkPrerequisites(ssh: DeploymentSsh): Prerequisites {
        val result = runCommand(
            ssh,
            """
            set -eu
            for tool in docker tar base64 head find id; do
                if ! command -v "${'$'}tool" >/dev/null 2>&1; then
                    printf 'tool_missing=%s\n' "${'$'}tool"
                    exit 20
                fi
            done
            if ! docker info >/dev/null 2>&1; then
                printf 'docker_unavailable\n'
                exit 21
            fi
            if docker compose version >/dev/null 2>&1; then
                printf 'compose=plugin\n'
                exit 0
            fi
            if command -v docker-compose >/dev/null 2>&1 && docker-compose version >/dev/null 2>&1; then
                printf 'compose=legacy\n'
                exit 0
            fi
            printf 'compose_missing\n'
            exit 22
            """.trimIndent(),
            MAX_SMALL_OUTPUT_BYTES,
            COMMAND_TIMEOUT_MS,
        )
        val text = result.stdout.trim()
        if (result.exitCode != 0) {
            when {
                text.contains("tool_missing=docker") -> throw RelayDeploymentException(RelayDeploymentErrorCode.DOCKER_MISSING, safeRemoteOutput(result))
                text.contains("tool_missing=tar") -> throw RelayDeploymentException(RelayDeploymentErrorCode.TAR_MISSING, safeRemoteOutput(result))
                text.contains("tool_missing=base64") -> throw RelayDeploymentException(RelayDeploymentErrorCode.BASE64_MISSING, safeRemoteOutput(result))
                text.contains("tool_missing=head") -> throw RelayDeploymentException(RelayDeploymentErrorCode.BASE64_MISSING, safeRemoteOutput(result))
                text.contains("tool_missing=find") -> throw RelayDeploymentException(RelayDeploymentErrorCode.FIND_MISSING, safeRemoteOutput(result))
                text.contains("docker_unavailable") -> throw RelayDeploymentException(RelayDeploymentErrorCode.DOCKER_UNAVAILABLE, safeRemoteOutput(result))
                else -> throw RelayDeploymentException(RelayDeploymentErrorCode.COMPOSE_MISSING, safeRemoteOutput(result))
            }
        }
        return when {
            text.contains("compose=plugin") -> Prerequisites(ComposeKind.PLUGIN)
            text.contains("compose=legacy") -> Prerequisites(ComposeKind.LEGACY)
            else -> throw RelayDeploymentException(RelayDeploymentErrorCode.COMPOSE_MISSING, safeRemoteOutput(result))
        }
    }

    private suspend fun uploadArchive(
        ssh: DeploymentSsh,
        projectExpression: String,
        archivePath: String,
        archive: ByteArray,
        onProgress: (RelayDeploymentProgress) -> Unit,
    ) {
        val encoded = Base64.encodeToString(archive, Base64.NO_WRAP).toByteArray(StandardCharsets.US_ASCII)
        try {
            runChecked(
                ssh,
                "upload",
                inProject(projectExpression, "head -c ${encoded.size} | base64 -d > ${shellQuote(archivePath)}"),
                MAX_SMALL_OUTPUT_BYTES,
                UPLOAD_TIMEOUT_MS,
                encoded,
            )
            emit(onProgress, RelayDeploymentStage.UPLOADING, 5, archive.size.toLong(), archive.size.toLong())
        } finally {
            encoded.fill(0)
        }
    }

    private fun prepareCommand(): String = """
        set -eu
        umask 077
        if [ -L .pebrel-relay-deployment ] || [ -L relay ] || [ -L protocol ]; then
            printf 'unsupported_project_layout\n'; exit 41
        fi
        if [ -f .pebrel-relay-deployment ]; then
            [ "${'$'}(cat .pebrel-relay-deployment)" = 'pebrel-relay-deployment-v1' ] || {
                printf 'unsupported_project_layout\n'; exit 41;
            }
        elif [ -n "${'$'}(find . -mindepth 1 -maxdepth 1 -print -quit)" ]; then
            printf 'install_directory_not_empty\n'; exit 41
        fi
        mkdir -p relay protocol
        [ ! -L relay/private ] || { printf 'unsupported_project_layout\n'; exit 41; }
        printf '%s\n' 'pebrel-relay-deployment-v1' > .pebrel-relay-deployment
    """.trimIndent()

    private fun extractCommand(stage: String, archivePath: String): String = """
        set -eu
        rm -rf -- ${shellQuote(stage)}
        mkdir -- ${shellQuote(stage)}
        tar -tzf ${shellQuote(archivePath)} | while IFS= read -r entry; do
            case "${'$'}entry" in
                /*|../*|*/../*|*"/.."*) exit 42 ;;
            esac
        done
        tar -xzf ${shellQuote(archivePath)} -C ${shellQuote(stage)}
        test -f ${shellQuote("$stage/relay/compose.yaml")}
        test -f ${shellQuote("$stage/relay/Caddyfile")}
        test -f ${shellQuote("$stage/relay/Dockerfile")}
        test -f ${shellQuote("$stage/relay/init.mjs")}
        test -f ${shellQuote("$stage/relay/package.json")}
        test -f ${shellQuote("$stage/relay/package-lock.json")}
        test -f ${shellQuote("$stage/relay/connector.mjs")}
        test -f ${shellQuote("$stage/relay/protocol.mjs")}
        test -f ${shellQuote("$stage/relay/runtime-link.mjs")}
        test -f ${shellQuote("$stage/relay/server.mjs")}
        test -f ${shellQuote("$stage/protocol/bridge-policy.json")}
    """.trimIndent()

    private fun installSourceCommand(stage: String): String = """
        set -eu
        for file in Caddyfile Dockerfile compose.yaml connector.mjs init.mjs package-lock.json package.json protocol.mjs runtime-link.mjs server.mjs invite.mjs tls.mjs lan.mjs loopback-proxy.mjs pairing.mjs qr-page.mjs README.md THIRD-PARTY-NOTICES.md; do
            [ ! -L "relay/${'$'}file" ] || { printf 'unsupported_project_layout\n'; exit 41; }
            cp -- ${shellQuote("$stage/relay/")}${'$'}file relay/${'$'}file
        done
        [ ! -L protocol/bridge-policy.json ] || exit 41
        cp -- ${shellQuote("$stage/protocol/bridge-policy.json")} protocol/bridge-policy.json
    """.trimIndent()

    private fun composeConfigCommand(config: ValidatedRequest, uid: String, gid: String): String {
        val projectId = java.security.MessageDigest.getInstance("SHA-256")
            .digest(config.installDirectory.toByteArray()).take(8).joinToString("") { "%02x".format(it) }
        val environment = listOf(
            "COMPOSE_PROJECT_NAME=pebrel-$uid-$projectId",
            "PEBREL_RELAY_DOMAIN=${config.domain}",
            "PEBREL_RELAY_HTTP_PORT=${config.httpChallengePort}",
            "PEBREL_RELAY_HTTPS_PORT=${config.httpsPort}",
            "PEBREL_RELAY_UID=$uid",
            "PEBREL_RELAY_GID=$gid",
        ).joinToString(" ") { shellQuote(it) }
        return """
            set -eu
            umask 077
            [ ! -L relay/.pebrel-deploy.env ] || exit 41
            printf '%s\n' $environment > relay/.pebrel-deploy.env
            chmod 600 relay/.pebrel-deploy.env
        """.trimIndent()
    }

    private fun initializeCommand(config: ValidatedRequest, uid: String, gid: String): String {
        val initialize = """
            import { readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
            import { execFileSync } from 'node:child_process';
            const domain = process.env.PEBREL_RELAY_DOMAIN;
            const name = process.env.PEBREL_COMPUTER_NAME;
            const port = Number(process.env.PEBREL_RELAY_HTTPS_PORT);
            const origin = 'wss://' + domain + (port === 443 ? '' : ':' + port);
            const marker = 'private/.pebrel-last-pairing.json';
            const previous = existsSync(marker) ? JSON.parse(readFileSync(marker, 'utf8')) : null;
            let id;
            if (previous && previous.name === name && previous.url === origin) {
              id = previous.device;
              if (!/^[a-zA-Z0-9_-]{1,64}${'$'}/.test(id)) throw new Error('invalid_previous_pairing');
            } else {
              const before = new Set(existsSync('private') ? readdirSync('private') : []);
              execFileSync(process.execPath, ['init.mjs', '--url', 'wss://' + domain, '--name', name, '--output', 'private'], { stdio: 'ignore' });
              const file = readdirSync('private').find(value => /^phone-[a-f0-9]+\.txt${'$'}/.test(value) && !before.has(value));
              if (!file) throw new Error('missing_phone_configuration');
              id = file.slice(6, -4);
            }
            for (const file of ['private/phone-' + id + '.txt', 'private/computer-' + id + '.json']) {
              const value = JSON.parse(readFileSync(file, 'utf8'));
              value.url = origin;
              writeFileSync(file, JSON.stringify(value, null, 2) + '\n', { mode: 0o600 });
            }
            writeFileSync(marker, JSON.stringify({ name, url: origin, device: id }), { mode: 0o600 });
            console.log('PEBREL_DEVICE=' + id);
            console.log('PEBREL_INVITATION_BEGIN');
            console.log(readFileSync('private/phone-' + id + '.txt', 'utf8').trim());
            console.log('PEBREL_INVITATION_END');
            console.log('PEBREL_DESKTOP_BEGIN');
            console.log(readFileSync('private/computer-' + id + '.json', 'utf8').trim());
            console.log('PEBREL_DESKTOP_END');
        """.trimIndent()
        return """
            set -eu
            docker run --rm --user ${shellQuote("$uid:$gid")} \
                -e ${shellQuote("PEBREL_RELAY_DOMAIN=${config.domain}")} \
                -e ${shellQuote("PEBREL_RELAY_HTTPS_PORT=${config.httpsPort}")} \
                -e ${shellQuote("PEBREL_COMPUTER_NAME=${config.computerName}")} \
                -v "${'$'}PWD:/work" -w /work/relay node:22-alpine node --input-type=module -e ${shellQuote(initialize)}
        """.trimIndent()
    }

    private fun composeCommand(kind: ComposeKind): String {
        val binary = when (kind) {
            ComposeKind.PLUGIN -> "docker compose"
            ComposeKind.LEGACY -> "docker-compose"
        }
        return "$binary --env-file relay/.pebrel-deploy.env -f relay/compose.yaml"
    }

    private fun healthCommand(url: String): String = """
        set -eu
        if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
            printf 'health_client_missing\n'; exit 31
        fi
        for attempt in 1 2 3 4 5; do
            if command -v curl >/dev/null 2>&1; then
                curl --fail --silent --show-error --max-time 5 ${shellQuote(url)} && exit 0
            else
                wget -qO- --timeout=5 --tries=1 ${shellQuote(url)} && exit 0
            fi
            sleep 2
        done
        exit 32
    """.trimIndent()

    private fun markerCommand(): String = """
        set -eu
        printf '%s\n' 'pebrel-relay-deployment-v1' > .pebrel-relay-deployment
    """.trimIndent()

    private fun parseResult(
        config: ValidatedRequest,
        project: String,
        deviceId: String,
        invitation: String,
        desktop: String,
    ): RelayDeploymentResult {
        val profile = try { RelayProfile.parse(invitation.trim()) }
            catch (error: Exception) { throw RelayDeploymentException(RelayDeploymentErrorCode.RESULT_INVALID, cause = error) }
        val desktopObject = try { JSONObject(desktop.trim()) }
            catch (error: Exception) { throw RelayDeploymentException(RelayDeploymentErrorCode.RESULT_INVALID, cause = error) }
        val desktopDevice = desktopObject.optString("device")
        val desktopToken = desktopObject.optString("token")
        val desktopUrl = desktopObject.optString("url")
        val desktopName = desktopObject.optString("name")
        if (desktopDevice != deviceId || desktopDevice != profile.device ||
            desktopToken.isEmpty() || desktopToken == profile.token || desktopToken.length != 43 ||
            !Regex("[a-zA-Z0-9_-]{43}").matches(desktopToken) ||
            desktopUrl != profile.url || desktopName != config.computerName) {
            throw RelayDeploymentException(RelayDeploymentErrorCode.RESULT_INVALID)
        }
        val authority = if (config.httpsPort == 443) config.domain else "${config.domain}:${config.httpsPort}"
        val healthUrl = "https://$authority/healthz"
        val desktopFile = "computer-$deviceId.json"
        val command = buildString {
            append("node pairing.mjs --config ./")
            append(desktopFile)
            append(" --invitation ./phone-$deviceId.txt")
            if (config.allowInput) append(" --allow-input")
        }
        return RelayDeploymentResult(
            mobileProfile = profile,
            mobileInvitation = invitation.trim(),
            desktopConfig = desktop.trim(),
            mobileInvitationFileName = "phone-$deviceId.txt",
            desktopConfigFileName = desktopFile,
            desktopCommand = command,
            healthUrl = healthUrl,
            installDirectory = project,
        )
    }

    private suspend fun runChecked(
        ssh: DeploymentSsh,
        operation: String,
        command: String,
        maxOutput: Int,
        timeoutMs: Long,
        input: ByteArray? = null,
    ): CommandResult {
        val result = runCommand(ssh, command, maxOutput, timeoutMs, input)
        if (result.exitCode != 0) {
            val code = when (operation) {
                "project", "identity" -> RelayDeploymentErrorCode.REMOTE_PERMISSION
                "upload_prepare", "upload_chunk", "upload" -> RelayDeploymentErrorCode.UPLOAD_FAILED
                "extract", "install_source" -> RelayDeploymentErrorCode.EXTRACT_FAILED
                "compose_config" -> RelayDeploymentErrorCode.REMOTE_PERMISSION
                else -> RelayDeploymentErrorCode.UNKNOWN
            }
            throw RelayDeploymentException(code, safeRemoteOutput(result))
        }
        return result
    }

    private suspend fun runCommand(
        ssh: DeploymentSsh,
        command: String,
        maxOutput: Int,
        timeoutMs: Long,
        input: ByteArray? = null,
    ): CommandResult = withTimeout(timeoutMs) {
        val connection = try {
            ssh.open(minOf(timeoutMs, CONNECT_TIMEOUT_MS))
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: Exception) {
            throw mapSshFailure(error)
        }
        try {
            ssh.blocking(connection) { connection.openExec(command) }
            withContext(Dispatchers.IO) {
                val stdout = connection.input(false)
                val stderr = connection.input(true)
                kotlinx.coroutines.coroutineScope {
                    val out: Deferred<ByteArray> = async(Dispatchers.IO) {
                        ssh.blocking(connection) { readBounded(stdout, maxOutput) }
                    }
                    val err: Deferred<ByteArray> = async(Dispatchers.IO) {
                        ssh.blocking(connection) { readBounded(stderr, maxOutput) }
                    }
                    val code: Deferred<Int> = async(Dispatchers.IO) {
                        ssh.blocking(connection) { connection.awaitExit() }
                    }
                    val writer: Deferred<Unit>? = input?.let { payload ->
                        async(Dispatchers.IO) {
                            ssh.blocking(connection) { connection.output().write(payload) }
                        }
                    }
                    val exitCode = code.await()
                    writer?.await()
                    val bytes = awaitAll(out, err)
                    CommandResult(
                        exitCode,
                        String(bytes[0] as ByteArray, StandardCharsets.UTF_8),
                        String(bytes[1] as ByteArray, StandardCharsets.UTF_8),
                    )
                }
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: RelayDeploymentException) {
            throw error
        } catch (error: Exception) {
            throw mapSshFailure(error)
        } finally {
            ssh.release(connection)
        }
    }

    private fun mapSshFailure(error: Throwable): RelayDeploymentException {
        val kind = when (error) {
            is SshFailure -> error.kind
            is NativeSshException -> SshFailureKind.entries.firstOrNull { it.name == error.code }
            else -> null
        }
        val code = when (kind) {
            SshFailureKind.TIMEOUT -> RelayDeploymentErrorCode.SSH_TIMEOUT
            SshFailureKind.TRUST_REJECTED -> RelayDeploymentErrorCode.SSH_TRUST_REJECTED
            SshFailureKind.HOST_KEY_CHANGED -> RelayDeploymentErrorCode.SSH_HOST_KEY_CHANGED
            SshFailureKind.AUTH -> RelayDeploymentErrorCode.SSH_AUTH
            else -> RelayDeploymentErrorCode.SSH_FAILED
        }
        return RelayDeploymentException(code, cause = error)
    }

    private fun deploymentFailure(code: RelayDeploymentErrorCode, result: CommandResult) =
        RelayDeploymentException(code, safeRemoteOutput(result))

    private fun RelayDeploymentException.copyWith(code: RelayDeploymentErrorCode) =
        RelayDeploymentException(code, safeOutput, cause)

    private fun emit(
        callback: (RelayDeploymentProgress) -> Unit,
        stage: RelayDeploymentStage,
        step: Int,
        bytesSent: Long = 0,
        bytesTotal: Long = 0,
    ) {
        callback(RelayDeploymentProgress(stage, step, TOTAL_STEPS, bytesSent, bytesTotal))
    }

    private fun marker(text: String, name: String): String? {
        val prefix = "$name="
        return text.lineSequence().firstOrNull { it.startsWith(prefix) }?.removePrefix(prefix)?.trim()?.takeIf { it.isNotEmpty() }
    }

    private fun markerBlock(text: String, begin: String, end: String): String? {
        val start = text.indexOf("$begin\n")
        if (start < 0) return null
        val contentStart = start + begin.length + 1
        val contentEnd = text.indexOf("\n$end", contentStart)
        if (contentEnd < 0) return null
        return text.substring(contentStart, contentEnd).trim().takeIf { it.isNotEmpty() }
    }

    private fun safeRemoteOutput(result: CommandResult): String = redactOutput(
        (result.stdout + "\n" + result.stderr).trim(),
    )

    private fun redactOutput(text: String): String {
        if (text.isEmpty()) return ""
        return text.replace(Regex("[A-Za-z0-9_-]{43}"), "<redacted-token>")
            .filter { it == '\n' || it == '\r' || it == '\t' || it.code in 0x20..0x7e || it.code >= 0xa0 }
            .takeLast(2048)
    }

    private fun inProject(projectExpression: String, command: String) =
        "set -eu\ncd -- $projectExpression\n$command"

    private fun remotePathExpression(path: String): String = when {
        path == "~" -> "\"\$HOME\""
        path.startsWith("~/") -> "\"\$HOME\"/${shellQuote(path.removePrefix("~/"))}"
        else -> shellQuote(path)
    }

    private fun shellQuote(value: String): String = "'${value.replace("'", "'\"'\"'")}'"

    private fun readBounded(input: InputStream, maxBytes: Int): ByteArray {
        val output = ByteArrayOutputStream(minOf(maxBytes, 16 * 1024))
        val buffer = ByteArray(16 * 1024)
        var remaining = maxBytes
        while (true) {
            val count = input.read(buffer)
            if (count < 0) break
            if (remaining > 0) {
                val accepted = minOf(remaining, count)
                output.write(buffer, 0, accepted)
                remaining -= accepted
            }
        }
        return output.toByteArray()
    }
}
