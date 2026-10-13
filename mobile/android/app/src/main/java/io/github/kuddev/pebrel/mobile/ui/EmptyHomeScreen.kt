package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R

@Composable
internal fun EmptyHomeScreen(onComputer: () -> Unit, onSshHost: () -> Unit, onLocal: () -> Unit) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val stackActions = maxWidth < 360.dp || LocalDensity.current.fontScale > 1.1f
        // 短屏减少留白，大字体仍可滚动；入口不依赖固定屏高才能触达。
        val topSpace = if (maxHeight < 480.dp) 40.dp else 112.dp
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(start = 28.dp, end = 28.dp, top = topSpace, bottom = 28.dp)) {
            Text(stringResource(R.string.home_empty_title), fontSize = 17.sp, lineHeight = 26.sp,
                fontWeight = FontWeight.Medium, modifier = Modifier.semantics { heading() })
            Text(stringResource(R.string.home_empty_hint), fontSize = 12.sp, lineHeight = 20.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 8.dp))
            Button(onClick = onComputer, shape = RoundedCornerShape(14.dp),
                modifier = Modifier.padding(top = 22.dp).fillMaxWidth().heightIn(min = 48.dp)) {
                Text(stringResource(R.string.computer_connect), fontSize = 14.sp, lineHeight = 20.sp,
                    textAlign = TextAlign.Center)
            }
            Spacer(Modifier.height(8.dp))
            if (stackActions) {
                EmptyHomeAction(R.string.add_ssh, onSshHost, Modifier.fillMaxWidth())
                EmptyHomeAction(R.string.home_open_local, onLocal, Modifier.fillMaxWidth())
            } else {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    EmptyHomeAction(R.string.add_ssh, onSshHost, Modifier.weight(1f))
                    EmptyHomeAction(R.string.home_open_local, onLocal, Modifier.weight(1f))
                }
            }
        }
    }
}

@Composable
private fun EmptyHomeAction(label: Int, onClick: () -> Unit, modifier: Modifier) {
    TextButton(onClick, modifier.heightIn(min = 48.dp), contentPadding = PaddingValues(8.dp)) {
        Text(stringResource(label), fontSize = 14.sp, lineHeight = 20.sp, textAlign = TextAlign.Center)
    }
}
