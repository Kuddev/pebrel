package io.github.kuddev.pebrel.mobile.ui

import androidx.annotation.DrawableRes
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import io.github.kuddev.pebrel.mobile.R
import java.util.Locale

@DrawableRes
internal fun fileSymbol(path: String, directory: Boolean, expanded: Boolean = false): Int {
    if (directory) return if (expanded) R.drawable.ic_folder_open else R.drawable.ic_git_folder
    val name = path.substringAfterLast('/').substringAfterLast('\\').lowercase(Locale.ROOT)
    if (name.startsWith(".git")) return R.drawable.ic_file_git
    if (name == "readme") return R.drawable.ic_file_md
    if (name == "dockerfile" || name == "containerfile" || name.startsWith("dockerfile.")) return R.drawable.ic_file_docker
    if (name in setOf("makefile", "justfile", "cmakelists.txt")) return R.drawable.ic_file_code
    return when (name.substringAfterLast('.', "")) {
        "md", "markdown", "mdx" -> R.drawable.ic_file_md
        "rs" -> R.drawable.ic_file_rs
        "py", "pyw", "pyi" -> R.drawable.ic_file_py
        "js", "mjs", "cjs" -> R.drawable.ic_file_js
        "jsx" -> R.drawable.ic_file_jsx
        "ts", "mts", "cts" -> R.drawable.ic_file_ts
        "tsx" -> R.drawable.ic_file_tsx
        "vue" -> R.drawable.ic_file_vue
        "kt", "kts" -> R.drawable.ic_file_kt
        "go" -> R.drawable.ic_file_go
        "swift" -> R.drawable.ic_file_swift
        "php" -> R.drawable.ic_file_php
        "svelte" -> R.drawable.ic_file_svelte
        "c", "h" -> R.drawable.ic_file_c
        "cpp", "cc", "cxx", "hpp", "hxx" -> R.drawable.ic_file_cpp
        "cs" -> R.drawable.ic_file_c_sharp
        "html", "htm" -> R.drawable.ic_file_html
        "css", "scss", "sass", "less" -> R.drawable.ic_file_css
        "json", "jsonc", "json5", "jsonl", "ndjson" -> R.drawable.ic_brackets_curly
        "toml" -> R.drawable.ic_brackets_square
        "yaml", "yml" -> R.drawable.ic_list_dashes
        "xml" -> R.drawable.ic_code
        "ini" -> R.drawable.ic_file_ini
        "env", "conf", "properties" -> R.drawable.ic_sliders_horizontal
        "sql", "db", "sqlite" -> R.drawable.ic_file_sql
        "csv", "tsv" -> R.drawable.ic_file_csv
        "java", "rb", "lua", "dart" -> R.drawable.ic_file_code
        "ps1", "psm1" -> R.drawable.ic_file_powershell
        "sh", "bash", "zsh", "fish", "bat", "cmd" -> R.drawable.ic_terminal_window
        "pem", "key", "pub", "crt", "cer" -> R.drawable.ic_key
        "png" -> R.drawable.ic_file_png
        "jpg", "jpeg" -> R.drawable.ic_file_jpg
        "svg" -> R.drawable.ic_file_svg
        "gif", "webp", "bmp", "ico", "avif", "heic" -> R.drawable.ic_file_image
        "pdf" -> R.drawable.ic_file_pdf
        "zip" -> R.drawable.ic_file_zip
        "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "zst" -> R.drawable.ic_file_archive
        "mp3", "wav", "flac", "ogg", "aac", "m4a" -> R.drawable.ic_file_audio
        "mp4", "mkv", "webm", "mov", "avi" -> R.drawable.ic_file_video
        "doc", "docx", "odt" -> R.drawable.ic_file_doc
        "xls", "xlsx", "ods" -> R.drawable.ic_file_xls
        "ppt", "pptx", "odp" -> R.drawable.ic_file_ppt
        "lock" -> R.drawable.ic_file_lock
        "txt" -> R.drawable.ic_file_txt
        "log", "rst" -> R.drawable.ic_file_text
        else -> R.drawable.ic_git_file
    }
}

@Composable
internal fun FileSymbol(path: String, directory: Boolean = false, modifier: Modifier = Modifier, expanded: Boolean = false) {
    val resource = remember(path, directory, expanded) { fileSymbol(path, directory, expanded) }
    Icon(painterResource(resource), contentDescription = null, modifier = modifier.size(22.dp),
        tint = if (directory) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
}
