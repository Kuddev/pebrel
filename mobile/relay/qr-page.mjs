import http from 'node:http';
import { randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import QRCode from 'qrcode';

const LOOPBACK = new Set(['127.0.0.1', '::1']);
const SAFE_STATUSES = new Set([
  'connecting', 'waiting_for_phone', 'paired', 'reconnecting',
  'connection_failed', 'authentication_failed', 'server_unavailable',
  'device_already_connected', 'runtime_unavailable', 'network_changed',
  'network_change_failed', 'helper_error',
]);
const ERROR_STATUSES = new Set([
  'connection_failed', 'authentication_failed', 'server_unavailable',
  'device_already_connected', 'runtime_unavailable', 'network_change_failed', 'helper_error',
]);

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

function advertisedEndpoint(invitation) {
  try { return new URL(invitation.url).host; }
  catch { return 'unavailable'; }
}

function normalizeNetworkOptions(value) {
  if (!Array.isArray(value)) return [];
  return value.slice(0, 32).map(item => {
    const address = typeof item?.address === 'string' ? item.address.trim() : '';
    const interfaceName = typeof item?.interfaceName === 'string'
      ? item.interfaceName.replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, 64) : '';
    if (!address || address.length > 253 || /[\u0000-\u001f\u007f]/.test(address)) return null;
    return { address, interfaceName, defaultRoute: item?.defaultRoute === true };
  }).filter(Boolean).filter((item, index, values) => values.findIndex(other => other.address === item.address) === index);
}

function readBoundedBody(request, limit = 1024) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let length = 0;
    let settled = false;
    request.on('data', chunk => {
      if (settled) return;
      length += chunk.length;
      if (length > limit) {
        settled = true;
        request.resume();
        reject(new Error('request_too_large'));
        return;
      }
      chunks.push(chunk);
    });
    request.on('end', () => {
      if (!settled) { settled = true; resolve(Buffer.concat(chunks).toString('utf8')); }
    });
    request.on('error', error => { if (!settled) { settled = true; reject(error); } });
  });
}

