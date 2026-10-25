package io.github.kuddev.pebrel.mobile.ui

import android.graphics.Color
import android.net.Uri
import android.view.ViewOutlineProvider
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.viewinterop.AndroidView
import org.json.JSONObject
import java.io.ByteArrayInputStream
import io.github.kuddev.pebrel.mobile.R

private val readerAssetFiles = setOf("reader/reader.css", "reader/highlight.min.js", "reader/reader.js",
    "reader/extensions.js", "reader/reader-compat.js", "reader/katex/katex.min.js", "reader/katex/katex.min.css", "reader/mermaid/mermaid.min.js")
private val readerFontAsset = Regex("reader/katex/fonts/KaTeX_[A-Za-z-]+\\.woff2")

private class ReaderView(context: android.content.Context) : WebView(context) {
    var readerReady = false
    var readerColors: ReaderColors? = null
}

@Composable
internal fun ReaderWebView(document: ReaderDocument, colors: ReaderColors, copied: Int?, copyLabel: String,
                           copiedLabel: String, heading: String?, onCopy: (Int) -> Unit,
                           onLink: (String) -> Unit, modifier: Modifier = Modifier, followUpdates: Boolean = false) {
    val currentCopy = rememberUpdatedState(onCopy)
    val currentLink = rememberUpdatedState(onLink)
    val currentDocument = rememberUpdatedState(document)
    val follow = rememberUpdatedState(followUpdates)
    val textScale = LocalDensity.current.fontScale
    var pageLoading by remember { mutableStateOf(true) }
    Box(modifier) {
        AndroidView(modifier = Modifier.fillMaxSize(), factory = { context ->
            ReaderView(context).apply {
                // AndroidView 默认不裁剪原生绘制；限制 WebView 的合成层，避免覆盖标题栏和输入区。
                outlineProvider = ViewOutlineProvider.BOUNDS
                clipToOutline = true
                settings.javaScriptEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.blockNetworkLoads = true
                settings.domStorageEnabled = false
                settings.setSupportMultipleWindows(false)
                settings.setSupportZoom(true)
                settings.builtInZoomControls = true
                settings.displayZoomControls = false
                webViewClient = object : WebViewClient() {
                    override fun onPageFinished(view: WebView, url: String) {
                        (view as ReaderView).readerReady = true
                        pageLoading = false
                        view.evaluateJavascript("window.pebrelSetReaderLabels(${JSONObject.quote(context.getString(R.string.reader_extension_error))},${JSONObject.quote(context.getString(R.string.reader_extension_loading))});", null)
                        if (follow.value) view.evaluateJavascript("window.pebrelRender(${JSONObject.quote(currentDocument.value.body)},true);", null)
                    }
                    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse {
                        val uri = request.url
                        val asset = uri.path.orEmpty().removePrefix("/")
                        // 文档不拥有网络或本地文件权限；唯一可加载资源是 APK 中固定的阅读器资产。
                        if (uri.scheme == "https" && uri.host == "reader.pebrel.local" &&
                            (asset in readerAssetFiles || readerFontAsset.matches(asset))) {
                            val mime = when { asset.endsWith(".css") -> "text/css"; asset.endsWith(".woff2") -> "font/woff2"; else -> "application/javascript" }
                            return WebResourceResponse(mime, if (asset.endsWith(".woff2")) null else "UTF-8", context.assets.open(asset))
                        }
                        return WebResourceResponse("text/plain", "UTF-8", ByteArrayInputStream(ByteArray(0)))
                    }

                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                        val uri = request.url
                        val parts = uri.schemeSpecificPart.split(':', limit = 2)
                        if (parts.firstOrNull() != currentDocument.value.token) return true
                        val index = parts.getOrNull(1)?.toIntOrNull()
                        when (uri.scheme) {
                            "pebrel-copy" -> index?.takeIf { it in currentDocument.value.code.indices }?.let { currentCopy.value(it) }
                            "pebrel-link" -> index?.let { currentDocument.value.links.getOrNull(it) }?.let { currentLink.value(it) }
                        }
                        return true
                    }
                }
            }
        }, update = { view ->
            view.setBackgroundColor(Color.parseColor(colors.background))
            view.settings.textZoom = (100 * textScale).toInt()
            if (view.tag != document.html) {
                view.tag = document.html
                if (followUpdates && view.readerReady && view.readerColors == colors) {
                    view.evaluateJavascript("window.pebrelRender(${JSONObject.quote(document.body)},false);", null)
                } else {
                    pageLoading = true
                    view.readerReady = false
                    view.readerColors = colors
                    view.loadDataWithBaseURL("https://reader.pebrel.local/", document.html, "text/html", "UTF-8", null)
                }
            }
            val label = JSONObject.quote(copyLabel)
            val success = JSONObject.quote(copiedLabel)
            view.evaluateJavascript("document.querySelectorAll('[data-copy]').forEach(function(a){a.textContent=Number(a.dataset.copy)===${copied ?: -1}?$success:$label;});", null)
            if (heading != null) {
                val id = JSONObject.quote(Uri.decode(heading.removePrefix("#")))
                view.evaluateJavascript("(function(){var h=document.getElementById($id);if(h)h.scrollIntoView();})();", null)
            }
        }, onRelease = { view ->
            view.stopLoading()
            view.destroy()
        })
        // 页面完成前保留就地进度，避免文件已到达但 WebView 仍空白时没有反馈。
        if (pageLoading) LinearProgressIndicator(Modifier.fillMaxWidth().align(Alignment.TopCenter))
    }
}
