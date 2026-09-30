import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { devicePattern, tokenPattern } from './protocol.mjs';
import { validateTlsPin } from './tls.mjs';

export const MAX_INVITATION_BYTES = 8192;
export { devicePattern, tokenPattern };

function privatePath(file) {
  const target = path.resolve(file);
  mkdirSync(path.dirname(target), { recursive: true, mode: 0o700 });
  return target;
}

function tightenMode(file) {
  try { chmodSync(file, 0o600); } catch { /* Windows ACLs provide the effective mode. */ }
}

export function writePrivateJson(file, value) {
  const target = privatePath(file);
  writeFileSync(target, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  tightenMode(target);
  return target;
}

export function readJsonFile(file) {
  return JSON.parse(readFileSync(file, 'utf8'));
}

function cleanName(value, fallback) {
  const text = String(value ?? fallback).trim();
  if (!text || /\p{Cc}/u.test(text)) throw new Error('invalid_name');
  return Array.from(text).slice(0, 80).join('');
}

export function normalizeWssUrl(value) {
  let url;
  try { url = new URL(value); } catch { throw new Error('invalid_invitation_url'); }
  if (url.protocol !== 'wss:' || url.username || url.password || url.search || url.hash ||
      !url.hostname || !['', '/'].includes(url.pathname)) throw new Error('invalid_invitation_url');
  url.pathname = '/';
  return url.toString().replace(/\/$/, '');
}

function parseObject(value) {
  if (typeof value === 'string' || Buffer.isBuffer(value)) {
    const text = value.toString();
    if (Buffer.byteLength(text, 'utf8') > MAX_INVITATION_BYTES) throw new Error('invitation_too_large');
    try { value = JSON.parse(text); } catch { throw new Error('invalid_invitation_json'); }
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid_invitation');
  return value;
}

/** Create the v1 payload scanned by Android. Secrets stay in the returned value and file only. */
export function createInvitation({ url, device, token, name, mode, tlsPin } = {}) {
  if (!devicePattern.test(device ?? '') || !tokenPattern.test(token ?? '')) throw new Error('invalid_invitation_credentials');
  const normalizedUrl = normalizeWssUrl(url);
  const resolvedMode = mode ?? (tlsPin ? 'lan' : 'relay');
  if (!['relay', 'lan'].includes(resolvedMode)) throw new Error('invalid_invitation_mode');
  if (resolvedMode === 'lan' && !tlsPin) throw new Error('lan_tls_pin_required');
  if (tlsPin) validateTlsPin(tlsPin);
  const invitation = {
    version: 1,
    url: normalizedUrl,
    device,
    token,
    name: cleanName(name, device),
  };
  if (resolvedMode === 'lan') invitation.mode = 'lan';
  if (tlsPin) invitation.tlsPin = tlsPin;
  if (Buffer.byteLength(JSON.stringify(invitation), 'utf8') > MAX_INVITATION_BYTES) throw new Error('invitation_too_large');
  return invitation;
}

/** Parse an existing phone invitation while retaining only the v1 contract fields. */
export function parseInvitation(value) {
  const data = parseObject(value);
  if (data.version !== 1) throw new Error('unsupported_invitation_version');
  const mode = data.mode ?? 'relay';
  if (!['relay', 'lan'].includes(mode)) throw new Error('invalid_invitation_mode');
  const tlsPin = data.tlsPin;
  if (tlsPin) validateTlsPin(tlsPin);
  if (mode === 'lan' && !tlsPin) throw new Error('lan_tls_pin_required');
  return createInvitation({
    url: data.url,
    device: data.device,
    token: data.token,
    name: data.name,
    mode,
    tlsPin,
  });
}

export function readInvitation(file) {
  return parseInvitation(readFileSync(file, 'utf8'));
}

export function writeInvitation(file, invitation) {
  const normalized = parseInvitation(invitation);
  const target = privatePath(file);
  writeFileSync(target, JSON.stringify(normalized), { flag: 'w', mode: 0o600 });
  tightenMode(target);
  return target;
}
