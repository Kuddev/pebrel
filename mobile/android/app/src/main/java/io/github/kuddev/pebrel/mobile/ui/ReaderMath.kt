package io.github.kuddev.pebrel.mobile.ui

import org.commonmark.node.CustomBlock
import org.commonmark.node.CustomNode
import org.commonmark.parser.SourceLine
import org.commonmark.parser.beta.InlineContentParser
import org.commonmark.parser.beta.InlineContentParserFactory
import org.commonmark.parser.beta.ParsedInline
import org.commonmark.parser.beta.Scanner
import org.commonmark.parser.block.*

internal class ReaderInlineMath(val source: String, val display: Boolean) : CustomNode()
internal class ReaderBlockMath(val delimiter: String) : CustomBlock() {
    val source = StringBuilder()
    var closed = false
}

/** 在 CommonMark 的解析入口保护公式，避免下划线、反斜线先被 Markdown 改写。 */
internal object ReaderInlineMathFactory : InlineContentParserFactory {
    override fun getTriggerCharacters() = setOf('$', '\\')
    override fun create() = InlineContentParser { state ->
        val scanner = state.scanner()
        val dollar = scanner.peek() == '$'
        val display: Boolean
        val closer: String
        if (dollar) {
            scanner.next()
            display = scanner.next('$')
            if (scanner.peek() == '$') return@InlineContentParser ParsedInline.none()
            closer = if (display) "\$\$" else "\$"
        } else {
            scanner.next()
            display = when {
                scanner.next('[') -> true
                scanner.next('(') -> false
                else -> return@InlineContentParser ParsedInline.none()
            }
            closer = if (display) "\\]" else "\\)"
        }
        if (!display && (scanner.peek().isWhitespace() || scanner.peek() == Scanner.END)) {
            return@InlineContentParser ParsedInline.none()
        }
        val start = scanner.position()
        var count = 0
        while (scanner.hasNext() && count++ < 32_768) {
            val end = scanner.position()
            if (scanner.next(closer)) {
                val source = scanner.getSource(start, end).content
                // 与货币文本区分；失败时由 CommonMark 回到原位置继续普通文本解析。
                if (source.isBlank() || (!display && (source.last().isWhitespace() || (dollar && scanner.peek().isDigit())))) {
                    return@InlineContentParser ParsedInline.none()
                }
                return@InlineContentParser ParsedInline.of(ReaderInlineMath(source, display), scanner.position())
            }
            if (!display && (scanner.peek() == '\n' || scanner.peek() == '`')) break
            if (scanner.next('\\') && scanner.hasNext()) scanner.next() else scanner.next()
        }
        ParsedInline.none()
    }
}

internal object ReaderBlockMathFactory : AbstractBlockParserFactory() {
    override fun tryStart(state: ParserState, matchedBlockParser: MatchedBlockParser): BlockStart? {
        if (state.indent >= 4) return BlockStart.none()
        val line = state.line.content.toString().substring(state.nextNonSpaceIndex).trimEnd()
        if (line != "\$\$" && line != "\\[") return BlockStart.none()
        return BlockStart.of(MathBlockParser(line)).atIndex(state.line.content.length)
    }
}

private class MathBlockParser(delimiter: String) : AbstractBlockParser() {
    private val math = ReaderBlockMath(delimiter)
    override fun getBlock() = math
    override fun addLine(line: SourceLine) { math.source.append(line.content).append('\n') }
    override fun tryContinue(state: ParserState): BlockContinue? {
        val closer = if (math.delimiter == "\$\$") "\$\$" else "\\]"
        if (state.line.content.toString().substring(state.nextNonSpaceIndex).trimEnd() == closer) {
            math.closed = true
            return BlockContinue.finished()
        }
        return BlockContinue.atIndex(state.index)
    }
}
