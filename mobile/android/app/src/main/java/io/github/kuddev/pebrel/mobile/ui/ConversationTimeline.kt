package io.github.kuddev.pebrel.mobile.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.kuddev.pebrel.mobile.R

/** What timeline rows may do; the screen owns transport, clipboard and navigation. */
@Stable
internal class ChatActions(
    val clipboard: ReaderClipboard,
    val onReader: (String) -> Unit,
    val onTerminal: () -> Unit,
)

private const val PREVIEW_LINES = 40

@Composable
internal fun ChatTimeline(
    items: List<ChatItem>,
    working: Boolean,
    activity: String,
    agentName: String,
    listState: LazyListState,
    contentPadding: PaddingValues,
    hasOlder: Boolean,
    olderEnabled: Boolean,
    onOlder: () -> Unit,
    actions: ChatActions,
    modifier: Modifier = Modifier,
) {
    LazyColumn(modifier.testTag("conversation-timeline"), state = listState, contentPadding = contentPadding,
        verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (hasOlder) item("older", contentType = "older") {
            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                TextButton(onOlder, enabled = olderEnabled) { Text(stringResource(R.string.chat_older)) }
            }
        }
        items(items, key = { it.key }, contentType = { it::class }) { item ->
            when (item) {
                is ChatItem.User -> UserMessage(item, actions)
                is ChatItem.Assistant -> AssistantMessage(item, agentName, actions)
                is ChatItem.Tool -> ToolCard(item, actions)
                is ChatItem.Changes -> ChangesRow(item)
                is ChatItem.Question -> AskedQuestionCard(item, actions.onTerminal)
            }
        }
        if (working) item("working", contentType = "working") { WorkingRow(activity) }
    }
}

@Composable
private fun UserMessage(item: ChatItem.User, actions: ChatActions) {
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp, bottomStart = 18.dp, bottomEnd = 6.dp)
    Row(Modifier.fillMaxWidth().padding(start = 40.dp), horizontalArrangement = Arrangement.End) {
        Column(Modifier.clip(shape).background(colors.primary.copy(alpha = .13f).compositeOver(colors.background))
            .padding(horizontal = 14.dp, vertical = 10.dp)) {
            SelectionContainer { ChatBlocks(item.document.blocks, "${item.key}/", actions) }
            if (item.truncated) ClippedNotice()
        }
    }
}

@Composable
private fun AssistantMessage(item: ChatItem.Assistant, agentName: String, actions: ChatActions) {
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (item.startsTurn) Text(agentName, style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
        SelectionContainer { ChatBlocks(item.document.blocks, "${item.key}/", actions) }
        if (item.truncated) ClippedNotice()
        if (item.document.rich) DisableSelection {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.chat_rich_hint), Modifier.weight(1f), style = MaterialTheme.typography.labelMedium,
                    color = glassPalette().textTertiary)
                TextButton({ actions.onReader(item.source) }) { Text(stringResource(R.string.chat_rich_reader)) }
            }
        }
    }
}

@Composable
private fun ClippedNotice() {
    Text(stringResource(R.string.chat_content_clipped), Modifier.padding(top = 6.dp), style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant)
}

private val ChatBody: TextStyle @Composable get() = MaterialTheme.typography.bodyLarge.copy(lineHeight = 24.sp)

@Composable
internal fun ChatBlocks(blocks: List<ChatBlock>, path: String, actions: ChatActions) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        blocks.forEachIndexed { index, block -> ChatBlockView(block, "$path$index", actions) }
    }
}

