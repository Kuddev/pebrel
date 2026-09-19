package io.github.kuddev.pebrel.mobile

import android.content.Intent
import android.os.Environment
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import androidx.test.uiautomator.Until
import java.io.File
import org.junit.Test
import org.junit.runner.RunWith

/** Real preview APK interaction, without seeding demo hosts or terminal output. */
@RunWith(AndroidJUnit4::class)
class PrototypeVisualTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val target = instrumentation.targetContext
    private val device = UiDevice.getInstance(instrumentation)
    private val waitMs = 8_000L

    // CI clears the target before instrumentation starts. Calling pm clear from
    // inside the test kills the process hosting the instrumentation itself.
    @Test
    fun prototypePagesAndDraftIsolation() {
        val launch = requireNotNull(target.packageManager.getLaunchIntentForPackage(target.packageName))
        target.startActivity(launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
        check(device.wait(Until.hasObject(By.pkg(target.packageName)), waitMs))
        try {
            waitLabel(R.string.sessions)
            waitLabel(R.string.ssh_hosts)
            waitLabel(R.string.computers)
            capture("01-home-empty")

            waitLabel(R.string.add_ssh).click()
            check(!scrollTo(target.getString(R.string.save)).isEnabled)
            capture("02-host-form")
            device.pressBack()
            waitLabel(R.string.sessions)
            waitLabel(R.string.add_ssh).click()
            val fields = edits(4)
            check(kotlin.math.abs(fields[2].visibleBounds.top - fields[3].visibleBounds.top) < 8) {
                "Username and port must share the prototype row"
            }
            fields[0].text = "Visual Test Host"
            fields[1].text = "192.0.2.10"
            fields[2].text = "tester"
            fields[3].text = "22"
            // The prototype places Save after the form, so scroll it into view.
            scrollTo(target.getString(R.string.save)).click()
            waitLabel("Visual Test Host")
            check(!hasLabel(target.getString(R.string.password)))
            capture("03-home-saved-host")

            waitLabel(R.string.settings).click()
            waitLabel(R.string.theme_settings)
            capture("04-settings")
            waitLabel(R.string.theme_settings).click()
            scrollTo("Nord").click()
            capture("05-theme-nord")
            waitLabel(R.string.back).click()
            waitLabel(R.string.back).click()
            waitLabel(R.string.sessions)
            capture("06-home-nord")

            scrollTo(target.getString(R.string.local_terminal)).click()
            val input = edits(1).last()
            input.click()
            input.text = "git"
            waitLabel("git status").click()
            check(edits(1).last().text == "git status")
            capture("07-terminal-candidate")
            edits(1).last().text = "printf '\\033[32mGhostty 中文 😀 é\\033[0m\\n'"
            waitLabel(R.string.send).click()
            val sendDeadline = SystemClock.uptimeMillis() + waitMs
            while (edits(1).last().text.isNotEmpty() && SystemClock.uptimeMillis() < sendDeadline) SystemClock.sleep(100)
            check(edits(1).last().text.isEmpty())
            // Record actual ANSI/CJK/emoji rendering as well as the interface chrome.
            SystemClock.sleep(500)
            capture("07b-ghostty-output")
            edits(1).last().text = "git status"
            // Back button in header, rather than the IME Back key.
            waitLabel(R.string.back).click()
            waitLabel(R.string.sessions)
            capture("08-session-thumbnail")

            scrollTo(target.getString(R.string.local_terminal)).click()
            check(edits(1).last().text.isEmpty()) { "A new session inherited another session draft" }
            edits(1).last().text = "second session draft"
            waitLabel(R.string.long_editor).click()
            check(edits(1).last().text == "second session draft")
            capture("09-long-editor")
            waitLabel(R.string.editor_done).click()
            waitLabel(R.string.back).click()

            // 360 dp width, not 360 physical pixels at the default 420 dpi.
            shell("wm size 720x1600")
            shell("wm density 320")
            waitLabel(R.string.sessions)
            capture("10-narrow-home")
            waitLabel(R.string.settings).click()
            waitLabel(R.string.theme_settings)
            capture("11-narrow-settings")
            waitLabel(R.string.back).click()
            waitLabel(R.string.add_ssh).click()
            val sshFields = edits(4)
            sshFields[0].text = "OpenSSH UI Test"
            sshFields[1].text = "10.0.2.2"
            sshFields[2].text = "pebreltest"
            sshFields[3].text = "2222"
            // The inline password is directly editable, not an authentication tile.
            repeat(8) {
                if (device.findObject(By.desc(target.getString(R.string.auth_password))) == null) {
                    device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4,
                        device.displayWidth / 2, device.displayHeight / 3, 25)
                }
            }
            val password = requireNotNull(device.findObject(By.desc(target.getString(R.string.auth_password))))
            password.text = "pebrel-test-only"
            capture("12-inline-password-pills")
            scrollTo(target.getString(R.string.save_connect)).click()
            waitLabel(R.string.trust_connect)
            capture("13-ssh-fingerprint")
            waitLabel(R.string.trust_connect).click()
            waitLabel(R.string.raw_terminal)
            capture("14-russh-terminal-frame")
        } finally {
            capture("last-state")
            shell("wm size reset")
            shell("wm density reset")
        }
    }

    private fun waitLabel(id: Int): UiObject2 = waitLabel(target.getString(id))
    private fun waitLabel(value: String): UiObject2 {
        val deadline = SystemClock.uptimeMillis() + waitMs
        while (SystemClock.uptimeMillis() < deadline) {
            device.findObject(By.text(value))?.let { return it }
            device.findObject(By.desc(value))?.let { return it }
            SystemClock.sleep(100)
        }
        error("Missing text or content description: $value")
    }

    private fun edits(count: Int): List<UiObject2> {
        val deadline = SystemClock.uptimeMillis() + waitMs
        while (SystemClock.uptimeMillis() < deadline) {
            val found = device.findObjects(By.clazz("android.widget.EditText"))
            if (found.size >= count) return found
            SystemClock.sleep(100)
        }
        error("Expected $count editable fields")
    }

    private fun hasLabel(value: String) = device.hasObject(By.text(value)) || device.hasObject(By.desc(value))

    private fun scrollTo(value: String): UiObject2 {
        repeat(10) {
            device.findObject(By.text(value))?.let { return it }
            device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4,
                device.displayWidth / 2, device.displayHeight / 3, 30)
            device.waitForIdle()
        }
        return waitLabel(value)
    }

    private fun capture(name: String) {
        device.waitForIdle()
        val root = requireNotNull(target.getExternalFilesDir(Environment.DIRECTORY_PICTURES)).resolve("visual")
        check(root.exists() || root.mkdirs())
        check(device.takeScreenshot(File(root, "$name.png")))
    }

    private fun shell(command: String) {
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command)).use { it.readBytes() }
    }
}
