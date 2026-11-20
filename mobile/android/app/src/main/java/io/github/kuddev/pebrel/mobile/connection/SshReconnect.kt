package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*

/** App-owned retries continue while the foreground session service owns disconnected tabs. */
internal class SshReconnect(private val scope: CoroutineScope, private val retry: (String) -> Unit) {
    private val jobs = mutableMapOf<String, Job>()
    private val attempts = mutableMapOf<String, Int>()

    fun schedule(id: String, immediate: Boolean = false) {
        if (immediate) cancel(id) else if (jobs[id]?.isActive == true) return
        val attempt = attempts[id] ?: 0
        attempts[id] = (attempt + 1).coerceAtMost(6)
        jobs[id] = scope.launch(Dispatchers.Main.immediate) {
            delay(if (immediate) 1 else (1000L shl attempt).coerceAtMost(30_000))
            jobs.remove(id)
            retry(id)
        }
    }
    fun cancel(id: String) { jobs.remove(id)?.cancel() }
    fun forget(id: String) { cancel(id); attempts.remove(id) }
    fun clear() { jobs.values.forEach(Job::cancel); jobs.clear(); attempts.clear() }

    companion object {
        fun retryable(failure: SshFailureKind?) = failure in setOf(
            SshFailureKind.NETWORK, SshFailureKind.TIMEOUT, SshFailureKind.UNKNOWN_HOST, SshFailureKind.REFUSED,
        )
    }
}
