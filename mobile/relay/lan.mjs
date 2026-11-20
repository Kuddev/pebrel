import os from 'node:os';
import net from 'node:net';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
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

const ROUTE_QUERY_TIMEOUT_MS = 5000;
const ROUTE_QUERY_MAX_OUTPUT = 16 * 1024;
const WINDOWS_DEFAULT_ADDRESS_COMMAND = [
  "$ErrorActionPreference='Stop';",
  "Get-NetIPConfiguration | Where-Object { $_.NetAdapter.Status -eq 'Up' -and $_.IPv4DefaultGateway -and $_.IPv4Address }",
  "| Sort-Object @{Expression={ if ($_.NetAdapter.HardwareInterface) { 0 } else { 1 } }}, @{Expression={$_.NetIPv4Interface.InterfaceMetric}}",
  "| ForEach-Object { $_.IPv4Address | ForEach-Object { $_.IPAddress } }",
].join(' ');

function isUsableIpv4(address) {
  return net.isIP(address) === 4 && !address.startsWith('169.254.');
}

function routeCommand(command, args) {
  try {
    return execFileSync(command, args, {
      encoding: 'utf8', maxBuffer: ROUTE_QUERY_MAX_OUTPUT, timeout: ROUTE_QUERY_TIMEOUT_MS,
      windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'],
    });
  } catch { return ''; }
}

function defaultRouteHints() {
  const addresses = new Set();
  const interfaces = new Set();
  if (process.platform === 'win32') {
    for (const line of routeCommand('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', WINDOWS_DEFAULT_ADDRESS_COMMAND]).split(/\r?\n/)) {
      const address = line.trim();
      if (isUsableIpv4(address)) addresses.add(address);
    }
  } else if (process.platform === 'darwin') {
    const output = routeCommand('route', ['-n', 'get', 'default']);
    const match = output.match(/^\s*interface:\s*(\S+)/m);
    if (match) interfaces.add(match[1]);
  } else {
    for (const line of routeCommand('ip', ['-4', 'route', 'show', 'default']).split(/\r?\n/)) {
      const source = line.match(/\bsrc\s+(\S+)/)?.[1];
      const device = line.match(/\bdev\s+(\S+)/)?.[1];
      if (isUsableIpv4(source)) addresses.add(source);
      if (device) interfaces.add(device);
    }
  }
  return { addresses, interfaces };
}

let routeCache;
function cachedRouteHints() {
  if (!routeCache || Date.now() - routeCache.at > 10_000) routeCache = { at: Date.now(), value: defaultRouteHints() };
  return routeCache.value;
}

/** Return non-loopback addresses suitable for a phone on the local network. */
export function listLanAddresses() {
  const routes = cachedRouteHints();
  const values = [];
  for (const [interfaceName, entries] of Object.entries(os.networkInterfaces())) {
    for (const entry of entries ?? []) {
      const address = entry.address?.split('%')[0];
      if (!address || entry.internal || !net.isIP(address) || address.startsWith('fe80:') ||
          (net.isIP(address) === 4 && !isUsableIpv4(address))) continue;
      values.push({ interfaceName, address, family: net.isIP(address),
        defaultRoute: routes.addresses.has(address) || routes.interfaces.has(interfaceName),
        routeOrder: routes.addresses.has(address) ? [...routes.addresses].indexOf(address) :
          routes.interfaces.has(interfaceName) ? [...routes.interfaces].indexOf(interfaceName) : 1000 });
    }
  }
  return values.sort((left, right) => Number(right.defaultRoute) - Number(left.defaultRoute) || left.routeOrder - right.routeOrder ||
    (left.family === 4 ? 0 : 1) - (right.family === 4 ? 0 : 1) ||
    left.interfaceName.localeCompare(right.interfaceName) || left.address.localeCompare(right.address));
}

