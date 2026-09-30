import net from 'node:net';

const MAX_PROXY_CONNECTIONS = 2;
const MAX_PROXY_CHUNK = 2 * 1024 * 1024 + 1024;

/**
 * Forward encrypted bytes from a loopback-only socket to the LAN relay. The
 * desktop connector therefore has a private wss://127.0.0.1 entry while the
 * phone uses the separately bound HTTPS/WSS listener.
 */
export async function startLoopbackProxy(targetHost, targetPort) {
  const sockets = new Set();
  const connections = new Set();
  const server = net.createServer(socket => {
    if (connections.size >= MAX_PROXY_CONNECTIONS) { socket.destroy(); return; }
    connections.add(socket);
    sockets.add(socket);
    socket.setNoDelay(true);
    const upstream = net.createConnection({ host: targetHost, port: targetPort });
    sockets.add(upstream);
    upstream.setNoDelay(true);
    const close = () => {
      socket.destroy(); upstream.destroy(); connections.delete(socket);
      sockets.delete(socket); sockets.delete(upstream);
    };
    socket.on('data', bytes => {
      if (bytes.length > MAX_PROXY_CHUNK) { close(); return; }
      if (!upstream.write(bytes)) socket.pause();
    });
    upstream.on('drain', () => socket.resume());
    upstream.on('data', bytes => {
      if (bytes.length > MAX_PROXY_CHUNK) { close(); return; }
      if (!socket.write(bytes)) upstream.pause();
    });
    socket.on('drain', () => upstream.resume());
    socket.on('error', close); upstream.on('error', close);
    socket.on('close', close); upstream.on('close', close);
  });
  server.maxConnections = MAX_PROXY_CONNECTIONS;
  await new Promise((resolve, reject) => {
    const fail = error => { server.off('listening', resolve); reject(error); };
    server.once('error', fail);
    server.listen(0, '127.0.0.1', () => { server.off('error', fail); resolve(); });
  });
  return {
    server,
    port: server.address().port,
    close: async () => {
      for (const socket of sockets) socket.destroy();
      await new Promise(resolve => server.close(() => resolve()));
    },
  };
}
