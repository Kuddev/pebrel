package io.github.kuddev.pebrel.mobile.ui

import android.os.SystemClock
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.connection.*
import kotlinx.coroutines.delay

/** Always retain all four rows, including the precise failed/cancelled step. */
@Composable
internal fun RelayInstallSteps(progress: RelayInstallProgress, failure: Int?) {
    val titles = listOf(R.string.service_step_connect, R.string.service_step_check,
        R.string.service_step_upload, R.string.service_step_start)
    var seconds by remember(progress) { mutableLongStateOf(0) }
    LaunchedEffect(progress) {
        val start = SystemClock.elapsedRealtime()
        while (!progress.finished && !progress.failed && !progress.cancelled) {
            delay(1000)
            seconds = (SystemClock.elapsedRealtime() - start) / 1000
        }
    }
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        titles.forEachIndexed { index, title ->
            val state = progress.state(index + 1)
            val label = when (state) {
                InstallStepState.WAITING -> R.string.service_step_waiting
                InstallStepState.ACTIVE -> R.string.service_step_active
                InstallStepState.DONE -> R.string.service_step_done
                InstallStepState.FAILED -> R.string.service_operation_failed
                InstallStepState.CANCELLED -> R.string.service_step_cancelled
            }
            val color = when (state) {
                InstallStepState.ACTIVE -> MaterialTheme.colorScheme.onSurface
                InstallStepState.FAILED -> MaterialTheme.colorScheme.error
                else -> MaterialTheme.colorScheme.onSurfaceVariant
            }
            Text(stringResource(R.string.service_step_row, index + 1, stringResource(title), stringResource(label)),
                style = MaterialTheme.typography.bodySmall, color = color)
            if (index + 1 == progress.step && !progress.finished) {
                val update = progress.update
                if (update.total > 0) {
                    Text(stringResource(R.string.service_upload_bytes, update.sent / 1024, (update.total + 1023) / 1024),
                        style = MaterialTheme.typography.bodySmall, color = color)
                    LinearProgressIndicator(progress = { update.sent.toFloat() / update.total }, modifier = Modifier.fillMaxWidth())
                }
                if (state == InstallStepState.ACTIVE) {
                    Text(stringResource(serviceStageText(update.stage)), style = MaterialTheme.typography.bodySmall, color = color)
                    if (update.total == 0) LinearProgressIndicator(Modifier.fillMaxWidth())
                    if (seconds >= 5) Text(stringResource(R.string.service_step_wait_seconds, seconds),
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (state == InstallStepState.FAILED && failure != null) Text(stringResource(failure),
                    style = MaterialTheme.typography.bodySmall, color = color)
            }
        }
    }
}
