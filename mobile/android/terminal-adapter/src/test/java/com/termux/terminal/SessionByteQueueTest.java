package com.termux.terminal;

import org.junit.Test;
import static org.junit.Assert.*;
import java.util.concurrent.*;

public class SessionByteQueueTest {
    @Test public void overflowNeverPartiallyEnqueuesACommand() throws Exception {
        SessionByteQueue queue = new SessionByteQueue(5);
        assertTrue(queue.offer(new byte[]{1, 2, 3}, 0, 3));
        assertFalse(queue.offer(new byte[]{4, 5, 6}, 0, 3));
        byte[] out = new byte[5];
        assertEquals(3, queue.read(out, false));
        assertArrayEquals(new byte[]{1, 2, 3, 0, 0}, out);
    }

    @Test public void wrapAndEofPreserveFinalBytes() throws Exception {
        SessionByteQueue queue = new SessionByteQueue(4);
        queue.put(new byte[]{1, 2, 3}, 0, 3);
        assertEquals(2, queue.read(new byte[2], false));
        queue.put(new byte[]{4, 5, 6}, 0, 3);
        queue.close();
        byte[] out = new byte[4];
        assertEquals(4, queue.read(out, false));
        assertArrayEquals(new byte[]{3, 4, 5, 6}, out);
        assertEquals(-1, queue.read(out, true));
        assertFalse(queue.offer(new byte[]{7}, 0, 1));
    }

    @Test public void closeReleasesBlockedProducersAndConsumers() throws Exception {
        ExecutorService worker = Executors.newSingleThreadExecutor();
        SessionByteQueue full = new SessionByteQueue(1);
        SessionByteQueue empty = new SessionByteQueue(1);
        try {
            full.offer(new byte[]{1}, 0, 1);
            Future<Boolean> producer = worker.submit(() -> full.put(new byte[]{2}, 0, 1));
            full.close();
            assertFalse(producer.get(2, TimeUnit.SECONDS));
            Future<Integer> consumer = worker.submit(() -> empty.read(new byte[1], true));
            empty.close();
            assertEquals(-1, (int) consumer.get(2, TimeUnit.SECONDS));
        } finally { full.close(); empty.close(); worker.shutdownNow(); }
    }
}
