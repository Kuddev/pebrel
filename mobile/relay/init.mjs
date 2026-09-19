import { randomBytes } from 'node:crypto';
import { mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import QRCode from 'qrcode';

const urlIndex = process.argv.indexOf('--url');
const nameIndex = process.argv.indexOf('--name');
const outputIndex = process.argv.indexOf('--output');
if (urlIndex < 0) throw new Error('Usage: node init.mjs --url wss://relay.example.com --name MyPC --output private');
const url = new URL(process.argv[urlIndex + 1]);
if (url.protocol !== 'wss:' || url.port || url.username || url.password || url.search || url.hash || !['/', ''].includes(url.pathname)) throw new Error('Use wss://your-domain on port 443 with no path or credentials');
const directory = path.resolve(outputIndex < 0 ? 'private' : process.argv[outputIndex + 1]);
const name = nameIndex < 0 ? 'Pebrel PC' : process.argv[nameIndex + 1];
const id = randomBytes(12).toString('hex');
const device = { id, desktopToken: randomBytes(32).toString('base64url'), mobileToken: randomBytes(32).toString('base64url') };
mkdirSync(directory, { recursive: true, mode: 0o700 });
const serverFile = path.join(directory, 'relay.config.json');
const server = existsSync(serverFile) ? JSON.parse(readFileSync(serverFile, 'utf8')) : { devices: [] };
if (!Array.isArray(server.devices) || server.devices.length >= 64) throw new Error('configure_at_most_64_devices');
server.devices.push(device);
writeFileSync(serverFile, JSON.stringify(server, null, 2) + '\n', { mode: 0o600 });
writeFileSync(path.join(directory, `computer-${id}.json`), JSON.stringify({ url: url.origin.replace('https:', 'wss:'), device: id, token: device.desktopToken, name }, null, 2), { flag: 'wx', mode: 0o600 });
const invitation = { version: 1, url: url.toString().replace(/\/$/, ''), device: id, token: device.mobileToken, name };
const invitationText = JSON.stringify(invitation);
writeFileSync(path.join(directory, `phone-${id}.txt`), invitationText, { flag: 'wx', mode: 0o600 });
const invitationQr = await QRCode.toString(invitationText, {
  type: 'svg', errorCorrectionLevel: 'M', margin: 2, width: 768,
});
writeFileSync(path.join(directory, `phone-${id}.svg`), invitationQr, { flag: 'wx', mode: 0o600 });
const envFile = path.resolve('.env');
if (!existsSync(envFile)) {
  const uid = process.getuid?.() ?? 1000;
  const gid = process.getgid?.() ?? 1000;
  writeFileSync(envFile, `PEBREL_RELAY_DOMAIN=${url.host}\nPEBREL_RELAY_UID=${uid}\nPEBREL_RELAY_GID=${gid}\n`, { flag: 'wx', mode: 0o600 });
}
console.log(`Created relay.config.json, computer-${id}.json, phone-${id}.txt and phone-${id}.svg in ${directory}`);
console.log('Keep these credentials private. Scan the phone SVG or import the phone text in Android; copy only the computer file to the desktop.');
