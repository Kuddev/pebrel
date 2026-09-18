package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class DesktopReconnectTest {
    @Test fun backgroundPausesRetriesForegroundResumesAndClosePreventsResurrection() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val attempts = mutableListOf<String>()
        val reconnect = DesktopReconnect(this) { attempts += it }
        try {
            reconnect.foreground(true)
            repeat(20) { reconnect.schedule("pc") }
            runCurrent()
            advanceTimeBy(500); runCurrent()
            assertEquals(listOf("pc"), attempts)
            reconnect.schedule("pc")
            reconnect.foreground(false)
            advanceTimeBy(60_000); runCurrent()
            assertEquals(1, attempts.size)
            reconnect.foreground(true)
            reconnect.schedule("pc", immediate = true)
            advanceTimeBy(1); runCurrent()
            assertEquals(2, attempts.size)
            reconnect.schedule("pc")
            reconnect.forget("pc")
            advanceTimeBy(60_000); runCurrent()
            assertEquals(2, attempts.size)
        } finally { reconnect.clear(); Dispatchers.resetMain() }
    }

    @Test fun certificateAndAuthenticationFailuresNeverAutoRetry() {
        for (failure in listOf(DesktopFailureKind.AUTHENTICATION, DesktopFailureKind.CERTIFICATE_CHANGED,
            DesktopFailureKind.TLS, DesktopFailureKind.PROTOCOL)) assertFalse(DesktopReconnect.retryable(failure))
        for (failure in listOf(DesktopFailureKind.NETWORK, DesktopFailureKind.DISCONNECTED,
            DesktopFailureKind.PEER_OFFLINE, DesktopFailureKind.TIMEOUT)) assertTrue(DesktopReconnect.retryable(failure))
    }
}