@Composable
private fun ChatBlockView(block: ChatBlock, path: String, actions: ChatActions) {
    val colors = MaterialTheme.colorScheme
    when (block) {
        is ChatBlock.Paragraph -> Text(block.text, style = ChatBody, color = colors.onSurface)
        is ChatBlock.Heading -> Text(block.text, Modifier.padding(top = 4.dp), color = colors.onSurface, style = when (block.level) {
            1 -> MaterialTheme.typography.titleLarge
            2 -> MaterialTheme.typography.titleMedium
            else -> MaterialTheme.typography.titleSmall
        })
        is ChatBlock.Quote -> Row(Modifier.height(IntrinsicSize.Min)) {
            Box(Modifier.width(3.dp).fillMaxHeight().clip(RoundedCornerShape(2.dp)).background(colors.outlineVariant))
            Column(Modifier.padding(start = 10.dp)) {
                CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides colors.onSurfaceVariant) {
                    ChatBlocks(block.blocks, "$path/", actions)
                }
            }
        }
        is ChatBlock.Items -> Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            block.items.forEachIndexed { index, item ->
                Row {
                    val marker = when {
                        item.checked == true -> "☑"
                        item.checked == false -> "☐"
                        block.ordered -> "${block.start + index}."
                        else -> "•"
                    }
                    Text(marker, Modifier.widthIn(min = 22.dp).padding(end = 6.dp), style = ChatBody,
                        color = colors.onSurfaceVariant, textAlign = TextAlign.End)
                    Box(Modifier.weight(1f)) { ChatBlocks(item.blocks, "$path/$index/", actions) }
                }
            }
        }
        is ChatBlock.Code -> CodeBox(block.text, block.language.ifBlank { null }, path.hashCode(), actions.clipboard)
        is ChatBlock.Math -> CodeBox(block.source, "LaTeX", path.hashCode(), actions.clipboard)
        is ChatBlock.Table -> ChatTable(block)
        ChatBlock.Rule -> HorizontalDivider(thickness = GlassHairline, color = glassPalette().hairline)
    }
}

/** Raw text with its own copy action. Long output starts collapsed so layout stays bounded. */
@Composable
internal fun CodeBox(text: String, label: String?, copyKey: Int, clipboard: ReaderClipboard, preview: Boolean = false) {
    val colors = MaterialTheme.colorScheme
    val palette = glassPalette()
    val shape = RoundedCornerShape(12.dp)
    val lines = remember(text) { text.count { it == '\n' } + 1 }
    var all by rememberSaveable(copyKey) { mutableStateOf(false) }
    val shown = remember(text, all) {
        if (!preview || all || lines <= PREVIEW_LINES) text else text.lineSequence().take(PREVIEW_LINES).joinToString("\n")
    }
    Column(Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerHighest.copy(alpha = .7f))
        .border(GlassHairline, palette.hairline, shape)) {
        // Selecting a message copies its words, not the controls' labels.
        DisableSelection {
            Row(Modifier.fillMaxWidth().padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(label.orEmpty(), Modifier.weight(1f), style = MaterialTheme.typography.labelSmall, color = palette.textTertiary,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
                val copied = clipboard.copied == copyKey
                TextButton({ clipboard.copy(text, copyKey) }, contentPadding = PaddingValues(horizontal = 12.dp)) {
                    Text(stringResource(if (copied) R.string.reader_copied else R.string.reader_copy),
                        style = MaterialTheme.typography.labelMedium)
                }
            }
        }
        Text(shown, Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, bottom = 10.dp),
            style = TextStyle(fontFamily = LocalTerminalFont.current, fontSize = 13.sp, lineHeight = 19.sp),
            color = colors.onSurface, softWrap = false)
        if (preview && lines > PREVIEW_LINES) DisableSelection {
            TextButton({ all = !all }, Modifier.padding(start = 4.dp)) {
                Text(if (all) stringResource(R.string.chat_show_less) else stringResource(R.string.chat_show_all, lines),
                    style = MaterialTheme.typography.labelMedium)
            }
        }
    }
}

