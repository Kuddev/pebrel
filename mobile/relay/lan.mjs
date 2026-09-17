import os from 'node:os';
import net from 'node:net';
import path from 'node:path';
import { randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import selfsigned from 'selfsigned';
import { createRelay } from './server.mjs';
import { connectDesktop } from './connector.mjs';
import { startLoopbackProxy } from './loopback-proxy.mjs';
import { createInvitation, writePrivateJson, devicePattern, tokenPattern } from './invite.mjs';
import { spkiPin } from './tls.mjs';

function isWildcard(address) {
  return address === '0.0.0.0' || address === '::' || address === '';
}

function validHost(address) {
  return typeof address === 'string' && address.length <= 253 &&
    (Boolean(net.isIP(address)) || /^(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?$/.test(address));
}

function urlHost(address) {
  return net.isIP(address) === 6 ? `[${address}]` : address;
}

/** Return non-loopback addresses suitable for a phone on the local network. */
export function listLanAddresses() {
  const values = [];
  for (const [interfaceName, entries] of Object.entries(os.networkInterfaces())) {
    for (const entry of entries ?? []) {
      const address = entry.address?.split('%')[0];
      if (!address || entry.internal || !net.isIP(address) || address.startsWith('fe80:')) continue;
      values.push({ interfaceName, address, family: net.isIP(address) });
    }
  }
  return values.sort((left, right) => (left.family === 4 ? 0 : 1) - (right.family === 4 ? 0 : 1) ||
    left.interfaceName.localeCompare(right.interfaceName) || left.address.localeCompare(right.address));
}

export function chooseLanAddress(address) {
  if (address) {
    if (!validHost(address) || address === 'localhost' || address === '127.0.0.1' || address === '::1') throw new Error('invalid_lan_address');
    if (address.startsWith('fe80:')) throw new Error('lan_link_local_unsupported');
    return address;
  }
  const selected = listLanAddresses()[0];
  if (!selected) throw new Error('no_lan_address');
  return selected.address;
}

function subjectAlternativeNames(address, advertiseAddress) {
  const values = new Map([
    ['localhost', { type: 2, value: 'localhost' }],
    ['127.0.0.1', { type: 7, ip: '127.0.0.1' }],
    ['::1', { type: 7, ip: '::1' }],
  ]);
  for (const candidate of [address, advertiseAddress]) {
    if (!candidate || isWildcard(candidate)) continue;
    const key = candidate.toLowerCase();
    values.set(key, net.isIP(candidate) ? { type: 7, ip: candidate } : { type: 2, value: candidate });
  }
  return [...values.values()];
}

/** Generate a self-signed server certificate with SANs for both connector and phone. */
export function createLanCertificate({ address, advertiseAddress = address } = {}) {
  const commonName = advertiseAddress || address || 'Pebrel LAN';
  const attributes = [{ name: 'commonName', value: commonName }];
  const extensions = [
    { name: 'basicConstraints', cA: false },
    { name: 'keyUsage', digitalSignature: true, keyEncipherment: true },
    { name: 'extKeyUsage', serverAuth: true },
    { name: 'subjectAltName', altNames: subjectAlternativeNames(address, advertiseAddress) },
  ];
  const pems = selfsigned.generate(attributes, {
    algorithm: 'sha256', keySize: 2048, days: 825,
    notBeforeDate: new Date(Date.now() - 60_000), extensions,
  });
  return { key: pems.private, cert: pems.cert, tlsPin: spkiPin(pems.cert) };
}

function generatedState({ name, address, advertiseAddress, port, device, desktopToken, mobileToken } = {}) {
  const selectedAddress = chooseLanAddress(address);
  const advertised = advertiseAddress || (isWildcard(selectedAddress) ? chooseLanAddress() : selectedAddress);
  if (!validHost(advertised) || advertised.toLowerCase().startsWith('fe80:')) throw new Error('invalid_advertise_address');
  const certificate = createLanCertificate({ address: selectedAddress, advertiseAddress: advertised });
  const state = {
    version: 1,
    mode: 'lan',
    name: name || 'Pebrel PC',
    device: device || randomBytes(12).toString('hex'),
    desktopToken: desktopToken || randomBytes(32).toString('base64url'),
    mobileToken: mobileToken || randomBytes(32).toString('base64url'),
    bindAddress: selectedAddress,
    advertiseAddress: advertised,
    port: Number.isInteger(port) ? port : 0,
    tlsKey: certificate.key,
    tlsCert: certificate.cert,
    tlsPin: certificate.tlsPin,
  };
  if (!devicePattern.test(state.device) || !tokenPattern.test(state.desktopToken) || !tokenPattern.test(state.mobileToken) ||
      state.desktopToken === state.mobileToken) throw new Error('invalid_lan_credentials');
  return state;
}

export function createLanState(options = {}) {
  return generatedState(options);
}

function stateNeedsCertificate(state, address, advertiseAddress) {
  return !state.tlsKey || !state.tlsCert || !state.tlsPin || state.bindAddress !== address || state.advertiseAddress !== advertiseAddress;
}

function listen(server, port, address) {
  return new Promise((resolve, reject) => {
    const fail = error => { server.off('listening', resolve); reject(error); };
    server.once('error', fail);
    server.listen(port, address, () => { server.off('error', fail); resolve(); });
  });
}

function parseState(file) {
  const state = JSON.parse(readFileSync(file, 'utf8'));
  if (state?.mode !== 'lan') throw new Error('invalid_lan_state');
  return state;
}

/** Start a TLS LAN relay and attach the existing outbound runtime connector. */
export async function startLanPairing({ state, statePath, name, address, advertiseAddress, port, allowInput = false, onStatus } = {}) {
  let resolved = state || (statePath ? parseState(statePath) : null);
  const selectedAddress = chooseLanAddress(address || resolved?.bindAddress);
  const advertised = advertiseAddress || resolved?.advertiseAddress || (isWildcard(selectedAddress) ? chooseLanAddress() : selectedAddress);
  if (!validHost(advertised) || advertised.toLowerCase().startsWith('fe80:')) throw new Error('invalid_advertise_address');
  if (!resolved) resolved = generatedState({ name, address: selectedAddress, advertiseAddress: advertised, port });
  if (stateNeedsCertificate(resolved, selectedAddress, advertised)) {
    const certificate = createLanCertificate({ address: selectedAddress, advertiseAddress: advertised });
    Object.assign(resolved, { tlsKey: certificate.key, tlsCert: certificate.cert, tlsPin: certificate.tlsPin }, {
      bindAddress: selectedAddress, advertiseAddress: advertised,
    });
  }
  if (spkiPin(resolved.tlsCert) !== resolved.tlsPin) throw new Error('lan_certificate_pin_mismatch');
  resolved.name = name || resolved.name || 'Pebrel PC';
  if (port !== undefined && (!Number.isInteger(port) || port < 0 || port > 65535)) throw new Error('invalid_lan_port');
  resolved.port = port !== undefined ? port : (Number.isInteger(resolved.port) ? resolved.port : 0);
  if (!Number.isInteger(resolved.port) || resolved.port < 0 || resolved.port > 65535) throw new Error('invalid_lan_port');
  if (!resolved.device || !resolved.desktopToken || !resolved.mobileToken) throw new Error('invalid_lan_credentials');
  const relay = createRelay({ devices: [{ id: resolved.device, desktopToken: resolved.desktopToken, mobileToken: resolved.mobileToken }] }, {
    tls: { key: resolved.tlsKey, cert: resolved.tlsCert },
  });
  let proxy;
  try {
    await listen(relay.server, resolved.port, selectedAddress);
    const actualPort = relay.server.address().port;
    resolved.port = actualPort;
    const proxyTarget = selectedAddress === '::' ? '::1' : isWildcard(selectedAddress) ? '127.0.0.1' : selectedAddress;
    proxy = await startLoopbackProxy(proxyTarget, actualPort);
    const invitation = createInvitation({
      url: `wss://${urlHost(advertised)}:${actualPort}`,
      device: resolved.device,
      token: resolved.mobileToken,
      name: resolved.name,
      mode: 'lan',
      tlsPin: resolved.tlsPin,
    });
    const connector = connectDesktop({
      url: `wss://127.0.0.1:${proxy.port}`,
      device: resolved.device,
      token: resolved.desktopToken,
      name: resolved.name,
    }, {
      allowInput,
      allowLoopback: true,
      tlsCert: resolved.tlsCert,
      tlsPin: resolved.tlsPin,
      onStatus,
    });
    if (statePath) writePrivateJson(statePath, resolved);
    return {
      state: resolved,
      invitation,
      relay,
      proxy,
      connector,
      address: selectedAddress,
      advertisedAddress: advertised,
      port: actualPort,
      close: async () => { connector.close(); await proxy.close(); await relay.close(); },
    };
  } catch (error) {
    await proxy?.close().catch(() => {});
    await relay.close().catch(() => {});
    throw error;
  }
}

export { urlHost };
export const createLanRelay = startLanPairing;
