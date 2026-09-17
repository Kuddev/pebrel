import http from 'node:http';
import { randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import QRCode from 'qrcode';

const LOOPBACK = new Set(['127.0.0.1', '::1']);

function isLoopback(socket) {
  const address = socket.remoteAddress?.replace(/^::ffff:/, '');
  return LOOPBACK.has(address);
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, character => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[character]));
}

function scriptText(value) {
  return JSON.stringify(value).replace(/</g, '\\u003c').replace(/>/g, '\\u003e').replace(/&/g, '\\u0026');
}

function pageMarkup(invitation, svg, route) {
  const raw = JSON.stringify(invitation);
  const title = escapeHtml(invitation.name);
  const encoded = scriptText(raw);
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self';">
<title>Pebrel · Pair computer</title>
<style>
:root { color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; background: #10111a; color: #f5f2ff; }
* { box-sizing: border-box; }
body { min-height: 100vh; margin: 0; display: grid; place-items: center; padding: 28px; }
main { width: min(560px, 100%); border: 1px solid #302d4b; border-radius: 22px; background: #171827; padding: 30px; box-shadow: 0 20px 70px #08081099; text-align: center; }
.brand { color: #b9a8ff; letter-spacing: .12em; text-transform: uppercase; font-size: 12px; font-weight: 700; }
h1 { margin: 10px 0 6px; font-size: clamp(24px, 5vw, 34px); }
.name { color: #cbc6da; margin: 0 0 22px; }
.qr { display: inline-grid; place-items: center; padding: 18px; border-radius: 18px; background: #f8f7ff; }
.qr svg { width: min(360px, 70vw); height: auto; display: block; }
.status { min-height: 24px; margin: 22px 0 14px; color: #d5cff1; }
.status[data-state="paired"] { color: #8de0bb; }
.status[data-state="error"] { color: #ff9b9b; }
button { border: 1px solid #4f477a; border-radius: 10px; background: #282344; color: #f5f2ff; font: inherit; padding: 10px 15px; cursor: pointer; }
button:hover { background: #352d5a; }
button:focus-visible { outline: 3px solid #b9a8ff; outline-offset: 3px; }
.hint { color: #9791aa; font-size: 13px; line-height: 1.5; margin: 18px auto 0; max-width: 42ch; }
.copied { min-height: 18px; margin: 8px 0 0; color: #8de0bb; font-size: 13px; }
</style></head><body><main>
<div class="brand">Pebrel</div><h1>Pair this computer</h1><p class="name">${title}</p>
<div class="qr" role="img" aria-label="Pebrel phone pairing QR code">${svg}</div>
<div id="status" class="status" role="status" aria-live="polite">Waiting for the phone…</div>
<button id="copy" type="button">Copy invitation</button><div id="copied" class="copied" aria-live="polite"></div>
<p class="hint">Scan this code in Pebrel on the same trusted network. Keep this window private while pairing.</p>
</main><script>
const invite = ${encoded};
const status = document.getElementById('status');
const copied = document.getElementById('copied');
const labels = { connecting: 'Connecting to the local relay…', waiting_for_phone: 'Waiting for the phone…', paired: 'Phone connected', reconnecting: 'Reconnecting…', connection_failed: 'Connection failed', authentication_failed: 'Credentials rejected', server_unavailable: 'Relay unavailable', device_already_connected: 'Another phone is already connected' };
document.getElementById('copy').addEventListener('click', async () => { try { await navigator.clipboard.writeText(invite); copied.textContent = 'Invitation copied'; } catch { copied.textContent = 'Copy was unavailable'; } });
async function refresh() { try { const response = await fetch('${route}/status', { cache: 'no-store' }); if (!response.ok) return; const value = await response.json(); status.textContent = labels[value.status] || value.status; status.dataset.state = value.status === 'paired' ? 'paired' : value.status.includes('failed') || value.status.includes('error') ? 'error' : ''; } catch { /* the helper may be closing */ } }
refresh(); setInterval(refresh, 1000);
</script></body></html>`;
}

/** Serve the QR page on IPv4 loopback only; no LAN interface can read its secret. */
export async function startQrPage(invitation, { host = '127.0.0.1', port = 0 } = {}) {
  if (host !== '127.0.0.1') throw new Error('qr_page_loopback_only');
  const route = `/pair/${randomBytes(18).toString('base64url')}`;
  const svg = await QRCode.toString(JSON.stringify(invitation), {
    type: 'svg', margin: 2, width: 360, errorCorrectionLevel: 'M',
    color: { dark: '#171827', light: '#f8f7ff' },
  });
  let currentStatus = 'connecting';
  const server = http.createServer((request, response) => {
    if (!isLoopback(request.socket)) {
      response.writeHead(403, { 'content-type': 'text/plain; charset=utf-8' });
      response.end('loopback only\n');
      return;
    }
    let url;
    try { url = new URL(request.url ?? '/', 'http://127.0.0.1'); }
    catch {
      response.writeHead(400, { 'content-type': 'text/plain; charset=utf-8' });
      response.end('bad request\n');
      return;
    }
    if (request.method !== 'GET' || (url.pathname !== route && url.pathname !== `${route}/status`)) {
      response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
      response.end('not found\n');
      return;
    }
    response.setHeader('cache-control', 'no-store');
    if (url.pathname === `${route}/status`) {
      response.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
      response.end(JSON.stringify({ status: currentStatus }));
      return;
    }
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    response.end(pageMarkup(invitation, svg, route));
  });
  server.maxConnections = 8;
  server.requestTimeout = 5_000;
  server.headersTimeout = 5_000;
  await new Promise((resolve, reject) => {
    const fail = error => { server.off('listening', resolve); reject(error); };
    server.once('error', fail);
    server.listen(port, host, () => { server.off('error', fail); resolve(); });
  });
  const actualPort = server.address().port;
  return {
    server,
    route,
    url: `http://127.0.0.1:${actualPort}${route}`,
    setStatus(value) { currentStatus = String(value).slice(0, 64); },
    close: async () => {
      await new Promise(resolve => server.close(() => resolve()));
    },
  };
}

/** Open the local page without invoking a shell or interpolating user input. */
export function openBrowser(url) {
  let command;
  let args;
  if (process.platform === 'win32') { command = 'cmd.exe'; args = ['/c', 'start', '', url]; }
  else if (process.platform === 'darwin') { command = 'open'; args = [url]; }
  else { command = 'xdg-open'; args = [url]; }
  const child = spawn(command, args, { detached: true, stdio: 'ignore', windowsHide: true });
  child.on('error', () => {});
  child.unref();
  return child;
}
