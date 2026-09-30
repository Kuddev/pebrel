package io.github.kuddev.pebrel.mobile.ui

import org.commonmark.Extension
import org.commonmark.ext.gfm.strikethrough.StrikethroughExtension
import org.commonmark.ext.gfm.tables.TablesExtension
import org.commonmark.ext.task.list.items.TaskListItemsExtension
import org.commonmark.node.*
import org.commonmark.parser.Parser
import org.commonmark.renderer.html.CoreHtmlNodeRenderer
import org.commonmark.renderer.html.HtmlRenderer

internal data class ReaderHeading(val id: String, val text: String, val level: Int)
internal data class ReaderDocument(val html: String, val code: List<String>, val links: List<String>, val headings: List<ReaderHeading>,
                                   val body: String, val token: String)

/** CommonMark owns parsing; both file and conversation views share these local actions. */
internal fun prepareReader(source: String, path: String, preview: Boolean, colors: ReaderColors,
                           copyLabel: String, imageLabel: String): ReaderDocument {
    val markup = ReaderMarkup(copyLabel, imageLabel)
    val body = if (preview) markup.markdown(source) else {
        val lineNumbers = (1..(source.count { it == '\n' } + 1)).joinToString("\n")
        """<div class="source-layout"><pre class="line-numbers" aria-hidden="true">$lineNumbers</pre><pre><code class="language-${readerLanguage(path)}">${readerEscape(source)}</code></pre></div>"""
    }
    return markup.document(body, colors)
}

internal class ReaderMarkup(private val copyLabel: String, private val imageLabel: String) {
    private val code = mutableListOf<String>()
    private val links = mutableListOf<String>()
    private val headings = mutableListOf<ReaderHeading>()
    private val extensions: List<Extension> = listOf(TablesExtension.create(), StrikethroughExtension.create(), TaskListItemsExtension.create())

    fun markdown(source: String): String {
        val tree = Parser.builder().extensions(extensions)
            .customInlineContentParserFactory(ReaderInlineMathFactory)
            .customBlockParserFactory(ReaderBlockMathFactory).build().parse(source)
        return HtmlRenderer.builder().extensions(extensions).escapeHtml(true).sanitizeUrls(true)
            .nodeRendererFactory { context -> object : org.commonmark.renderer.NodeRenderer {
                override fun getNodeTypes(): Set<Class<out Node>> = setOf(ReaderInlineMath::class.java, ReaderBlockMath::class.java)
                override fun render(node: Node) {
                    when (node) {
                        is ReaderInlineMath -> context.writer.raw(math(node.source, node.display))
                        is ReaderBlockMath -> if (node.closed) context.writer.raw(math(node.source.toString(), true))
                            else context.writer.text(node.delimiter + node.source)
                    }
                }
            } }
            .nodeRendererFactory { context -> object : CoreHtmlNodeRenderer(context) {
                override fun visit(block: FencedCodeBlock) { context.writer.raw(codeBlock(block.literal, block.info.substringBefore(' '))) }
                override fun visit(block: IndentedCodeBlock) { context.writer.raw(codeBlock(block.literal, "plaintext")) }

                override fun visit(link: Link) {
                    val index = links.size
                    links += link.destination
                    context.writer.tag("a", mapOf("href" to "pebrel-link:$index"))
                    visitChildren(link)
                    context.writer.tag("/a")
                }

                override fun visit(image: Image) {
                    val index = links.size
                    links += image.destination
                    context.writer.tag("a", mapOf("class" to "image-link", "href" to "pebrel-link:$index"))
                    context.writer.text("$imageLabel · ${nodeText(image).ifBlank { image.destination }}")
                    context.writer.tag("/a")
                }

                override fun visit(heading: Heading) {
                    val text = nodeText(heading)
                    val slug = text.lowercase().trim().replace(Regex("[^\\p{L}\\p{N}_ -]"), "").replace(' ', '-')
                    val base = slug.ifBlank { "heading" }
                    var id = base
                    var suffix = 1
                    while (headings.any { it.id == id }) { id = "$base-${suffix++}" }
                    headings += ReaderHeading(id, text, heading.level)
                    context.writer.tag("h${heading.level}", mapOf("id" to id))
                    visitChildren(heading)
                    context.writer.tag("/h${heading.level}")
                }
            } }.build().render(tree)
    }

