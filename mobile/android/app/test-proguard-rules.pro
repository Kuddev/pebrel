# The emulator invokes this instrumented suite by its fully qualified class name.
-keep class io.github.kuddev.pebrel.mobile.PrototypeVisualTest { *; }
-keep class io.github.kuddev.pebrel.mobile.GhosttyEngineTest { *; }
-keep class io.github.kuddev.pebrel.mobile.SshIntegrationTest { *; }

# Error Prone's unused compile-time IncompatibleModifiers annotation mentions
# Java compiler model types, which Android does not implement. Require the
# annotation to be discarded; this does not suppress missing runtime APIs.
-dontwarn javax.lang.model.element.Modifier
-checkdiscard class com.google.errorprone.annotations.IncompatibleModifiers
