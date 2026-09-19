package com.termux.terminal;

/** Bounded stream queue. UI producers use atomic offer; only I/O workers may wait. */
public final class SessionByteQueue {
    private final byte[] data;
    private int head;
    private int size;
    private boolean closed;

    public SessionByteQueue(int capacity) {
        if (capacity <= 0) throw new IllegalArgumentException("capacity");
        data = new byte[capacity];
    }

    public synchronized boolean offer(byte[] source, int offset, int length) {
        if (offset < 0 || length < 0 || offset > source.length - length)
            throw new IndexOutOfBoundsException();
        if (closed || length > data.length - size) return false;
        int tail = (head + size) % data.length;
        int first = Math.min(length, data.length - tail);
        System.arraycopy(source, offset, data, tail, first);
        System.arraycopy(source, offset + first, data, 0, length - first);
        size += length;
        notifyAll();
        return true;
    }

    public synchronized boolean put(byte[] source, int offset, int length) throws InterruptedException {
        if (length > data.length) throw new IllegalArgumentException("batch exceeds capacity");
        while (!closed && length > data.length - size) wait();
        return offer(source, offset, length);
    }

    public synchronized int read(byte[] target, boolean waitForInput) throws InterruptedException {
        while (size == 0 && !closed && waitForInput) wait();
        if (size == 0) return closed ? -1 : 0;
        int count = Math.min(size, target.length);
        int first = Math.min(count, data.length - head);
        System.arraycopy(data, head, target, 0, first);
        System.arraycopy(data, 0, target, first, count - first);
        head = (head + count) % data.length;
        size -= count;
        notifyAll();
        return count;
    }

    public synchronized int pending() { return size; }

    /** EOF preserves bytes already received, including the last unterminated line. */
    public synchronized void close() {
        closed = true;
        notifyAll();
    }
}
