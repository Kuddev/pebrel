package com.termux.terminal;

import android.os.ParcelFileDescriptor;
import android.system.Os;
import android.system.OsConstants;
import java.io.File;
import java.io.InputStream;
import java.io.OutputStream;

/** Termux JNI PTY with public Android descriptor APIs instead of hidden-field reflection. */
public final class LocalPtyTransport implements SessionTransport {
    private final String directory;
    private ParcelFileDescriptor master;
    private InputStream input;
    private OutputStream output;
    private volatile int process;
    private boolean closed;

    public LocalPtyTransport(String directory) { this.directory = directory; }

    @Override
    public synchronized void open(int columns, int rows, int cellWidth, int cellHeight) throws Exception {
        if (closed) throw new java.io.IOException("Session closed");
        int[] pid = new int[1];
        int fd = JNI.createSubprocess("/system/bin/sh", directory, new String[]{"sh", "-i"},
            new String[]{"HOME=" + directory, "PATH=/system/bin:/system/xbin", "TERM=xterm-256color",
                "COLORTERM=truecolor", "TMPDIR=" + directory, "LANG=C.UTF-8", "PS1=\\w $ "},
            pid, rows, columns, cellWidth, cellHeight);
        process = pid[0];
        master = ParcelFileDescriptor.adoptFd(fd);
        input = new ParcelFileDescriptor.AutoCloseInputStream(ParcelFileDescriptor.dup(master.getFileDescriptor()));
        output = new ParcelFileDescriptor.AutoCloseOutputStream(ParcelFileDescriptor.dup(master.getFileDescriptor()));
    }

    @Override public InputStream input() { return input; }
    @Override public OutputStream output() { return output; }
    @Override public int pid() { return process; }

    @Override
    public synchronized void resize(int columns, int rows, int cellWidth, int cellHeight) {
        if (master != null && !closed)
            JNI.setPtyWindowSize(master.getFd(), rows, columns, cellWidth, cellHeight);
    }

    @Override
    public int awaitExit() {
        int pid = process;
        if (pid <= 0) return -1;
        int status = JNI.waitFor(pid);
        process = 0;
        return status;
    }

    @Override public String cwd() {
        try { return new File("/proc/" + process + "/cwd").getCanonicalPath(); }
        catch (Exception ignored) { return directory; }
    }

    @Override
    public synchronized void close() {
        if (closed) return;
        closed = true;
        if (process > 0) {
            try { Os.kill(-process, OsConstants.SIGKILL); }
            catch (Exception ignored) {
                try { Os.kill(process, OsConstants.SIGKILL); } catch (Exception ignoredAgain) { }
            }
        }
        try { if (input != null) input.close(); } catch (Exception ignored) { }
        try { if (output != null) output.close(); } catch (Exception ignored) { }
        try { if (master != null) master.close(); } catch (Exception ignored) { }
    }
}
