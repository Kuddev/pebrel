package com.termux.terminal;

import android.os.Handler;
import android.os.Looper;
import java.nio.charset.StandardCharsets;
import java.util.UUID;
import java.util.concurrent.Executors;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Modified for Pebrel in 2026: injected transport and bounded asynchronous I/O.
 * Based on Termux TerminalSession; retain upstream licensing and Apache exceptions
 * documented in third_party/termux/UPSTREAM.md.
 * VT state stays on the main thread, matching TerminalView ownership. Stream I/O,
 * connection, resize and shutdown never run on it. See third_party/termux/UPSTREAM.md.
 */
public final class TerminalSession extends TerminalOutput {
    public final String mHandle = UUID.randomUUID().toString();
    public String mSessionName;
    private final SessionTransport transport;
    private final int transcriptRows;
    private TerminalSessionClient client;
    private TerminalEmulator emulator;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final SessionByteQueue received = new SessionByteQueue(64 * 1024);
    private final SessionByteQueue outgoing = new SessionByteQueue(128 * 1024);
    private final byte[] receiveBatch = new byte[16 * 1024];
    private final byte[] mUtf8InputBuffer = new byte[5];
    private final AtomicBoolean scheduled = new AtomicBoolean();
    private final AtomicReference<int[]> pendingSize = new AtomicReference<>();
    private final AtomicBoolean resizing = new AtomicBoolean();
    private final ExecutorService controls = Executors.newSingleThreadExecutor();
    private volatile boolean closed;
    private volatile boolean opened;
    private volatile boolean eof;
    private volatile int exitStatus = -1;
    private volatile String failure;
    private boolean finished;

    public TerminalSession(SessionTransport transport, int transcriptRows, TerminalSessionClient client) {
        this.transport = transport;
        this.transcriptRows = transcriptRows;
        this.client = client;
    }

    public void updateTerminalSessionClient(TerminalSessionClient value) {
        client = value;
        if (emulator != null) emulator.updateTerminalSessionClient(value);
    }

    public void updateSize(int columns, int rows, int cellWidth, int cellHeight) {
        pendingSize.set(new int[]{columns, rows, cellWidth, cellHeight});
        if (emulator == null) {
            emulator = new TerminalEmulator(this, columns, rows, cellWidth, cellHeight, transcriptRows, client);
            new Thread(() -> runTransport(columns, rows, cellWidth, cellHeight), "pebrel-terminal-reader").start();
        } else {
            emulator.resize(columns, rows, cellWidth, cellHeight);
            requestResize();
        }
    }

    private void runTransport(int columns, int rows, int cellWidth, int cellHeight) {
        try {
            transport.open(columns, rows, cellWidth, cellHeight);
            if (closed) return;
            opened = true;
            main.post(() -> {
                if (!closed) {
                    client.setTerminalShellPid(this, transport.pid());
                    client.onTransportReady(this);
                    requestResize();
                }
            });
            new Thread(this::writeTransport, "pebrel-terminal-writer").start();
            byte[] buffer = new byte[8192];
            try {
                int count;
                while (!closed && (count = transport.input().read(buffer)) != -1) {
                    if (count == 0) continue;
                    if (!received.put(buffer, 0, count)) break;
                    schedule(0);
                }
            } catch (java.io.IOException e) {
                // A Linux PTY returns EIO when its last slave closes; waitpid owns its exit status.
                if (transport.pid() <= 0 && !closed) throw e;
            }
            exitStatus = transport.awaitExit();
        } catch (Exception e) {
            if (!closed) failure = e.getClass().getSimpleName();
        } finally {
            opened = false;
            try { transport.close(); } catch (Exception ignored) { }
            outgoing.close();
            received.close();
            eof = true;
            schedule(0);
            controls.shutdown();
        }
    }

    private void writeTransport() {
        byte[] buffer = new byte[8192];
        try {
            int count;
            while (!closed && (count = outgoing.read(buffer, true)) != -1) {
                transport.output().write(buffer, 0, count);
                transport.output().flush();
            }
        } catch (Exception e) {
            if (!closed) {
                failure = e.getClass().getSimpleName();
                try { transport.close(); } catch (Exception ignored) { }
            }
        }
    }

    private void requestResize() {
        if (!opened || closed || !resizing.compareAndSet(false, true)) return;
        try {
            controls.execute(() -> {
                try {
                    int[] size;
                    while (!closed && (size = pendingSize.getAndSet(null)) != null)
                        transport.resize(size[0], size[1], size[2], size[3]);
                } catch (Exception e) {
                    failure = e.getClass().getSimpleName();
                    finishIfRunning();
                } finally {
                    resizing.set(false);
                    if (pendingSize.get() != null) requestResize();
                }
            });
        } catch (java.util.concurrent.RejectedExecutionException ignored) { resizing.set(false); }
    }

