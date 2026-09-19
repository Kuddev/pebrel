package io.github.kuddev.pebrel.mobile.connection

import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import io.github.kuddev.pebrel.ssh.NativeSshException
import org.junit.Assert.assertEquals
import org.junit.Test

class SshStatusTest {
    @Test fun wrappedDnsFailureIsNotReportedAsAuthentication() {
        assertEquals(SshFailureKind.UNKNOWN_HOST, classifySshFailure(IOException("transport", UnknownHostException())))
    }
    @Test fun nestedTimeoutWinsOverGenericAuthenticationFailure() {
        assertEquals(SshFailureKind.TIMEOUT, classifySshFailure(NativeSshException("AUTH", SocketTimeoutException())))
    }
    @Test fun rejectedPasswordHasItsOwnActionableCategory() {
        assertEquals(SshFailureKind.AUTH, classifySshFailure(NativeSshException("AUTH")))
    }
    @Test fun refusedPortAndGenericDisconnectRemainDistinct() {
        assertEquals(SshFailureKind.REFUSED, classifySshFailure(ConnectException()))
        assertEquals(SshFailureKind.NETWORK, classifySshFailure(IOException()))
    }
    @Test fun explicitHostKeyRejectionSurvivesTransportWrapping() {
        val failure = IOException("outer", SshFailure(SshFailureKind.HOST_KEY_CHANGED, IOException("negotiation")))
        assertEquals(SshFailureKind.HOST_KEY_CHANGED, classifySshFailure(failure))
    }
    @Test fun localCryptoFailureIsNotBlamedOnServerAlgorithms() {
        assertEquals(SshFailureKind.CRYPTO, classifySshFailure(
            IOException(java.security.NoSuchAlgorithmException("local provider"))))
    }
    @Test fun arbitraryFailureDoesNotBlameCredentials() {
        assertEquals(SshFailureKind.UNKNOWN, classifySshFailure(IllegalStateException()))
    }
    @Test fun nativeErrorsKeepNegotiationChannelAndTrustDistinct() {
        listOf(SshFailureKind.NEGOTIATION, SshFailureKind.CHANNEL, SshFailureKind.TRUST_REJECTED).forEach {
            assertEquals(it, classifySshFailure(IOException("wrapper", NativeSshException(it.name))))
        }
        assertEquals(SshFailureKind.UNKNOWN, classifySshFailure(NativeSshException("FUTURE_CODE")))
    }
}