    fun codeBlock(literal: String, language: String = "plaintext"): String {
        if (language.lowercase() in setOf("math", "latex", "tex")) return math(literal, true)
        val index = code.size
        code += literal
        val safeLanguage = language.takeIf { it.matches(Regex("[a-zA-Z0-9_+-]{1,40}")) } ?: "plaintext"
        if (language.equals("mermaid", true)) {
            return """<figure class="diagram" id="diagram-$index"><div class="diagram-render" data-diagram="$index"></div><details><summary>Mermaid</summary><div class="code-block"><a class="copy-code" data-copy="$index" href="pebrel-copy:$index">${readerEscape(copyLabel)}</a><pre><code class="language-mermaid">${readerEscape(literal)}</code></pre></div></details></figure>"""
        }
        return """<figure class="code-block"><a class="copy-code" data-copy="$index" href="pebrel-copy:$index">${readerEscape(copyLabel)}</a><pre><code class="language-$safeLanguage">${readerEscape(literal)}</code></pre><figcaption>${readerEscape(language)}</figcaption></figure>"""
    }

    private fun math(source: String, display: Boolean): String {
        val index = code.size
        code += source
        return """<span class="formula${if (display) " formula-display" else ""}" id="formula-$index" tabindex="0" aria-label="LaTeX"><span class="math-render" data-display="$display">${readerEscape(source)}</span><span class="formula-actions"><a data-copy="$index" href="pebrel-copy:$index">${readerEscape(copyLabel)} LaTeX</a></span></span>"""
    }

    fun document(body: String, colors: ReaderColors): ReaderDocument {
        val token = body.hashCode().toUInt().toString(16)
        // 自动刷新会重新编号链接；旧页面的迟到点击只属于它自己的文档。
        val guarded = body.replace("href=\"pebrel-copy:", "href=\"pebrel-copy:$token:")
            .replace("href=\"pebrel-link:", "href=\"pebrel-link:$token:")
        val html = """<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="referrer" content="no-referrer"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline' https://reader.pebrel.local; script-src https://reader.pebrel.local; font-src https://reader.pebrel.local; img-src data:"><link rel="stylesheet" href="https://reader.pebrel.local/reader/katex/katex.min.css"><link rel="stylesheet" href="https://reader.pebrel.local/reader/reader.css"><style>:root{--background:${colors.background};--foreground:${colors.foreground};--muted:${colors.muted};--surface:${colors.surface};--code-surface:${colors.codeSurface};--border:${colors.border};--accent:${colors.accent}}</style></head><body><main>$guarded</main><script src="https://reader.pebrel.local/reader/highlight.min.js"></script><script src="https://reader.pebrel.local/reader/katex/katex.min.js"></script><script src="https://reader.pebrel.local/reader/reader-compat.js"></script><script src="https://reader.pebrel.local/reader/mermaid/mermaid.min.js"></script><script src="https://reader.pebrel.local/reader/extensions.js"></script><script src="https://reader.pebrel.local/reader/reader.js"></script></body></html>"""
        return ReaderDocument(html, code, links, headings, guarded, token)
    }
}

private fun nodeText(node: Node): String = buildString {
    node.accept(object : AbstractVisitor() {
        override fun visit(text: Text) { append(text.literal) }
        override fun visit(code: Code) { append(code.literal) }
        override fun visit(line: SoftLineBreak) { append(' ') }
    })
}

internal fun readerLanguage(path: String): String = when (path.substringAfterLast('.').lowercase()) {
    "rs" -> "rust"
    "kt", "kts" -> "kotlin"
    "js", "jsx", "mjs", "cjs" -> "javascript"
    "ts", "tsx" -> "typescript"
    "py" -> "python"
    "md", "markdown" -> "markdown"
    "c", "h", "cpp", "hpp", "cc" -> "cpp"
    "cs" -> "csharp"
    "sh", "bash", "zsh" -> "bash"
    "ps1" -> "powershell"
    "yml", "yaml" -> "yaml"
    "html", "xml", "svg" -> "xml"
    "json", "css", "go", "java", "sql", "swift", "ruby", "toml", "ini", "diff" -> path.substringAfterLast('.').lowercase()
    else -> "plaintext"
}

internal fun readerEscape(text: String): String = text.replace("&", "&amp;").replace("<", "&lt;")
    .replace(">", "&gt;").replace("\"", "&quot;").replace("'", "&#39;")