export function chooseLanAddress(address) {
  if (address) {
    if (!validHost(address) || address === 'localhost' || address === '127.0.0.1' || address === '::1') throw new Error('invalid_lan_address');
    if (address.startsWith('fe80:')) throw new Error('lan_link_local_unsupported');
    return address;
  }
  const candidates = listLanAddresses();
  if (!candidates.length) throw new Error('no_lan_address');
  const preferredIpv4 = candidates.filter(candidate => candidate.defaultRoute && candidate.family === 4);
  if (preferredIpv4.length) return preferredIpv4[0].address;
  const preferred = candidates.filter(candidate => candidate.defaultRoute);
  if (preferred.length === 1) return preferred[0].address;
  const ipv4 = candidates.filter(candidate => candidate.family === 4);
  if (ipv4.length === 1) return ipv4[0].address;
  if (candidates.length === 1) return candidates[0].address;
  throw new Error('ambiguous_lan_address');
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

async function closeNetwork(network) {
  if (!network) return;
  network.connector?.close();
  await network.proxy?.close().catch(() => {});
  await network.relay?.close().catch(() => {});
}

/** Start a TLS LAN relay and attach the existing outbound runtime connector. */
export async function startLanPairing({ state, statePath, name, address, advertiseAddress, port, allowInput = false, onStatus } = {}) {
  let resolved = state || (statePath ? parseState(statePath) : null);
  const savedAddress = resolved?.bindAddress;
  const savedIsAvailable = savedAddress && (isWildcard(savedAddress) || !net.isIP(savedAddress) ||
    Object.values(os.networkInterfaces()).flat().some(entry => entry?.address?.split('%')[0] === savedAddress));
  const selectedAddress = chooseLanAddress(address || (savedIsAvailable ? savedAddress : undefined));
  const previousAdvertised = resolved?.advertiseAddress;
  const advertised = advertiseAddress || (previousAdvertised && previousAdvertised !== savedAddress ? previousAdvertised : null) ||
    (isWildcard(selectedAddress) ? chooseLanAddress() : selectedAddress);
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

  let networkGeneration = 0;
  const startNetwork = async ({ bindAddress, advertisedAddress, listenPort, certificate }) => {
    const generation = ++networkGeneration;
    const relay = createRelay({ devices: [{ id: resolved.device, desktopToken: resolved.desktopToken, mobileToken: resolved.mobileToken }] }, {
      tls: { key: certificate.key, cert: certificate.cert },
    });
    let proxy;
    try {
      await listen(relay.server, listenPort, bindAddress);
      const actualPort = relay.server.address().port;
      const proxyTarget = bindAddress === '::' ? '::1' : isWildcard(bindAddress) ? '127.0.0.1' : bindAddress;
      proxy = await startLoopbackProxy(proxyTarget, actualPort);
      const invitation = createInvitation({
        url: `wss://${urlHost(advertisedAddress)}:${actualPort}`,
        device: resolved.device,
        token: resolved.mobileToken,
        name: resolved.name,
        mode: 'lan',
        tlsPin: certificate.tlsPin,
      });
      const connector = connectDesktop({
        url: `wss://127.0.0.1:${proxy.port}`,
        device: resolved.device,
        token: resolved.desktopToken,
        name: resolved.name,
      }, {
        allowInput,
        allowLoopback: true,
        tlsCert: certificate.cert,
        tlsPin: certificate.tlsPin,
        onStatus: value => { if (generation === networkGeneration && !closed) onStatus?.(value); },
      });
      return { relay, proxy, connector, invitation, address: bindAddress,
        advertisedAddress, port: actualPort, certificate };
    } catch (error) {
      await proxy?.close().catch(() => {});
      await relay.close().catch(() => {});
      throw error;
    }
  };

  const followsBindAddress = isWildcard(selectedAddress) || advertised === selectedAddress;
  const certificate = { key: resolved.tlsKey, cert: resolved.tlsCert, tlsPin: resolved.tlsPin };
  let current;
  let closed = false;
  let changing = Promise.resolve();
  let result;

  current = await startNetwork({ bindAddress: selectedAddress, advertisedAddress: advertised,
    listenPort: resolved.port, certificate });
  resolved.port = current.port;
  try { if (statePath) writePrivateJson(statePath, resolved); }
  catch (error) { closed = true; await closeNetwork(current); throw error; }

  const snapshot = () => ({ invitation: result.invitation, state: result.state, address: result.address,
    advertisedAddress: result.advertisedAddress, port: result.port, addresses: listLanAddresses() });
  const updateResult = network => {
    result.state = resolved;
    result.invitation = network.invitation;
    result.relay = network.relay;
    result.proxy = network.proxy;
    result.connector = network.connector;
    result.address = network.address;
    result.advertisedAddress = network.advertisedAddress;
    result.port = network.port;
    result.addresses = listLanAddresses();
  };

  const switchAddressNow = async requestedAddress => {
    if (closed) throw new Error('lan_pairing_closed');
    const nextAddress = chooseLanAddress(requestedAddress);
    const previous = current ?? { address: resolved.bindAddress, advertisedAddress: resolved.advertiseAddress,
      port: resolved.port, certificate: { key: resolved.tlsKey, cert: resolved.tlsCert, tlsPin: resolved.tlsPin } };
    const nextAdvertised = followsBindAddress ? nextAddress : previous.advertisedAddress;
    if (current && nextAddress === current.address && nextAdvertised === current.advertisedAddress) return snapshot();
    const previousCertificate = previous.certificate;
    const nextCertificate = createLanCertificate({ address: nextAddress, advertiseAddress: nextAdvertised });
    await closeNetwork(previous);
    let next;
    try {
      next = await startNetwork({ bindAddress: nextAddress, advertisedAddress: nextAdvertised,
        listenPort: previous.port, certificate: nextCertificate });
      if (closed) throw new Error('lan_pairing_closed');
      const nextState = { ...resolved, bindAddress: nextAddress, advertiseAddress: nextAdvertised,
        port: next.port, tlsKey: nextCertificate.key, tlsCert: nextCertificate.cert, tlsPin: nextCertificate.tlsPin };
      if (statePath) writePrivateJson(statePath, nextState);
      Object.assign(resolved, nextState);
      current = next;
      updateResult(current);
      return snapshot();
    } catch (error) {
      await closeNetwork(next);
      if (closed) { current = null; throw error; }
      try {
        current = await startNetwork({ bindAddress: previous.address, advertisedAddress: previous.advertisedAddress,
          listenPort: previous.port, certificate: previousCertificate });
        updateResult(current);
      } catch { current = null; }
      throw error;
    }
  };

  const switchAddress = requestedAddress => {
    const operation = changing.then(() => switchAddressNow(requestedAddress));
    changing = operation.catch(() => {});
    return operation;
  };
  const close = async () => {
    closed = true;
    await changing.catch(() => {});
    const network = current;
    current = null;
    await closeNetwork(network);
  };

  result = {
    state: resolved,
    invitation: current.invitation,
    relay: current.relay,
    proxy: current.proxy,
    connector: current.connector,
    address: current.address,
    advertisedAddress: current.advertisedAddress,
    port: current.port,
    addresses: listLanAddresses(),
    switchAddress,
    close,
  };
  return result;
}

export { urlHost };
export const createLanRelay = startLanPairing;
