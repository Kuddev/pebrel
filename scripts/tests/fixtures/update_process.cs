// Native handoff fixture only. This does not emulate GPUI or an Inno installer.
using System;
using System.IO;
using System.Reflection;
using System.Threading;

class UpdateProcess {
    static int Main(string[] args) {
#if INSTALLER
        string target = null;
        foreach (var argument in args) {
            if (argument.StartsWith("/DIR=")) target = argument.Substring(5).Trim('"');
        }
        if (target == null) return 2;
        var directory = Environment.GetEnvironmentVariable("PEBREL_HANDOFF_FIXTURE");
        File.WriteAllText(Path.Combine(directory, "installer-started"), "started");
        if (!File.Exists(Path.Combine(directory, "parent-exited"))) return 3;
        if (Environment.GetEnvironmentVariable("PEBREL_HANDOFF_FAIL_INSTALL") == "1") return 17;
        if (Environment.GetEnvironmentVariable("PEBREL_HANDOFF_NOOP_INSTALL") == "1") return 0;
        File.Copy(Path.Combine(directory, "candidate.exe"), Path.Combine(target, "pebrel.exe"), true);
        return 0;
#else
        if (args.Length > 0 && args[0] == "--version") {
#if NEW_VERSION
            Console.WriteLine("Pebrel 9.9.9 (handoff-fixture)");
#elif SAME_VERSION
            Console.WriteLine("Pebrel 1.8.0 (replacement-fixture)");
#else
            Console.WriteLine("Pebrel 1.8.0 (handoff-fixture)");
#endif
            return 0;
        }
        var directory = Environment.GetEnvironmentVariable("PEBREL_HANDOFF_FIXTURE");
        if (args.Length > 0 && args[0].StartsWith("wait")) {
            var signal = args[0] == "wait-other" ? "exit-other" : "exit-parent";
            while (!File.Exists(Path.Combine(directory, signal))) Thread.Sleep(25);
            File.WriteAllText(Path.Combine(directory, "parent-exited"), "exited");
        } else {
            File.WriteAllText(Path.Combine(directory, "new-launch"),
                Assembly.GetExecutingAssembly().Location + "\n" +
                Environment.GetEnvironmentVariable("PEBREL_UPDATE_RESTORE") + "\n" +
                Environment.GetEnvironmentVariable("PEBREL_CONFIG_DIR"));
        }
        return 0;
#endif
    }
}