/** Columns size to their widest cell, so header and body stay aligned while the table scrolls. */
@Composable
private fun ChatTable(table: ChatBlock.Table) {
    val colors = MaterialTheme.colorScheme
    val palette = glassPalette()
    val shape = RoundedCornerShape(10.dp)
    val columns = maxOf(table.header.size, table.rows.maxOfOrNull { it.size } ?: 0)
    if (columns == 0) return
    val rows = listOf(table.header) + table.rows
    // Row edges measured in this frame's layout, read by the same frame's draw.
    val edges = remember(table) { IntArray(rows.size + 1) }
    val headerFill = colors.onSurface.copy(alpha = .05f)
    val rule = palette.hairline
    Box(Modifier.clip(shape).border(GlassHairline, rule, shape).horizontalScroll(rememberScrollState())) {
        Layout(content = {
            rows.forEachIndexed { rowIndex, row ->
                for (column in 0 until columns) {
                    val header = rowIndex == 0 && table.header.isNotEmpty()
                    Text(row.getOrNull(column) ?: AnnotatedString(""), Modifier.padding(horizontal = 10.dp, vertical = 7.dp),
                        style = MaterialTheme.typography.bodyMedium.copy(fontWeight = if (header) FontWeight.SemiBold else null),
                        color = colors.onSurface, textAlign = table.alignments.getOrNull(column) ?: TextAlign.Start)
                }
            }
        }, modifier = Modifier.drawBehind {
            if (table.header.isNotEmpty()) drawRect(headerFill, size = Size(size.width, edges[1].toFloat()))
            val stroke = GlassHairline.toPx()
            for (row in 1 until rows.size) drawRect(rule, Offset(0f, edges[row] - stroke / 2), Size(size.width, stroke))
        }) { measurables, _ ->
            val maxCell = 280.dp.roundToPx()
            val widths = IntArray(columns) { column ->
                rows.indices.maxOf { row -> measurables[row * columns + column].maxIntrinsicWidth(Constraints.Infinity) }
                    .coerceAtMost(maxCell)
            }
            val placeables = measurables.mapIndexed { index, cell -> cell.measure(Constraints.fixedWidth(widths[index % columns])) }
            for (row in rows.indices) {
                edges[row + 1] = edges[row] + (0 until columns).maxOf { placeables[row * columns + it].height }
            }
            layout(widths.sum(), edges[rows.size]) {
                for (row in rows.indices) {
                    var x = 0
                    for (column in 0 until columns) {
                        placeables[row * columns + column].place(x, edges[row])
                        x += widths[column]
                    }
                }
            }
        }
    }
}

@DrawableRes
private fun toolIcon(kind: ToolKind): Int = when (kind) {
    ToolKind.Shell -> R.drawable.ic_terminal
    ToolKind.Read -> R.drawable.ic_file_text
    ToolKind.Edit -> R.drawable.ic_edit
    ToolKind.Search -> R.drawable.ic_search
    ToolKind.Web -> R.drawable.ic_cloud
    ToolKind.Task -> R.drawable.ic_brackets_curly
    ToolKind.Todo -> R.drawable.ic_list_dashes
    ToolKind.Question -> R.drawable.ic_chat
    ToolKind.Other -> R.drawable.ic_command
}

@Composable
private fun toolStateLabel(state: ToolState): String = stringResource(when (state) {
    ToolState.Running -> R.string.chat_tool_running
    ToolState.Waiting -> R.string.chat_tool_waiting
    ToolState.Pending -> R.string.chat_tool_pending
    ToolState.Done -> R.string.chat_tool_done
    ToolState.Failed -> R.string.chat_tool_failed
})

