package io.github.kuddev.pebrel.mobile.connection

/** One connection attempt tries each saved route once. Reconnect backoff owns later attempts. */
internal class RelayRouteAttempt(routes: List<RelayProfile>) {
    private val remaining = ArrayDeque(routes)
    val hasFallback: Boolean get() = remaining.isNotEmpty()

    fun next(failure: DesktopFailureKind? = null): RelayProfile? {
        if (failure != null && !DesktopReconnect.retryable(failure)) remaining.clear()
        return remaining.removeFirstOrNull()
    }

    companion object {
        fun forProfile(profile: RelayProfile, saved: RelayProfile?): RelayRouteAttempt {
            // A new invitation or changed address must prove itself before joining saved routes.
            val routes = if (saved?.connectionRoutes()?.any { it.sameConnection(profile) } == true)
                saved.connectionRoutes() else listOf(profile)
            return RelayRouteAttempt(routes)
        }
    }
}
