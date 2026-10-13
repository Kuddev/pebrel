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
# Real-app touch tests observe the session opened through Home, rather than
# substituting an Activity layout. Preserve only this javap-verified test boundary.
-keepclassmembers class io.github.kuddev.pebrel.mobile.PebrelApplication {
    public io.github.kuddev.pebrel.mobile.session.SessionRepository getSessions();
    public android.graphics.Typeface terminalTypeface(java.lang.String);
}
-keepclassmembers class io.github.kuddev.pebrel.mobile.session.SessionRepository {
    public kotlinx.coroutines.flow.StateFlow getSessions();
    public io.github.kuddev.pebrel.mobile.session.DisplayPreferences getDisplay();
    public void closeTerminal(java.lang.String);
}
-keepclassmembers class io.github.kuddev.pebrel.mobile.session.LocalSession {
    public java.lang.String getId();
    public java.lang.String getSource();
    public java.lang.String getStatus();
    public io.github.kuddev.pebrel.terminal.TerminalSession getTerminal();
}
-keepclassmembers class io.github.kuddev.pebrel.mobile.session.DisplayPreferences {
    public kotlinx.coroutines.flow.StateFlow getState();
}
-keepclassmembers class io.github.kuddev.pebrel.mobile.session.TerminalPreferences {
    public java.lang.String getFontFamily();
    public int getFontSize();
}
-keep interface kotlinx.coroutines.flow.StateFlow { public *; }
# Real OpenSSH regression uses these app-owned APIs across the test APK boundary.
-keep class io.github.kuddev.pebrel.mobile.connection.HostProfile { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SshConnection { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SshTerminalTransport { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SshFailure { public *; }
-keep enum io.github.kuddev.pebrel.mobile.connection.SshStage { *; }
-keep enum io.github.kuddev.pebrel.mobile.connection.SshSessionMode { *; }
-keep enum io.github.kuddev.pebrel.mobile.connection.SshFailureKind { *; }
# The SFTP instrumentation exercises suspend APIs from a separate APK. Preserve
# only its javap-verified public boundary; production release shrinking is unchanged.
-keep,includedescriptorclasses class io.github.kuddev.pebrel.mobile.connection.SftpClient { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SftpClientKt { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SftpEntry { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SftpListing { public *; }
-keep class io.github.kuddev.pebrel.mobile.connection.SftpContent { public *; }
-keep,includedescriptorclasses class kotlinx.coroutines.BuildersKt { public *; }
-keep,includedescriptorclasses class kotlinx.coroutines.CompletableDeferredKt { public *; }
-keep,includedescriptorclasses class kotlinx.coroutines.DelayKt { public *; }
-keep,includedescriptorclasses class kotlinx.coroutines.Dispatchers { public *; }
-keep,includedescriptorclasses class kotlinx.coroutines.JobKt { public *; }
-keep,includedescriptorclasses class kotlinx.coroutines.TimeoutKt { public *; }
-keep interface kotlinx.coroutines.CompletableDeferred { *; }
-keep interface kotlinx.coroutines.Deferred { *; }
-keep interface kotlinx.coroutines.CoroutineScope { *; }
-keep interface kotlinx.coroutines.Job { *; }
