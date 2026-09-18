package io.github.kuddev.pebrel.mobile.connection

/** Upload counts are bytes handed to the SSH channel, not proof of remote installation. */
data class RelayServiceProgress(val stage: String, val sent: Int = 0, val total: Int = 0)

enum class InstallStepState { WAITING, ACTIVE, DONE, FAILED, CANCELLED }

data class RelayInstallProgress(
    val step: Int = 1,
    val update: RelayServiceProgress = RelayServiceProgress("connecting"),
    val finished: Boolean = false,
    val failed: Boolean = false,
    val cancelled: Boolean = false,
) {
    fun advance(next: RelayServiceProgress): RelayInstallProgress {
        if (finished || failed || cancelled) return this
        val nextStep = when (next.stage) {
            "connecting" -> 1
            "checking" -> 2
            "uploading", "uploaded" -> 3
            "initializing", "installing", "starting", "verifying", "ready" -> 4
            else -> return this
        }
        // The service emits its own preflight after upload. Never move the UI backwards.
        if (nextStep < step) return this
        if (next.stage == "uploading" && update.stage == "uploaded") return this
        if (next.stage == "uploading" && next.sent < update.sent) return this
        return copy(step = nextStep, update = next)
    }

    fun state(number: Int): InstallStepState = when {
        finished || number < step -> InstallStepState.DONE
        number > step -> InstallStepState.WAITING
        failed -> InstallStepState.FAILED
        cancelled -> InstallStepState.CANCELLED
        else -> InstallStepState.ACTIVE
    }
}