    private void schedule(long delay) {
        if (scheduled.compareAndSet(false, true)) main.postDelayed(this::drain, delay);
    }

    private void drain() {
        scheduled.set(false);
        try {
            int count = received.read(receiveBatch, false);
            if (count > 0) {
                android.os.Trace.beginSection("Pebrel.VT.append");
                try { emulator.append(receiveBatch, count); }
                finally { android.os.Trace.endSection(); }
                client.onTextChanged(this);
            }
        } catch (InterruptedException e) { Thread.currentThread().interrupt(); }
        if (received.pending() > 0) schedule(8);
        else if (eof && !finished) {
            finished = true;
            client.onSessionFinished(this);
        }
    }

    public boolean tryWrite(byte[] data, int offset, int count) {
        return opened && !closed && outgoing.offer(data, offset, count);
    }

    @Override public void write(byte[] data, int offset, int count) {
        if (!tryWrite(data, offset, count)) client.onInputRejected(this);
    }

    /** Write the Unicode code point to the terminal encoded in UTF-8. */
    public void writeCodePoint(boolean prependEscape, int codePoint) {
        if (codePoint > 1114111 || (codePoint >= 0xD800 && codePoint <= 0xDFFF)) {
            // 1114111 (= 2**16 + 1024**2 - 1) is the highest code point, [0xD800,0xDFFF] is the surrogate range.
            throw new IllegalArgumentException("Invalid code point: " + codePoint);
        }

        int bufferPosition = 0;
        if (prependEscape) mUtf8InputBuffer[bufferPosition++] = 27;

        if (codePoint <= /* 7 bits */0b1111111) {
            mUtf8InputBuffer[bufferPosition++] = (byte) codePoint;
        } else if (codePoint <= /* 11 bits */0b11111111111) {
            /* 110xxxxx leading byte with leading 5 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b11000000 | (codePoint >> 6));
            /* 10xxxxxx continuation byte with following 6 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b10000000 | (codePoint & 0b111111));
        } else if (codePoint <= /* 16 bits */0b1111111111111111) {
            /* 1110xxxx leading byte with leading 4 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b11100000 | (codePoint >> 12));
            /* 10xxxxxx continuation byte with following 6 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b10000000 | ((codePoint >> 6) & 0b111111));
            /* 10xxxxxx continuation byte with following 6 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b10000000 | (codePoint & 0b111111));
        } else { /* We have checked codePoint <= 1114111 above, so we have max 21 bits = 0b111111111111111111111 */
            /* 11110xxx leading byte with leading 3 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b11110000 | (codePoint >> 18));
            /* 10xxxxxx continuation byte with following 6 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b10000000 | ((codePoint >> 12) & 0b111111));
            /* 10xxxxxx continuation byte with following 6 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b10000000 | ((codePoint >> 6) & 0b111111));
            /* 10xxxxxx continuation byte with following 6 bits */
            mUtf8InputBuffer[bufferPosition++] = (byte) (0b10000000 | (codePoint & 0b111111));
        }
        write(mUtf8InputBuffer, 0, bufferPosition);
    }


    public TerminalEmulator getEmulator() { return emulator; }
    public String getTitle() { return emulator == null ? null : emulator.getTitle(); }
    public String getFailure() { return failure; }
    public boolean isRunning() { return !finished && !closed; }
    public int getExitStatus() { return exitStatus; }
    public int getPid() { return transport.pid(); }
    public String getCwd() { return transport.cwd(); }
    public void reset() { if (emulator != null) { emulator.reset(); client.onTextChanged(this); } }
    public void finishIfRunning() {
        if (closed) return;
        closed = true;
        received.close();
        outgoing.close();
        // Independent from a potentially blocked resize request.
        new Thread(() -> {
            try { transport.close(); } catch (Exception ignored) { }
        }, "pebrel-terminal-close").start();
    }
    @Override public void titleChanged(String oldTitle, String newTitle) { client.onTitleChanged(this); }
    @Override public void onCopyTextToClipboard(String text) { client.onCopyTextToClipboard(this, text); }
    @Override public void onPasteTextFromClipboard() { client.onPasteTextFromClipboard(this); }
    @Override public void onBell() { client.onBell(this); }
    @Override public void onColorsChanged() { client.onColorsChanged(this); }
}
