package io.github.kuddev.pebrel.mobile.connection

import kotlinx.coroutines.*

/** Main-thread owner. Only previously connected computers are enrolled by the
 * repository. Background pauses retries; foreground resumes them immediately.
 */
internal class DesktopReconnect(private val scope: CoroutineScope, private val retry: (String) -> Unit) {
    private val jobs = mutableMapOf<String, Job>()
    private val attempts = mutableMapOf<String, Int>()
    private var foreground = false

    fun foreground(value: Boolean) {
        foreground = value
        if (!value) { jobs.values.forEach(Job::cancel); jobs.clear() }
    }

    fun schedule(id: String, immediate: Boolean = false) {
        if (!foreground || jobs[id]?.isActive == true) return
        val attempt = attempts[id] ?: 0
        attempts[id] = (attempt + 1).coerceAtMost(6)
        jobs[id] = scope.launch(Dispatchers.Main.immediate) {
            delay(if (immediate) 1 else (500L shl attempt).coerceAtMost(15_000))
            jobs.remove(id)
            if (foreground) retry(id)
        }
    }

    fun forget(id: String) { jobs.remove(id)?.cancel(); attempts.remove(id) }
    fun clear() { jobs.values.forEach(Job::cancel); jobs.clear(); attempts.clear() }

    companion object {
        fun retryable(failure: DesktopFailureKind?) = failure !in setOf(
            DesktopFailureKind.AUTHENTICATION, DesktopFailureKind.CERTIFICATE_CHANGED,
            DesktopFailureKind.CERTIFICATE_DATE, DesktopFailureKind.TLS, DesktopFailureKind.PROTOCOL,
        )
    }
}
