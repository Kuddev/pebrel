package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import org.commonmark.ext.gfm.strikethrough.Strikethrough
import org.commonmark.ext.gfm.strikethrough.StrikethroughExtension
import org.commonmark.ext.gfm.tables.TableBlock
import org.commonmark.ext.gfm.tables.TableCell
import org.commonmark.ext.gfm.tables.TableHead
import org.commonmark.ext.gfm.tables.TableRow
import org.commonmark.ext.gfm.tables.TablesExtension
import org.commonmark.ext.task.list.items.TaskListItemMarker
import org.commonmark.ext.task.list.items.TaskListItemsExtension
import org.commonmark.node.*
import org.commonmark.parser.Parser

/** One rendered Markdown block. Built off the main thread; holds no Compose state. */
@Immutable
internal sealed interface ChatBlock {
    data class Paragraph(val text: AnnotatedString) : ChatBlock
    data class Heading(val level: Int, val text: AnnotatedString) : ChatBlock
    data class Quote(val blocks: List<ChatBlock>) : ChatBlock
    data class Items(val ordered: Boolean, val start: Int, val items: List<ChatListItem>) : ChatBlock
    data class Code(val language: String, val text: String) : ChatBlock
    /** Display formulas stay as copyable TeX source; the rich reader typesets them. */
    data class Math(val source: String) : ChatBlock
    data class Table(val header: List<AnnotatedString>, val rows: List<List<AnnotatedString>>,
                     val alignments: List<TextAlign>) : ChatBlock
    data object Rule : ChatBlock
}

@Immutable
internal data class ChatListItem(val blocks: List<ChatBlock>, val checked: Boolean?)

/** [rich] marks content (formulas, diagrams) that the native view shows as source only. */
@Immutable
internal data class ChatDocument(val blocks: List<ChatBlock>, val rich: Boolean)

@Immutable
internal data class ChatMarkdownStyle(val link: Color, val codeBackground: Color, val codeFont: FontFamily,
                                      val imageLabel: String)

// Shares the reader's formula parsers, so `_` and `\` inside TeX are not rewritten as Markdown.
private val chatParser: Parser by lazy {
    Parser.builder()
        .extensions(listOf(TablesExtension.create(), StrikethroughExtension.create(), TaskListItemsExtension.create()))
        .customInlineContentParserFactory(ReaderInlineMathFactory)
        .customBlockParserFactory(ReaderBlockMathFactory)
        .build()
}

internal fun parseChatMarkdown(source: String, style: ChatMarkdownStyle): ChatDocument {
    val builder = ChatBlockBuilder(style)
    val blocks = builder.blocks(chatParser.parse(source))
    return ChatDocument(blocks, builder.rich)
}

private class ChatBlockBuilder(private val style: ChatMarkdownStyle) {
    var rich = false
        private set

    fun blocks(parent: Node): List<ChatBlock> = buildList {
        var node = parent.firstChild
        while (node != null) {
            block(node)?.let(::add)
            node = node.next
        }
    }

    private fun block(node: Node): ChatBlock? = when (node) {
        is Paragraph -> ChatBlock.Paragraph(inline(node))
        is Heading -> ChatBlock.Heading(node.level, inline(node))
        is BlockQuote -> ChatBlock.Quote(blocks(node))
        is BulletList -> ChatBlock.Items(false, 1, items(node))
        is OrderedList -> ChatBlock.Items(true, node.markerStartNumber ?: 1, items(node))
        is FencedCodeBlock -> code(node.literal, node.info.orEmpty().substringBefore(' '))
        is IndentedCodeBlock -> code(node.literal, "")
        is HtmlBlock -> ChatBlock.Code("html", node.literal.trimEnd('\n'))
        is ThematicBreak -> ChatBlock.Rule
        is TableBlock -> table(node)
        is ReaderBlockMath -> if (node.closed) {
            rich = true
            ChatBlock.Math(node.source.toString().trim('\n'))
        } else ChatBlock.Paragraph(AnnotatedString(node.delimiter + node.source))
        // An unknown extension block keeps its text instead of disappearing.
        else -> plainText(node).takeIf(String::isNotBlank)?.let { ChatBlock.Paragraph(AnnotatedString(it)) }
    }

    private fun code(literal: String, language: String): ChatBlock {
        val text = literal.trimEnd('\n')
        val name = language.lowercase()
        if (name in setOf("math", "latex", "tex")) {
            rich = true
            return ChatBlock.Math(text)
        }
        if (name == "mermaid") rich = true
        return ChatBlock.Code(language, text)
    }

    private fun items(list: Node): List<ChatListItem> = buildList {
        var item = list.firstChild
        while (item != null) {
            val marker = item.firstChild as? TaskListItemMarker
            add(ChatListItem(blocks(item), marker?.isChecked))
            item = item.next
        }
    }

    private fun table(node: TableBlock): ChatBlock {
        val header = mutableListOf<AnnotatedString>()
        val alignments = mutableListOf<TextAlign>()
        val rows = mutableListOf<List<AnnotatedString>>()
        var section = node.firstChild
        while (section != null) {
            var row = section.firstChild
            while (row != null) {
                if (row is TableRow) {
                    val cells = mutableListOf<AnnotatedString>()
                    var cell = row.firstChild
                    while (cell != null) {
                        if (cell is TableCell) {
                            cells += inline(cell)
                            if (section is TableHead) alignments += when (cell.alignment) {
                                TableCell.Alignment.CENTER -> TextAlign.Center
                                TableCell.Alignment.RIGHT -> TextAlign.End
                                else -> TextAlign.Start
                            }
                        }
                        cell = cell.next
                    }
                    if (section is TableHead) header += cells else rows += cells
                }
                row = row.next
            }
            section = section.next
        }
        return ChatBlock.Table(header, rows, alignments)
    }

    private fun inline(parent: Node): AnnotatedString = buildAnnotatedString { appendChildren(parent) }

    private fun AnnotatedString.Builder.appendChildren(parent: Node) {
        var node = parent.firstChild
        while (node != null) {
            appendInline(node)
            node = node.next
        }
    }

    private fun AnnotatedString.Builder.appendInline(node: Node) {
        when (node) {
            is Text -> append(node.literal)
            is Code -> withStyle(SpanStyle(fontFamily = style.codeFont, background = style.codeBackground)) {
                append(node.literal)
            }
            is Emphasis -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { appendChildren(node) }
            is StrongEmphasis -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { appendChildren(node) }
            is Strikethrough -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough)) { appendChildren(node) }
            // The screen's UriHandler decides where a destination may open; Markdown only names it.
            is Link -> withLink(link(node.destination)) { appendChildren(node) }
            is Image -> withLink(link(node.destination)) {
                append("${style.imageLabel} · ${plainText(node).ifBlank { node.destination }}")
            }
            is SoftLineBreak -> append(' ')
            is HardLineBreak -> append('\n')
            is HtmlInline -> append(node.literal)
            is ReaderInlineMath -> {
                rich = true
                withStyle(SpanStyle(fontFamily = style.codeFont, background = style.codeBackground)) { append(node.source) }
            }
            is TaskListItemMarker -> Unit
            else -> appendChildren(node)
        }
    }

    private fun link(destination: String) = LinkAnnotation.Url(destination,
        TextLinkStyles(SpanStyle(color = style.link, textDecoration = TextDecoration.Underline)))
}

private fun plainText(node: Node): String = buildString {
    node.accept(object : AbstractVisitor() {
        override fun visit(text: Text) { append(text.literal) }
        override fun visit(code: Code) { append(code.literal) }
        override fun visit(line: SoftLineBreak) { append(' ') }
    })
}