function pageMarkup(invitation, svg, route, addresses, currentAddress) {
  const raw = JSON.stringify(invitation);
  const title = escapeHtml(invitation.name);
  const endpoint = escapeHtml(advertisedEndpoint(invitation));
  const options = normalizeNetworkOptions(addresses);
  const picker = options.length > 1 ? `<form id="network-form" class="network-form">
<label for="network">Network interface / 网络接口</label><select id="network">${options.map(item =>
  `<option value="${escapeHtml(item.address)}"${item.address === currentAddress ? ' selected' : ''}>${escapeHtml(item.interfaceName ? `${item.interfaceName} · ${item.address}` : item.address)}${item.defaultRoute ? ' · default / 默认' : ''}</option>`).join('')}</select>
<button id="network-submit" type="submit">Use this address / 使用此地址</button><small>Changing the network disconnects the phone. Scan the new QR to reconnect. / 切换网络会断开手机连接，请扫描新二维码重新连接。</small></form>` : '';
  const encoded = scriptText(raw);
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; form-action 'self';">
<title>Pebrel · Pair computer</title>
<style>
:root { color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; background: #10111a; color: #f5f2ff; }
* { box-sizing: border-box; }
body { min-height: 100vh; margin: 0; display: grid; place-items: center; padding: 28px; }
main { width: min(560px, 100%); border: 1px solid #302d4b; border-radius: 22px; background: #171827; padding: 30px; box-shadow: 0 20px 70px #08081099; text-align: center; }
.brand { color: #b9a8ff; letter-spacing: .12em; text-transform: uppercase; font-size: 12px; font-weight: 700; }
h1 { margin: 10px 0 6px; font-size: clamp(24px, 5vw, 34px); }
.name { color: #cbc6da; margin: 0 0 22px; }
.endpoint { color: #b8b1cc; margin: -12px 0 22px; font-size: 13px; }
.endpoint code { color: #f5f2ff; font: inherit; }
.qr { display: inline-grid; place-items: center; padding: 18px; border-radius: 18px; background: #f8f7ff; }
.qr svg { width: min(360px, 70vw); height: auto; display: block; }
.status { min-height: 24px; margin: 22px 0 14px; color: #d5cff1; }
.status[data-state="paired"] { color: #8de0bb; }
.status[data-state="error"] { color: #ff9b9b; }
button { border: 1px solid #4f477a; border-radius: 10px; background: #282344; color: #f5f2ff; font: inherit; padding: 10px 15px; cursor: pointer; }
button:hover { background: #352d5a; }
button:focus-visible, select:focus-visible { outline: 3px solid #b9a8ff; outline-offset: 3px; }
button:disabled { opacity: .55; cursor: wait; }
.network-form { margin: 18px 0; display: grid; gap: 10px; text-align: left; }
.network-form label { font-size: 13px; color: #cbc6da; }
select { width: 100%; min-height: 44px; padding: 10px; border: 1px solid #4f477a; border-radius: 10px; background: #202034; color: #f5f2ff; font: inherit; }
.network-form button { min-height: 44px; }
.network-form small { color: #aaa3bb; line-height: 1.5; }
.hint { color: #9791aa; font-size: 13px; line-height: 1.5; margin: 18px auto 0; max-width: 42ch; }
.copied { min-height: 18px; margin: 8px 0 0; color: #8de0bb; font-size: 13px; }
</style></head><body><main>
<div class="brand">Pebrel</div><h1>连接手机 · Connect phone</h1><p class="name">${title}</p>
<p class="endpoint">Phone address / 手机连接地址: <code>${endpoint}</code></p>
<div class="qr" role="img" aria-label="Pebrel phone pairing QR code">${svg}</div>
<div id="status" class="status" role="status" aria-live="polite">Waiting for the phone…</div>
${picker}
<button id="copy" type="button">复制邀请 · Copy invitation</button><div id="copied" class="copied" aria-live="polite"></div>
<p class="hint">在手机 Pebrel 扫码即可连接。两台设备须位于同一受信任网络。 / Scan in Pebrel on your phone using the same trusted network.</p>
</main><script>
const invite = ${encoded};
const status = document.getElementById('status');
const copied = document.getElementById('copied');
const safeStatuses = ${scriptText([...SAFE_STATUSES])};
const errorStatuses = ${scriptText([...ERROR_STATUSES])};
const labels = {
  connecting: 'Connecting to the local relay… · 正在连接本机中转…',
  waiting_for_phone: 'Waiting for the phone… · 等待手机连接…',
  paired: 'Phone connected · 手机已连接',
  reconnecting: 'Reconnecting… · 正在重连…',
  connection_failed: 'Connection failed; check the network or firewall. · 连接失败，请检查网络或防火墙。',
  authentication_failed: 'Credentials rejected; import a fresh invitation. · 凭据被拒绝，请重新导入配对邀请。',
  server_unavailable: 'Relay unavailable; check the server. · 中转服务不可用，请检查服务器。',
  device_already_connected: 'Another phone is already connected. · 另一部手机已连接。',
  runtime_unavailable: 'Cannot read Pebrel sessions. Reopen Connect phone while Pebrel is running. · 无法读取 Pebrel 会话，请保持 Pebrel 运行后重新打开连接手机。',
  network_changed: 'Network changed. Scan this new QR code. · 已切换网络，请重新扫描此二维码。',
  network_change_failed: 'Could not switch the network. Check the interface and retry. · 无法切换网络，请检查所选接口后重试。',
  helper_error: 'Pairing helper reported an unexpected state. · 配对工具报告了未知状态。',
};
document.getElementById('copy').addEventListener('click', async () => { try { await navigator.clipboard.writeText(invite); copied.textContent = '已复制 · Copied'; } catch { copied.textContent = '复制失败，请重试 · Could not copy'; } });
let switching = false;
document.getElementById('network-form')?.addEventListener('submit', async event => {
  event.preventDefault();
  if (switching) return;
  switching = true;
  const button = document.getElementById('network-submit');
  button.disabled = true;
  status.textContent = '正在切换网络… · Switching network…';
  try {
    const response = await fetch('${route}/network', { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ address: document.getElementById('network').value }) });
    if (!response.ok) throw new Error('network_change_failed');
    location.reload();
  } catch {
    switching = false; button.disabled = false;
    status.textContent = labels.network_change_failed; status.dataset.state = 'error';
  }
});
async function refresh() { if (switching) return; try { const response = await fetch('${route}/status', { cache: 'no-store' }); if (!response.ok) return; const value = await response.json(); const key = safeStatuses.includes(value.status) ? value.status : 'helper_error'; status.textContent = labels[key]; status.dataset.state = key === 'paired' ? 'paired' : errorStatuses.includes(key) ? 'error' : ''; } catch { /* the helper may be closing */ } }
refresh(); setInterval(refresh, 1000);
</script></body></html>`;
}

/** Serve the QR page on IPv4 loopback only; no LAN interface can read its secret. */
export async function startQrPage(invitation, {
  host = '127.0.0.1', port = 0, addresses = [], currentAddress = '', onAddressChange,
} = {}) {
  if (host !== '127.0.0.1') throw new Error('qr_page_loopback_only');
  const route = `/pair/${randomBytes(18).toString('base64url')}`;
  const makeSvg = value => QRCode.toString(JSON.stringify(value), {
    type: 'svg', margin: 2, width: 360, errorCorrectionLevel: 'M',
    color: { dark: '#171827', light: '#f8f7ff' },
  });
  let svg = await makeSvg(invitation);
  let currentStatus = 'connecting';
  let options = normalizeNetworkOptions(addresses);
  let changing = false;
  const server = http.createServer(async (request, response) => {
    const listener = server.address();
    if (!listener) { response.writeHead(503); response.end(); return; }
    const origin = `http://127.0.0.1:${listener.port}`;
    response.setHeader('cache-control', 'no-store');
    response.setHeader('x-content-type-options', 'nosniff');
    if (!isLoopback(request.socket) || request.headers.host !== new URL(origin).host) {
      response.writeHead(403); response.end(); return;
    }
    let url;
    try { url = new URL(request.url ?? '/', origin); }
    catch { response.writeHead(400); response.end(); return; }
    if (request.method === 'POST' && url.pathname === `${route}/network`) {
      if (typeof onAddressChange !== 'function' || request.headers.origin !== origin ||
          request.headers['content-type']?.split(';')[0].trim() !== 'application/json') {
        response.writeHead(403); response.end(); return;
      }
      if (changing) { response.writeHead(409); response.end(); return; }
      changing = true;
      try {
        const body = JSON.parse(await readBoundedBody(request));
        if (!options.some(item => item.address === body.address)) throw new Error('invalid_network');
        const result = await onAddressChange(body.address);
        const nextSvg = await makeSvg(result.invitation);
        invitation = result.invitation;
        svg = nextSvg;
        currentAddress = result.address;
        options = normalizeNetworkOptions(result.addresses);
        currentStatus = 'network_changed';
        response.writeHead(204); response.end();
      } catch {
        currentStatus = 'network_change_failed';
        response.writeHead(400); response.end();
      } finally { changing = false; }
      return;
    }
    if (request.method !== 'GET' || (url.pathname !== route && url.pathname !== `${route}/status`)) {
      response.writeHead(404); response.end(); return;
    }
    if (url.pathname === `${route}/status`) {
      response.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
      response.end(JSON.stringify({ status: currentStatus })); return;
    }
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    response.end(pageMarkup(invitation, svg, route, options, currentAddress));
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
    server, route, url: `http://127.0.0.1:${actualPort}${route}`,
    setStatus(value) {
      const next = SAFE_STATUSES.has(value) ? value : 'helper_error';
      if (changing) return;
      if (['reconnecting', 'connecting', 'waiting_for_phone'].includes(next) && ERROR_STATUSES.has(currentStatus)) return;
      if (currentStatus === 'network_changed' && ['connecting', 'waiting_for_phone'].includes(next)) return;
      currentStatus = next;
    },
    close: async () => { await new Promise(resolve => server.close(() => resolve())); },
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
