package com.termux.terminal;

import java.io.Closeable;
import java.io.InputStream;
import java.io.OutputStream;

/** A byte stream with terminal geometry; all methods are called off the UI thread. */
public interface SessionTransport extends Closeable {
    void open(int columns, int rows, int cellWidth, int cellHeight) throws Exception;
    InputStream input();
    OutputStream output();
    void resize(int columns, int rows, int cellWidth, int cellHeight) throws Exception;
    int awaitExit() throws Exception;
    default int pid() { return 0; }
    default String cwd() { return null; }
}