@Composable
private fun ToolCard(item: ChatItem.Tool, actions: ChatActions) {
    val colors = MaterialTheme.colorScheme
    val palette = glassPalette()
    val motion = rememberPebrelMotion()
    val shape = RoundedCornerShape(14.dp)
    var expanded by rememberSaveable(item.key) { mutableStateOf(false) }
    val failed = item.state == ToolState.Failed
    val stateLabel = toolStateLabel(item.state)
    val name = toolDisplayName(item.name) ?: stringResource(R.string.chat_tool_result)
    val rotation by animateFloatAsState(if (expanded) 90f else 0f, motion.toggleSpring(), label = "tool-chevron")
    Column(Modifier.fillMaxWidth().animateContentSize(motion.expandSpring()).clip(shape)
        .background(if (failed) colors.error.copy(alpha = .08f) else colors.onSurface.copy(alpha = .035f))
        .border(GlassHairline, if (failed) colors.error.copy(alpha = .4f) else palette.hairline, shape)) {
        Row(Modifier.fillMaxWidth().pressClickable(role = Role.Button) { expanded = !expanded }
            .semantics { stateDescription = stateLabel }
            .heightIn(min = 48.dp).padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Glyph(toolIcon(item.kind), Modifier.size(18.dp), if (failed) colors.error else colors.onSurfaceVariant)
            Column(Modifier.weight(1f)) {
                Text(name, style = MaterialTheme.typography.labelLarge.copy(fontWeight = FontWeight.SemiBold),
                    color = colors.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (item.summary.isNotEmpty()) Text(item.summary, style = MaterialTheme.typography.bodySmall.copy(
                    fontFamily = LocalTerminalFont.current), color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            ToolStateMark(item.state, stateLabel)
            Glyph(R.drawable.ic_chevron, Modifier.size(14.dp).graphicsLayer { rotationZ = rotation })
        }
        if (expanded) Column(Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, bottom = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (item.input.isNotBlank()) CodeBox(item.input, stringResource(R.string.chat_tool_input),
                "${item.key}/input".hashCode(), actions.clipboard, preview = true)
            item.output?.takeIf(String::isNotEmpty)?.let { output ->
                CodeBox(output, stringResource(R.string.chat_tool_output), "${item.key}/output".hashCode(), actions.clipboard, preview = true)
            }
            if (item.truncated) ClippedNotice()
        }
    }
}

@Composable
private fun ToolStateMark(state: ToolState, label: String) {
    val colors = MaterialTheme.colorScheme
    when (state) {
        ToolState.Running -> CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 1.5.dp, color = colors.primary)
        ToolState.Done -> Glyph(R.drawable.ic_check, Modifier.size(16.dp), colors.onSurfaceVariant)
        // Words, not hue alone, for the states that ask for attention.
        ToolState.Failed -> Text(label, style = MaterialTheme.typography.labelMedium, color = colors.error)
        ToolState.Waiting -> Text(label, style = MaterialTheme.typography.labelMedium, color = colors.primary)
        ToolState.Pending -> Box(Modifier.size(8.dp).border(1.dp, colors.onSurfaceVariant, CircleShape))
    }
}

/** Display-only: answering goes through the desktop's prompt identity, or the terminal. */
@Composable
private fun AskedQuestionCard(item: ChatItem.Question, onTerminal: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val palette = glassPalette()
    val shape = RoundedCornerShape(16.dp)
    Column(Modifier.fillMaxWidth().clip(shape).background(colors.primary.copy(alpha = .06f))
        .border(GlassHairline, palette.hairline, shape).padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Glyph(R.drawable.ic_chat, Modifier.size(16.dp), colors.primary)
            Text(stringResource(R.string.chat_question), Modifier.weight(1f), style = MaterialTheme.typography.labelMedium,
                color = colors.primary)
            if (item.answered) Text(stringResource(R.string.chat_question_answered), style = MaterialTheme.typography.labelMedium,
                color = colors.onSurfaceVariant)
        }
        item.questions.forEach { question ->
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                if (question.header.isNotEmpty()) Text(question.header, style = MaterialTheme.typography.labelSmall,
                    color = palette.textTertiary)
                Text(question.question, style = MaterialTheme.typography.titleSmall, color = colors.onSurface)
                if (question.multiple) Text(stringResource(R.string.chat_multiple_choice), style = MaterialTheme.typography.labelMedium,
                    color = colors.onSurfaceVariant)
                question.options.forEachIndexed { index, option -> OptionRow(index + 1, option.label, option.description) }
            }
        }
        if (!item.answered) TextButton(onTerminal, Modifier.align(Alignment.End)) { Text(stringResource(R.string.chat_question_terminal)) }
    }
}

@Composable
internal fun OptionNumber(number: Int, selected: Boolean = false) {
    val colors = MaterialTheme.colorScheme
    Box(Modifier.size(24.dp).clip(CircleShape).background(if (selected) colors.primary else colors.onSurface.copy(alpha = .08f)),
        contentAlignment = Alignment.Center) {
        Text("$number", style = MaterialTheme.typography.labelMedium, color = if (selected) colors.onPrimary else colors.onSurface)
    }
}

@Composable
private fun OptionRow(number: Int, label: String, description: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        OptionNumber(number)
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurface)
            if (description.isNotEmpty()) Text(description, style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun ChangesRow(item: ChatItem.Changes) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = .4f))
        .padding(horizontal = 12.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(pluralStringResource(R.plurals.chat_files_changed, item.files.size, item.files.size),
            style = MaterialTheme.typography.labelLarge)
        item.files.take(CHANGED_FILES).forEach { file ->
            Text(file.substringAfterLast('/'), style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (item.files.size > CHANGED_FILES) Text("+${item.files.size - CHANGED_FILES}", style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

private const val CHANGED_FILES = 5

@Composable
private fun WorkingRow(activity: String) {
    Row(Modifier.padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 1.5.dp, color = MaterialTheme.colorScheme.primary)
        Text(activity.ifEmpty { stringResource(R.string.chat_working) }, style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}
