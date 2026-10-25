package io.github.kuddev.pebrel.mobile.ui

import androidx.annotation.DrawableRes
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R

@Composable
fun ConnectionOnboardingScreen(
    onComputer: () -> Unit,
    onSshHost: () -> Unit,
    onLocal: () -> Unit,
    onLater: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 22.dp)) {
            Image(painterResource(R.drawable.ic_pebrel), null, Modifier.size(40.dp))
            Text(stringResource(R.string.onboarding_connection_title), fontSize = 26.sp, lineHeight = 36.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(top = 26.dp).semantics { heading() })
            Text(stringResource(R.string.onboarding_connection_hint), fontSize = 14.sp, lineHeight = 21.sp,
                color = colors.onSurfaceVariant, modifier = Modifier.padding(top = 8.dp))
            Column(Modifier.padding(top = 26.dp).workspaceFrame()) {
                OnboardingConnectionRow(R.drawable.ic_monitor, stringResource(R.string.onboarding_my_computer),
                    stringResource(R.string.onboarding_my_computer_hint), colors.primary, onComputer)
            }
            Text(stringResource(R.string.onboarding_other_ways), fontSize = 13.sp, fontWeight = FontWeight.Medium,
                color = colors.onSurfaceVariant,
                modifier = Modifier.padding(start = 2.dp, top = 28.dp, bottom = 8.dp).semantics { heading() })
            Column(Modifier.workspaceFrame()) {
                OnboardingConnectionRow(R.drawable.ic_server, stringResource(R.string.onboarding_ssh_host),
                    stringResource(R.string.new_ssh_hint), colors.onSurfaceVariant, onSshHost)
                HorizontalDivider(Modifier.padding(start = 48.dp), color = colors.outlineVariant)
                OnboardingConnectionRow(R.drawable.ic_phone, stringResource(R.string.onboarding_phone_terminal),
                    stringResource(R.string.onboarding_phone_terminal_hint), colors.onSurfaceVariant, onLocal)
            }
        }
        TextButton(onClick = onLater,
            modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 2.dp, bottom = 20.dp)
                .fillMaxWidth().heightIn(min = 48.dp)) {
            Text(stringResource(R.string.onboarding_later), fontSize = 14.sp)
        }
    }
}

@Composable
private fun OnboardingConnectionRow(
    @DrawableRes icon: Int,
    title: String,
    detail: String,
    iconColor: Color,
    onClick: () -> Unit,
) {
    Row(Modifier.fillMaxWidth().clickable(role = Role.Button, onClick = onClick)
        .heightIn(min = 72.dp).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Glyph(icon, Modifier.size(20.dp), iconColor)
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 15.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium)
            Text(detail, fontSize = 12.sp, lineHeight = 18.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 2.dp))
        }
        Glyph(R.drawable.ic_chevron, Modifier.size(14.dp))
    }
}
