package io.github.kuddev.pebrel.mobile.session

import kotlinx.coroutines.*

/** Main-thread owner. Only encrypted records survive authentication; nothing is persisted. */
internal class LiveSshCredentials(
    private val scope: CoroutineScope,
    private val seal: (String, CharArray) -> ByteArray,
    private val unseal: (String, ByteArray) -> CharArray,
) {
    private class Entry(val hostKey: String) {
        var encrypted: ByteArray? = null
        var job: Job? = null
    }
    private val entries = mutableMapOf<String, Entry>()

    fun remember(id: String, hostKey: String, password: CharArray) {
        forget(id)
        val entry = Entry(hostKey)
        entries[id] = entry
        val secret = password.copyOf()
        entry.job = scope.launch(Dispatchers.Main.immediate) {
            try {
                // NonCancellable returns the result to its owner even when a tab closes during crypto.
                val encrypted = withContext(NonCancellable + Dispatchers.IO) { seal(hostKey, secret) }
                if (entries[id] === entry && isActive) entry.encrypted = encrypted else encrypted.fill(0)
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { /* A saved credential or an inline prompt remains available. */ }
            finally { secret.fill('\u0000') }
        }.also { job -> job.invokeOnCompletion { secret.fill('\u0000') } }
    }

    suspend fun load(id: String, hostKey: String): CharArray? {
        val entry = entries[id]?.takeIf { it.hostKey == hostKey } ?: return null
        entry.job?.join()
        val encrypted = entry.encrypted?.copyOf() ?: return null
        return try {
            val secret = withContext(NonCancellable + Dispatchers.IO) { unseal(hostKey, encrypted) }
            if (!currentCoroutineContext().isActive || entries[id] !== entry) {
                secret.fill('\u0000')
                currentCoroutineContext().ensureActive()
                null
            } else secret
        } finally { encrypted.fill(0) }
    }

    fun forget(id: String) {
        entries.remove(id)?.let { it.job?.cancel(); it.encrypted?.fill(0) }
    }
    fun clear() { entries.keys.toList().forEach(::forget) }
}
