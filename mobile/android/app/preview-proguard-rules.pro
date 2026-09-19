# AndroidJUnitRunner shares this dependency with the optimized target APK.
# Preserve its binary API so instrumentation can start without altering release rules.
-keep class androidx.tracing.** { *; }
# These APIs are called across the instrumentation/target APK boundary.
-keep class io.github.kuddev.pebrel.terminal.GhosttyCore { public *; }
-keep class io.github.kuddev.pebrel.terminal.TerminalFrame { public *; }
-keep class io.github.kuddev.pebrel.terminal.TerminalSession { public *; }
-keep class io.github.kuddev.pebrel.terminal.TerminalCallbacks { public *; }
-keep interface io.github.kuddev.pebrel.terminal.SessionTransport { *; }
-keep class io.github.kuddev.pebrel.terminal.LocalPtyTransport { public *; }
-keep class io.github.kuddev.pebrel.terminal.GhosttyView { public *; }

# The test runner and optimized target share Kotlin runtime classes. R8 may
# otherwise remove or rename APIs used only by the separate instrumentation APK.
-keep class kotlin.** { *; }
# Resource IDs are accessed from the separate UI instrumentation APK.
-keep class io.github.kuddev.pebrel.mobile.R$string { public static <fields>; }
# Real OpenSSH regression uses these app-owned APIs across the test APK boundary.
-keep class io.github.kuddev.pebrel.mobile.connection.HostProfile { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SshConnection { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SshTerminalTransport { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SshFailure { public *; }
-keep enum io.github.kuddev.pebrel.mobile.connection.SshStage { *; }
-keep enum io.github.kuddev.pebrel.mobile.connection.SshFailureKind { *; }
