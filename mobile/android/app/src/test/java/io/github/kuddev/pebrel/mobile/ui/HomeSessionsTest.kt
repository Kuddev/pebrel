package io.github.kuddev.pebrel.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Density
import io.github.kuddev.pebrel.mobile.R
import io.github.kuddev.pebrel.mobile.session.DisplayPreferences
import io.github.kuddev.pebrel.mobile.connection.*
import io.github.kuddev.pebrel.mobile.session.DesktopWorkspace
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class HomeSessionsTest {
    @Test fun terminalHeaderActionsStayCompactAndKeepSeparateTouchTargets() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val opened = mutableListOf<String>()
        val gitEnabled = mutableStateOf(true)
        compose.setContent { MaterialTheme {
            CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, fontScale = 2f)) {
                Box(Modifier.size(320.dp, 48.dp)) {
                    TerminalHeader("long-workspace-title-for-compact-header", "Computer", "ready",
                        onBack = { opened += "back" }, onSessions = { opened += "sessions" },
                        onGit = { opened += "git" }, gitEnabled = gitEnabled.value,
                        onConversation = { opened += "chat" }, onDetails = { opened += "details" })
                }
            }
        } }
        val git = compose.onNodeWithContentDescription(context.getString(R.string.git_title))
        val chat = compose.onNodeWithContentDescription(context.getString(R.string.chat_title))
        val more = compose.onNodeWithContentDescription(context.getString(R.string.more_actions))
        val actions = listOf(git, chat, more)
        actions.forEach { it.assertIsDisplayed().assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(48.dp) }
        actions.zipWithNext().forEach { (left, right) ->
            val leftBounds = left.getUnclippedBoundsInRoot()
            val rightBounds = right.getUnclippedBoundsInRoot()
            assertEquals(leftBounds.right, rightBounds.left)
            assertEquals(36.dp, rightBounds.left - leftBounds.left)
        }
        val back = compose.onNodeWithContentDescription(context.getString(R.string.back))
        back.assertWidthIsEqualTo(48.dp).assertHeightIsEqualTo(48.dp)
        // 点两侧留白而非仅图形中心，防止紧凑布局下相邻按钮抢走触摸。
        listOf(git, chat).forEach { node ->
            node.performTouchInput {
                click(Offset(1f, center.y))
                click(Offset(width - 1f, center.y))
            }
        }
        more.performTouchInput { click(center) }
        compose.onNodeWithText(context.getString(R.string.terminal_details)).performClick()
        back.performTouchInput { click(center) }
        compose.runOnIdle {
            assertEquals(listOf("git", "git", "chat", "chat", "details", "back"), opened)
            gitEnabled.value = false
        }
        git.assertIsNotEnabled().performTouchInput { click(center) }
        chat.assertIsEnabled().performTouchInput { click(center) }
        compose.runOnIdle { assertEquals(listOf("git", "git", "chat", "chat", "details", "back", "chat"), opened) }
    }

    @Test fun sshHeaderUsesTheSameCompactGroupWithoutEnablingDisconnectedFiles() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val opened = mutableListOf<String>()
        val status = mutableStateOf("ready")
        compose.setContent { MaterialTheme {
            Box(Modifier.size(320.dp, 48.dp)) {
                TerminalHeader("SSH", "Computer", status.value, {}, {},
                    onFiles = { opened += "files" }, onClose = { opened += "close" })
            }
        } }
        val files = compose.onNodeWithContentDescription(context.getString(R.string.sftp_title))
        val more = compose.onNodeWithContentDescription(context.getString(R.string.more_actions))
        files.assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(48.dp).performTouchInput { click(center) }
        more.assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(48.dp).performTouchInput { click(center) }
        assertEquals(files.getUnclippedBoundsInRoot().right, more.getUnclippedBoundsInRoot().left)
        compose.onNodeWithText(context.getString(R.string.close_session)).performClick()
        compose.runOnIdle { status.value = "failed" }
        files.assertIsNotEnabled().performTouchInput { click(center) }
        compose.runOnIdle { assertEquals(listOf("files", "close"), opened) }
    }

    @Test fun firstSnapshotRebindsTheTabPageBeforeItsFirstMutation() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val methods = java.util.concurrent.CopyOnWriteArrayList<String>()
        val receiver = java.util.concurrent.atomic.AtomicReference<((org.json.JSONObject) -> Unit)?>()
        fun snapshot(added: Boolean) = org.json.JSONObject("""{
            "process_id":11,"revision":${if (added) 2 else 1},"mobile_policy":{"allow_input":true},
            "windows":[{"id":7,"tabs":[{"tab_id":"0123456789abcdef0123456789abcdef","index":0,
            "label":"work","kind":"shell","active":true,"panes":[{"id":${if (added) 3 else 2},"title":"work","task_state":"idle"}]}]}]
        }""")
        val transport = object : DesktopTransport {
            override suspend fun open(allowInput: Boolean, receive: (org.json.JSONObject) -> Unit, disconnected: (Throwable?) -> Unit) {
                receiver.set(receive)
                receive(org.json.JSONObject("""{"type":"mobile.ready","protocol":"pebrel.mobile.relay","version":1,"capabilities":{"input":true}}"""))
            }
            override fun send(frame: org.json.JSONObject) {
                val method = frame.getString("method")
                methods += method
                val result = if (method == "tab.new") org.json.JSONObject()
                    .put("action", org.json.JSONObject().put("window_id", 7).put("pane_id", 3))
                    .put("snapshot", snapshot(true)) else org.json.JSONObject()
                checkNotNull(receiver.get()).invoke(org.json.JSONObject().put("id", frame.getString("id")).put("ok", true).put("result", result))
            }
            override fun close() = Unit
        }
        val repository = io.github.kuddev.pebrel.mobile.session.SessionRepository(context) { transport }
        val id = repository.connectRelay(RelayProfile("wss://initial.example", "initial-tab", "a".repeat(43), "Computer"))
        var opened: DesktopTab? = null
        try {
            compose.setContent { MaterialTheme {
                val computers by repository.desktops.collectAsState()
                DesktopTabsPage(computers.find { it.id == id }, repository, {}, { opened = it }, {}, null, {})
            } }
            compose.waitUntil(5000) { methods.contains("events.subscribe") }
            compose.runOnIdle { assertNull(repository.desktops.value.single().runtimeProcess) }
            checkNotNull(receiver.get()).invoke(org.json.JSONObject().put("event", "runtime.snapshot").put("data", snapshot(false)))
            compose.waitUntil(5000) {
                org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
                repository.desktops.value.single().status == "ready" && repository.desktops.value.single().allowInput
            }
            compose.onNodeWithContentDescription(context.getString(R.string.tab_new)).performClick()
            compose.onNodeWithText(context.getString(R.string.tab_new_terminal)).performClick()
            compose.onNodeWithText(context.getString(R.string.tab_open)).performClick()
            compose.waitUntil(5000) { opened != null }
            compose.runOnIdle {
                assertEquals(1, methods.count { it == "tab.new" })
                assertEquals(3L, opened?.primaryPane?.id)
            }
        } finally { repository.closeAll() }
    }

    @Test fun fileTabsWithoutPanesRemainVisibleAndHaveSeparateCloseTargets() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val tabs = parseDesktopTabs(org.json.JSONObject("""{"windows":[{"id":7,"tabs":[
            {"index":0,"tab_id":"0123456789abcdef0123456789abcdef","label":"C:/docs/README.md","kind":"document","active":true,"panes":[],"file":{"path":"C:/docs/README.md","ready":true,"revision":3}},
            {"index":1,"tab_id":"1123456789abcdef0123456789abcdef","label":"preview.png","kind":"image","panes":[],"file":{"path":"C:/docs/preview.png","ready":true}}
        ]}]}"""))
        val computer = DesktopWorkspace("files", HostProfile("pc", "Computer", "localhost", 22, "user"),
            status = "ready", allowInput = true, hasConnected = true, tabs = tabs)
        var opened: String? = null
        var closed: String? = null
        assertEquals(2, sessionCards(emptyList(), listOf(computer)).size)
        compose.setContent { MaterialTheme {
            Column { DesktopTabRows(computer, { opened = it.id }, {}, { closed = it.id }) }
        } }
        compose.onNodeWithText("README.md").performClick()
        compose.runOnIdle { assertEquals(tabs[0].id, opened); assertNull(closed) }
        compose.onNodeWithContentDescription(context.getString(R.string.tab_close_named, "preview.png"))
            .assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp).performClick()
        compose.runOnIdle { assertEquals(tabs[1].id, closed); assertEquals(tabs[0].id, opened) }
    }

    @Test fun readerPreservesMarkdownFeaturesButNeverExecutesDocumentHtml() {
        val colors = ReaderColors("#ffffff", "#111111", "#555555", "#eeeeee", "#aaaaaa", "#007777")
        val source = "# 标题\n\n- [x] 已完成\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```kotlin\nprintln(\"中文😀\")\n```\n\n<script>malicious()</script>\n\n![预览](preview.png)"
        val document = prepareReader(source, "README.md", true, colors, "复制", "图片")
        assertTrue(document.html.contains("<table>"))
        assertTrue(document.html.contains("checkbox"))
        assertFalse(document.html.contains("<script>malicious()"))
        assertTrue(document.html.contains("&lt;script&gt;"))
        assertEquals(listOf("println(\"中文😀\")\n"), document.code)
        assertEquals(listOf("preview.png"), document.links)
        assertEquals("标题", document.headings.single().text)
    }

    @Test fun readerCodeBlocksKeepTheirOwnSurfaceWhenTheDocumentContainerMatchesTheBackground() {
        val background = androidx.compose.ui.graphics.Color(0xff2e3440)
        val codeSurface = androidx.compose.ui.graphics.Color(0xff3b4252)
        val colors = androidx.compose.material3.darkColorScheme(background = background,
            surfaceContainer = background, surfaceContainerHighest = codeSurface).readerColors()
        val doc = prepareReader("```python\nprint(1)\n```", "README.md", true, colors, "Copy", "Image")
        assertEquals(colors.background, colors.surface)
        assertNotEquals(colors.background, colors.codeSurface)
        assertTrue(doc.html.contains("--code-surface:#3b4252"))
    }

    @Test fun readerMathPreservesLatexBeforeMarkdownAndLeavesCurrencyAndCodeAlone() {
        val colors = ReaderColors("#ffffff", "#111111", "#555555", "#eeeeee", "#aaaaaa", "#007777")
        val source = "Inline \$x_1 + \\frac{a}{b}\$ and \\(z^2\\). Price \$5 and \$10. `\$not_math\$`\n\n\$\$\n\\begin{aligned}\nx &= y \\\\\n\ny &= z\n\\end{aligned}\n\$\$\n"
        val doc = prepareReader(source, "README.md", true, colors, "Copy", "Image")
        assertEquals(3, doc.code.size)
        assertEquals("x_1 + \\frac{a}{b}", doc.code[0])
        assertEquals("z^2", doc.code[1])
        assertTrue(doc.code[2].contains("\\begin{aligned}\nx &= y"))
        assertTrue(doc.html.contains("Price \$5 and \$10"))
        assertTrue(doc.html.contains("<code>\$not_math\$</code>"))
        assertFalse(doc.html.contains("<em>1"))
        assertTrue(doc.html.contains("reader/katex/katex.min.js"))
    }

    @Test fun readerMermaidKeepsItsExactSourceAndDoesNotGrantRemoteScripts() {
        val colors = ReaderColors("#ffffff", "#111111", "#555555", "#eeeeee", "#aaaaaa", "#007777")
        val code = "flowchart LR\n A[开始] --> B[完成]\n"
        val doc = prepareReader("```mermaid\n${code}```\n", "README.md", true, colors, "Copy", "Image")
        assertEquals(listOf(code), doc.code)
        assertTrue(doc.html.contains("class=\"diagram-render\""))
        assertTrue(doc.html.contains("pebrel-copy:${doc.token}:0"))
        assertTrue(doc.html.contains("script-src https://reader.pebrel.local"))
        assertFalse(doc.html.contains("script-src 'unsafe-inline'"))
    }

    @Test fun fileReaderAssemblesBoundedChunksAndDoesNotContinueAfterReconnect() = kotlinx.coroutines.runBlocking {
        val tab = DesktopTab(7, "0123456789abcdef0123456789abcdef", 0, "README.md", "document", true,
            null, emptyList(), DesktopFile("C:/docs/README.md", false, false, false, true, 1))
        val parts = listOf("中文".toByteArray(Charsets.UTF_8), "😀".toByteArray(Charsets.UTF_8))
        var index = 0
        var stale = false
        val client = DesktopTabs(request = { method, params ->
            assertEquals("tab.read", method)
            assertEquals(tab.id, params.getString("tab_id"))
            val offset = params.getInt("offset")
            if (index > 0) assertEquals("text:1", params.getString("revision"))
            val chunk = parts[index++]
            org.json.JSONObject().put("tab_id", tab.id).put("window_id", 7).put("kind", "text").put("revision", "text:1")
                .put("offset", offset).put("next_offset", offset + chunk.size).put("total_bytes", 10)
                .put("eof", index == 2).put("data", android.util.Base64.encodeToString(chunk, android.util.Base64.NO_WRAP))
        }, availability = { _, _ -> if (stale) "desktop_session_changed" else null })
        assertEquals("中文😀", client.read(tab).bytes.toString(Charsets.UTF_8))
        stale = true
        try { client.read(tab); fail("stale reader must not dispatch") }
        catch (error: DesktopRpcFailure) { assertEquals("desktop_session_changed", error.code) }
        assertEquals(2, index)
    }

    @Test fun markdownRelativeFilesResolveOnTheComputerNotOnAndroid() {
        val file = DesktopFile("D:\\workspace\\docs\\README.md", false, false, false, true, 1)
        assertEquals("D:/workspace/image.png", resolveReaderLink(file, "../image.png"))
        assertEquals("D:/workspace/docs/中文.png", resolveReaderLink(file, "%E4%B8%AD%E6%96%87.png"))
        assertNull(resolveReaderLink(file, "javascript:alert(1)"))
        assertNull(resolveReaderLink(file.copy(remote = true), "image.png"))
    }
    @get:Rule val compose = createComposeRule()
    private val pane = DesktopPane(1, 2, "PC shell", "/project", "pwsh", "idle", 1)
    private val desktop = DesktopWorkspace("pc", HostProfile("host", "My computer", "192.0.2.1", 22, "root"),
        panes = listOf(pane), status = "ready", transport = "Relay", hasConnected = true)

    @Test fun tailnetHostCanSaveAndConnectWithoutAPassword() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val host = HostProfile("tailnet", "Tailnet host", "host.example.ts.net", 22, "alice")
        var submitted = false
        compose.setContent { MaterialTheme {
            HostForm(host, {}, false, false, {}) { saved, password, _, connect ->
                assertEquals(host.address, saved.address)
                assertNull(password)
                submitted = connect
            }
        } }
        compose.onNodeWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.save_connect))
            .performScrollTo().assertIsEnabled().performClick()
        compose.runOnIdle { assertTrue(submitted) }
    }

    @Test fun passwordlessLoginRemainsAnExplicitUserAction() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        var submitted = false
        compose.setContent { MaterialTheme {
            LoginForm(HostProfile("tailnet", "Tailnet host", "100.64.1.2", 22, "alice"), {}, false, false, {}) {
                    password, computer, _, _ ->
                assertNull(password)
                assertFalse(computer)
                submitted = true
            }
        } }
        compose.runOnIdle { assertFalse(submitted) }
        compose.onAllNodesWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.connect))
            .filterToOne(hasClickAction()).performScrollTo().assertIsEnabled().performClick()
        compose.runOnIdle { assertTrue(submitted) }
    }

    @Test fun passwordlessLoginStillDisablesConnectWhileBusy() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        compose.setContent { MaterialTheme {
            LoginForm(HostProfile("tailnet", "Tailnet host", "100.64.1.2", 22, "alice"), {}, false, true, {}) {
                    _, _, _, _ -> fail("Busy login submitted")
            }
        } }
        compose.onAllNodesWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.connect))
            .filterToOne(hasClickAction()).assertIsNotEnabled()
    }

    @Test fun connectionPasswordAllowsNoneWithoutDiscardingEnteredSecrets() = kotlinx.coroutines.runBlocking {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val repository = io.github.kuddev.pebrel.mobile.session.SessionRepository(context)
        val host = HostProfile("passwordless-fixture", "Tailnet host", "100.64.1.2", 22, "alice")
        assertArrayEquals(charArrayOf(), repository.passwordForConnection(host, null))
        val entered = charArrayOf('s', 'e', 'c', 'r', 'e', 't')
        val copied = checkNotNull(repository.passwordForConnection(host, entered))
        assertNotSame(entered, copied)
        assertArrayEquals(entered, copied)
        copied.fill('\u0000')
        assertEquals('s', entered[0])
        entered.fill('\u0000')
    }

    @Test fun connectedPcAppearsInSessionGalleryAndOpensExactPane() {
        var opened = ""
        compose.setContent { MaterialTheme { HomeScreen(
            emptyList(), emptyList(), listOf(desktop), emptyList(), {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {},
            onPane = { computer, target -> opened = "$computer:${target.window}:${target.id}" }) } }
        compose.onNodeWithText("PC shell").assertIsDisplayed().performClick()
        compose.runOnIdle { assertEquals("pc:1:2", opened) }
    }

    @Test fun plusOpensFourActionsAndDismissesBeforeRouting() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val opened = mutableListOf<String>()
        compose.setContent { MaterialTheme { HomeScreen(
            sessions = emptyList(), hosts = listOf(desktop.host), desktops = emptyList(), relays = emptyList(),
            onSession = {}, onSessions = {}, onHosts = {}, onLogin = {}, onEditHost = {}, onDeleteHost = {},
            onAddHost = { opened += "ssh" }, onDesktop = {}, onRelay = {}, onComputers = {},
            onAddRelay = { opened += "computer" }, onLocal = { opened += "local" }, onDeployRelay = { opened += "relay" },
        ) } }
        val actions = listOf(
            io.github.kuddev.pebrel.mobile.R.string.computer_connect to "computer",
            io.github.kuddev.pebrel.mobile.R.string.add_ssh to "ssh",
            io.github.kuddev.pebrel.mobile.R.string.local_terminal to "local",
            io.github.kuddev.pebrel.mobile.R.string.new_deploy_relay to "relay",
        )
        actions.forEachIndexed { index, (title, action) ->
            compose.onNodeWithContentDescription(context.getString(io.github.kuddev.pebrel.mobile.R.string.new_connection)).performClick()
            compose.onNodeWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.new_computer_hint)).assertIsDisplayed()
            compose.runOnIdle { assertEquals(index, opened.size) }
            compose.onNode(hasText(context.getString(title)) and
                SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button) and hasAnyAncestor(isDialog()))
                .assertHeightIsAtLeast(48.dp)
                // Robolectric 对独立 Dialog 的触摸注入不稳定；坐标点击另在 APK 上验证。
                .performSemanticsAction(SemanticsActions.OnClick) { it() }
            compose.waitUntil(timeoutMillis = 5_000) { opened.size == index + 1 }
            compose.runOnIdle { assertEquals(action, opened.last()) }
            compose.onNodeWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.new_computer_hint)).assertDoesNotExist()
        }
        compose.runOnIdle { assertEquals(listOf("computer", "ssh", "local", "relay"), opened) }
    }

    @Test fun emptyHomeHasThreeWorkingActionsWithoutEmptyGroupsOrDuplicateControls() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val opened = mutableListOf<String>()
        val empty = isHomeEmpty(emptyList(), emptyList(), emptyList(), emptyList())
        assertTrue(empty)
        compose.setContent { MaterialTheme { Column {
            HomeHeader({}, {}, showNotices = !empty)
            HomeScreen(
                sessions = emptyList(), hosts = emptyList(), desktops = emptyList(), relays = emptyList(),
                onSession = {}, onSessions = {}, onHosts = {}, onLogin = {}, onEditHost = {}, onDeleteHost = {},
                onAddHost = { opened += "ssh" }, onDesktop = {}, onRelay = {}, onComputers = {},
                onAddRelay = { opened += "computer" }, onLocal = { opened += "local" }, onDeployRelay = {},
            )
        } } }
        compose.onNodeWithText(context.getString(R.string.home_empty_title)).assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.home_empty_hint)).assertIsDisplayed()
        compose.onNodeWithContentDescription(context.getString(R.string.settings)).assertIsDisplayed()
        compose.onNodeWithContentDescription(context.getString(R.string.notifications)).assertDoesNotExist()
        compose.onNodeWithContentDescription(context.getString(R.string.new_connection)).assertDoesNotExist()
        listOf(R.string.sessions, R.string.ssh_hosts, R.string.computers, R.string.no_hosts, R.string.no_computers).forEach {
            compose.onNodeWithText(context.getString(it)).assertDoesNotExist()
        }
        listOf(R.string.computer_connect, R.string.add_ssh, R.string.home_open_local).forEach {
            compose.onNodeWithText(context.getString(it)).performScrollTo().assertHeightIsAtLeast(48.dp).performClick()
        }
        compose.runOnIdle { assertEquals(listOf("computer", "ssh", "local"), opened) }
    }

    @Test fun savedHostWithoutSessionsKeepsItsEntryAndTheNewMenu() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        var opened = ""
        assertFalse(isHomeEmpty(emptyList(), listOf(desktop.host), emptyList(), emptyList()))
        compose.setContent { MaterialTheme { HomeScreen(
            sessions = emptyList(), hosts = listOf(desktop.host), desktops = emptyList(), relays = emptyList(),
            onSession = {}, onSessions = {}, onHosts = {}, onLogin = { opened = it.id }, onEditHost = {}, onDeleteHost = {},
            onAddHost = {}, onDesktop = {}, onRelay = {}, onComputers = {}, onAddRelay = {}, onLocal = {}, onDeployRelay = {},
        ) } }
        compose.onNodeWithText(context.getString(R.string.home_empty_title)).assertDoesNotExist()
        compose.onNodeWithContentDescription(context.getString(R.string.new_connection)).assertIsDisplayed()
        compose.onNodeWithText(desktop.host.name).performScrollTo().assertIsDisplayed().performClick()
        compose.runOnIdle { assertEquals(desktop.host.id, opened) }
    }

    @Test fun savedOfflineComputerWithoutSessionsKeepsItsReconnectEntry() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val profile = RelayProfile("wss://relay.example.com", "computer", "token", "Saved computer")
        var opened: RelayProfile? = null
        assertFalse(isHomeEmpty(emptyList(), emptyList(), emptyList(), listOf(profile)))
        compose.setContent { MaterialTheme { HomeScreen(
            sessions = emptyList(), hosts = emptyList(), desktops = emptyList(), relays = listOf(profile),
            onSession = {}, onSessions = {}, onHosts = {}, onLogin = {}, onEditHost = {}, onDeleteHost = {},
            onAddHost = {}, onDesktop = {}, onRelay = { opened = it }, onComputers = {}, onAddRelay = {}, onLocal = {}, onDeployRelay = {},
        ) } }
        compose.onNodeWithText(context.getString(R.string.home_empty_title)).assertDoesNotExist()
        compose.onNodeWithText(profile.name).performScrollTo().assertIsDisplayed().performClick()
        compose.runOnIdle { assertSame(profile, opened) }
    }

    // 行高依赖真实字体度量；legacy 图形模式的占位字形不代表设备布局。
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Test fun computersAndConnectActionShareRowsAndTextAlignmentIncludingWhitespaceClicks() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val saved = RelayProfile("wss://example.com", "saved", "token", "Saved computer")
        val opened = mutableListOf<String>()
        compose.setContent { MaterialTheme { Box(Modifier.size(360.dp, 400.dp)) {
            ComputerRows(listOf(desktop), listOf(saved), { opened += it }, { opened += it.name }, { opened += "add" })
        } } }
        val titles = listOf(desktop.host.name, saved.name, context.getString(R.string.relay_connect))
        val textLeft = titles.map { compose.onNodeWithText(it, useUnmergedTree = true).getUnclippedBoundsInRoot().left }
        assertEquals(1, textLeft.distinct().size)
        titles.forEach { title ->
            compose.onNode(hasText(title) and hasClickAction()).assertHeightIsEqualTo(72.dp)
                .performTouchInput { click(androidx.compose.ui.geometry.Offset(4f, center.y)) }
        }
        compose.runOnIdle { assertEquals(listOf(desktop.id, saved.name, "add"), opened) }
    }

    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Test fun sameNamedComputersExposeTheirAddressesWithoutMergingIdentities() {
        val old = RelayProfile("wss://192.168.1.137:49183", "old", "token", "Pebrel", mode = "lan")
        val current = RelayProfile("wss://192.168.0.104:56188", "current", "token", "Pebrel", mode = "lan")
        var opened: RelayProfile? = null
        compose.setContent { MaterialTheme { Box(Modifier.size(360.dp, 300.dp)) {
            ComputerRows(emptyList(), listOf(old, current), {}, { opened = it }, {})
        } } }
        compose.onAllNodesWithText("Pebrel").assertCountEquals(2)
        compose.onNodeWithText("192.168.1.137:49183", substring = true).assertIsDisplayed()
        compose.onNodeWithText("192.168.0.104:56188", substring = true).assertIsDisplayed().performClick()
        compose.runOnIdle { assertSame(current, opened) }
    }

    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Test fun groupedComputersGrowForLargeTextAndKeepAddAndForgetReachable() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val saved = RelayProfile("wss://example.com", "saved", "token", "Saved computer with a long name")
        var added = false
        var forgotten = false
        compose.setContent { MaterialTheme {
            CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, fontScale = 2f)) {
                Column(Modifier.size(320.dp, 480.dp).verticalScroll(rememberScrollState())) {
                    ComputerRows(emptyList(), listOf(saved), {}, {}, { added = true }, { forgotten = true })
                }
            }
        } }
        val row = compose.onNode(hasText(saved.name) and hasClickAction()).assertHeightIsAtLeast(72.dp).getUnclippedBoundsInRoot()
        val add = compose.onNodeWithText(context.getString(R.string.relay_connect)).performScrollTo()
            .assertHeightIsAtLeast(72.dp).getUnclippedBoundsInRoot()
        assertTrue(row.bottom <= add.top)
        compose.onNodeWithText(context.getString(R.string.relay_connect)).performClick()
        compose.runOnIdle { assertTrue(added); assertFalse(forgotten) }
        compose.onNodeWithContentDescription(context.getString(R.string.more_actions)).performScrollTo().performClick()
        compose.onNodeWithText(context.getString(R.string.computer_remove_title)).performClick()
        compose.onNodeWithText(context.getString(R.string.computer_remove_hint, saved.name)).assertExists()
        compose.runOnIdle { assertFalse(forgotten) }
        compose.onNodeWithText(context.getString(R.string.cancel)).performClick()
        compose.runOnIdle { assertFalse(forgotten) }
    }

    @Test fun emptyHomeActionsRemainReachableOnShortNarrowScreensWithLargeText() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val opened = mutableListOf<String>()
        compose.setContent { MaterialTheme {
            CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, fontScale = 2f)) {
                Box(Modifier.size(320.dp, 320.dp)) {
                    EmptyHomeScreen({ opened += "computer" }, { opened += "ssh" }, { opened += "local" })
                }
            }
        } }
        listOf(R.string.computer_connect, R.string.add_ssh, R.string.home_open_local).forEach {
            compose.onNodeWithText(context.getString(it)).performScrollTo().assertIsDisplayed()
                .assertHeightIsAtLeast(48.dp).performClick()
        }
        compose.runOnIdle { assertEquals(listOf("computer", "ssh", "local"), opened) }
    }

    @Test fun desktopCardReservesHeightForLargeMetadataWithoutOverlappingItsTitle() {
        val cwd = "/workspace/project/src"
        compose.setContent { MaterialTheme {
            CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, fontScale = 2f)) {
                Box(Modifier.size(320.dp, 480.dp)) {
                    HomeScreen(
                        emptyList(), emptyList(), listOf(desktop.copy(panes = listOf(pane.copy(cwd = cwd)))), emptyList(),
                        {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {},
                    )
                }
            }
        } }
        compose.onNodeWithText(pane.title).assertHeightIsAtLeast(360.dp)
        val path = compose.onNodeWithText(cwd, useUnmergedTree = true).assertIsDisplayed().getUnclippedBoundsInRoot()
        val title = compose.onNodeWithText(pane.title, useUnmergedTree = true).assertIsDisplayed().getUnclippedBoundsInRoot()
        assertTrue(path.bottom <= title.top)
    }

    @Test fun disconnectedPcRemainsButFailedFirstAttemptDoesNotBecomeASession() {
        val cards = sessionCards(emptyList(), listOf(desktop.copy(status = "disconnected"),
            desktop.copy(id = "failed", status = "failed", hasConnected = false)))
        assertEquals(listOf("pc:pc:1:2"), cards.map { it.key })
        assertFalse(isHomeEmpty(emptyList(), emptyList(), listOf(desktop.copy(status = "disconnected")), emptyList()))
        assertTrue(isHomeEmpty(emptyList(), emptyList(), listOf(desktop.copy(status = "failed", hasConnected = false)), emptyList()))
    }

    @Test fun onboardingOffersTheThreeConnectionsAndAnExplicitSkip() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val opened = mutableListOf<String>()
        compose.setContent { MaterialTheme {
            ConnectionOnboardingScreen({ opened += "computer" }, { opened += "ssh" },
                { opened += "local" }, { opened += "later" })
        } }
        compose.onNodeWithText(context.getString(R.string.onboarding_connection_title)).assertIsDisplayed()
        val computer = compose.onNodeWithText(context.getString(R.string.onboarding_my_computer))
        val ssh = compose.onNodeWithText(context.getString(R.string.onboarding_ssh_host))
        val local = compose.onNodeWithText(context.getString(R.string.onboarding_phone_terminal))
        assertTrue(computer.getUnclippedBoundsInRoot().bottom < ssh.getUnclippedBoundsInRoot().top)
        assertTrue(ssh.getUnclippedBoundsInRoot().top < local.getUnclippedBoundsInRoot().top)
        listOf(computer, ssh, local).forEach { it.performScrollTo().assertHeightIsAtLeast(72.dp).performClick() }
        compose.onNodeWithText(context.getString(R.string.onboarding_later)).assertIsDisplayed().performClick()
        compose.runOnIdle { assertEquals(listOf("computer", "ssh", "local", "later"), opened) }
    }

    @Test fun onboardingAllowsLargeTextToScrollWithoutHidingSkip() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        var opened = false
        compose.setContent { MaterialTheme {
            CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, fontScale = 2f)) {
                Box(Modifier.size(320.dp, 480.dp)) {
                    ConnectionOnboardingScreen({}, {}, { opened = true }, {})
                }
            }
        } }
        compose.onNodeWithText(context.getString(R.string.onboarding_phone_terminal)).performScrollTo().assertIsDisplayed().performClick()
        compose.onNodeWithText(context.getString(R.string.onboarding_later)).assertIsDisplayed()
        compose.runOnIdle { assertTrue(opened) }
    }

    @Test fun onboardingCompletionSurvivesPreferenceReloadWithoutChangingTerminalSettings() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val preferences = DisplayPreferences(context)
        assertFalse(preferences.connectionOnboardingCompleted)
        val terminal = preferences.state.value
        preferences.completeConnectionOnboarding()
        val reloaded = DisplayPreferences(context)
        assertTrue(reloaded.connectionOnboardingCompleted)
        assertEquals(terminal, reloaded.state.value)
    }

    @Test fun pairingApprovalShowsTheLocalCodeAndKeepsCancellationAvailable() {
        var disconnected = false
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val waiting = desktop.copy(status = "approval", panes = emptyList(), hasConnected = false,
            pairingApproval = DesktopPairingApproval("375219", System.currentTimeMillis() / 1000 + 120))
        compose.setContent { MaterialTheme {
            DesktopScreen(waiting, {}, onRetry = {}, onDisconnect = { disconnected = true })
        } }
        compose.onNodeWithText("375 219").assertIsDisplayed()
        compose.onNodeWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.retry)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.device_unavailable)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(io.github.kuddev.pebrel.mobile.R.string.disconnect)).performClick()
        compose.runOnIdle { assertTrue(disconnected) }
    }

    @Test fun pairingMethodsKeepCameraInlineAndPreserveTheOtherEntryRoutes() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        var deployed = false
        compose.setContent { MaterialTheme { RelayForm({}, {}, { deployed = true }) } }
        compose.onNodeWithText(context.getString(R.string.pair_step_find)).assertIsDisplayed()
        compose.onNodeWithContentDescription(context.getString(R.string.pair_scan_frame)).assertHeightIsAtLeast(258.dp)
        compose.onNode(hasText(context.getString(R.string.pair_short_code)) and
            SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton))
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
        compose.onNodeWithContentDescription(context.getString(R.string.pair_scan_frame)).assertDoesNotExist()
        compose.onNodeWithContentDescription(context.getString(R.string.pair_short_code)).assertExists()
        compose.onNodeWithText(context.getString(R.string.pair_tab_paste))
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
        compose.onNodeWithContentDescription(context.getString(R.string.pair_short_code)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.pair_import_connect)).performScrollTo().assertIsNotEnabled()
        compose.onNodeWithText(context.getString(R.string.pair_relay_alternative)).performScrollTo()
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
        compose.runOnIdle { assertTrue(deployed) }
    }

    @Test fun pastedPairingInvitationIsValidatedBeforeRoutingAndBackStillWorks() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        var invitation = ""
        var cancelled = false
        val profile = RelayProfile("wss://relay.example.com", "fixture", "A".repeat(43), "UI pairing")
        compose.setContent { MaterialTheme { RelayForm({ cancelled = true }, { invitation = it }) } }
        compose.onNodeWithText(context.getString(R.string.pair_tab_paste))
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
        compose.onNode(hasSetTextAction()).performTextInput("invalid")
        compose.onNodeWithText(context.getString(R.string.pair_import_connect)).performScrollTo()
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
        compose.onNodeWithText(context.getString(R.string.pair_invalid)).performScrollTo().assertIsDisplayed()
        compose.runOnIdle { assertTrue(invitation.isEmpty()) }
        compose.onNode(hasSetTextAction()).performTextReplacement(profile.toJson().toString())
        compose.onNodeWithText(context.getString(R.string.pair_import_connect)).performScrollTo()
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
        compose.runOnIdle { assertEquals(profile.name, RelayProfile.parse(invitation).name) }
        compose.onNodeWithContentDescription(context.getString(R.string.back))
            .performSemanticsAction(SemanticsActions.OnClick) { it() }
        compose.runOnIdle { assertTrue(cancelled) }
    }

    @Test fun pairingCodeUsesOneAccessibleNumericFieldAtLargeFontSize() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        var entered = ""
        compose.setContent { MaterialTheme {
            val value = remember { mutableStateOf("") }
            CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, fontScale = 2f)) {
                Box(Modifier.size(320.dp, 160.dp)) {
                    PairingCodeField(value.value, { value.value = it; entered = it })
                }
            }
        } }
        compose.onNodeWithContentDescription(context.getString(R.string.pair_short_code))
            .assertHeightIsAtLeast(48.dp).performTextInput("12ab34567890")
        compose.runOnIdle { assertEquals("12345678", entered) }
        compose.onAllNodes(hasSetTextAction()).assertCountEquals(1)
    }
}
