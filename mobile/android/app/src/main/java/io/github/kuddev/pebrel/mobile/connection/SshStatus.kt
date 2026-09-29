package io.github.kuddev.pebrel.mobile.connection

import io.github.kuddev.pebrel.ssh.NativeSshException
import java.io.IOException
import java.net.ConnectException
import java.net.NoRouteToHostException
import java.net.SocketTimeoutException
import java.net.UnknownHostException

enum class SshStage { NETWORK, VERIFYING, AUTHENTICATING, OPENING_SHELL }
enum class SshFailureKind {
    UNKNOWN_HOST, TIMEOUT, REFUSED, AUTH, HOST_KEY_CHANGED, TRUST_REJECTED, CHANNEL, NETWORK, CRYPTO, NEGOTIATION, UNKNOWN,
}

class SshFailure(val kind: SshFailureKind, cause: Exception) : IOException(kind.name, cause)

/** Typed errors, never raw server messages, credentials or command content in UI. */
fun classifySshFailure(error: Throwable?): SshFailureKind {
    val causes = generateSequence(error) { it.cause }.take(12).toList()
    causes.filterIsInstance<SshFailure>().firstOrNull()?.let { return it.kind }
    return when {
        causes.any { it is UnknownHostException } -> SshFailureKind.UNKNOWN_HOST
        causes.any { it is SocketTimeoutException || it is java.util.concurrent.TimeoutException } -> SshFailureKind.TIMEOUT
        causes.any { it is ConnectException } -> SshFailureKind.REFUSED
        causes.any { it is java.security.GeneralSecurityException } -> SshFailureKind.CRYPTO
        causes.any { it is NativeSshException } -> causes.filterIsInstance<NativeSshException>().first().let {
            SshFailureKind.entries.firstOrNull { kind -> kind.name == it.code } ?: SshFailureKind.UNKNOWN
        }
        causes.any { it is NoRouteToHostException } -> SshFailureKind.NETWORK
        causes.any { it is IOException } -> SshFailureKind.NETWORK
        else -> SshFailureKind.UNKNOWN
    }
}
