package io.github.kuddev.pebrel.mobile.session

import kotlinx.coroutines.*
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class LiveSshCredentialsTest {
    @Test fun retryCredentialsAreBoundToTheirLoginAndErasedWhenTheTabCloses() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        lateinit var plaintext: CharArray
        lateinit var ciphertext: ByteArray
        val credentials = LiveSshCredentials(this, { key, secret ->
            assertEquals("login-A", key)
            plaintext = secret
            byteArrayOf(4, 5, 6).also { ciphertext = it }
        }, { key, encrypted ->
            assertEquals("login-A", key)
            assertArrayEquals(byteArrayOf(4, 5, 6), encrypted)
            charArrayOf('p', 'w')
        })
        try {
            credentials.remember("A", "login-A", charArrayOf('p', 'w'))
            assertNull(credentials.load("A", "different-login"))
            val restored = checkNotNull(credentials.load("A", "login-A"))
            assertArrayEquals(charArrayOf('p', 'w'), restored)
            restored.fill('\u0000')
            assertTrue(plaintext.all { it == '\u0000' })
            assertTrue(ciphertext.any { it != 0.toByte() })
            credentials.forget("A")
            assertTrue(ciphertext.all { it == 0.toByte() })
            assertNull(credentials.load("A", "login-A"))
        } finally { credentials.clear(); Dispatchers.resetMain() }
    }

    @Test fun closingBeforeEncryptionStartsNeverRevivesTheCredentialRecord() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        var encrypted = false
        val credentials = LiveSshCredentials(this, { _, _ -> encrypted = true; byteArrayOf(1) }, { _, _ -> charArrayOf('p') })
        try {
            credentials.remember("A", "login-A", charArrayOf('p'))
            credentials.forget("A")
            advanceUntilIdle()
            assertFalse(encrypted)
            assertNull(credentials.load("A", "login-A"))
        } finally { credentials.clear(); Dispatchers.resetMain() }
    }
}
