import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { connectDesktop } from './connector.mjs';
import { startLanPairing, createLanState } from './lan.mjs';
import { openBrowser, startQrPage } from './qr-page.mjs';
import { createInvitation, parseInvitation, readInvitation, readJsonFile, writeInvitation, writePrivateJson,
  devicePattern, tokenPattern, normalizeWssUrl } from './invite.mjs';

function argument(args, name, fallback) {
  const index = args.indexOf(name);
  return index < 0 ? fallback : args[index + 1];
}

function hasArgument(args, name) {
  return args.includes(name);
}

function parseValue(value, label) {
  if (typeof value === 'object' && value !== null) return value;
  try { return JSON.parse(value); } catch { throw new Error(`invalid_${label}`); }
}

/** Normalize the desktop file emitted by init.mjs or a server device record. */
export function parseDesktopConfig(value, { device } = {}) {
  const data = parseValue(value, 'desktop_config');
  if (data.url && data.device && data.token) {
    if (!devicePattern.test(data.device) || !tokenPattern.test(data.token)) throw new Error('invalid_desktop_credentials');
    return {
      url: normalizeWssUrl(data.url), device: data.device, token: data.token,
      name: data.name || data.device, runtimeFile: data.runtimeFile,
      tlsPin: data.tlsPin, tlsCert: data.tlsCert,
    };
  }
  if (!Array.isArray(data.devices)) throw new Error('invalid_desktop_config');
  const selected = device ? data.devices.find(item => item?.id === device) : data.devices.length === 1 ? data.devices[0] : null;
  if (!selected || !selected.id || !selected.desktopToken) throw new Error('desktop_device_required');
  const relayUrl = data.url ?? data.relayUrl ?? data.serverUrl;
  if (!relayUrl) throw new Error('desktop_relay_url_required');
  if (!devicePattern.test(selected.id) || !tokenPattern.test(selected.desktopToken)) throw new Error('invalid_desktop_credentials');
  return {
    url: normalizeWssUrl(relayUrl), device: selected.id, token: selected.desktopToken,
    name: selected.name || data.name || selected.id, runtimeFile: data.runtimeFile,
  };
}

function adjacentInvitation(configPath, device) {
  if (!configPath || !device) return null;
  const target = path.join(path.dirname(path.resolve(configPath)), `phone-${device}.txt`);
  return existsSync(target) ? target : null;
}

function safeStatus(status, setStatus) {
  setStatus(status);
}

function portArgument(args, fallback) {
  const raw = argument(args, '--port');
  if (raw === undefined) return fallback;
  const value = Number(raw);
  if (!Number.isInteger(value) || value < 0 || value > 65535) throw new Error('invalid_lan_port');
  return value;
}

/** Start a cloud relay pairing page and the outbound desktop connector. */
export async function startRelayPairing({ config, invitation, allowInput = false, open = true } = {}) {
  const desktop = parseDesktopConfig(config);
  const phone = parseInvitation(invitation);
  if (desktop.device !== phone.device) throw new Error('pairing_device_mismatch');
  if (desktop.url !== phone.url) throw new Error('pairing_url_mismatch');
  if (phone.mode === 'lan') {
    if (!desktop.tlsPin || desktop.tlsPin !== phone.tlsPin) throw new Error('lan_connector_pin_required');
  }
  let page;
  try {
    page = await startQrPage(phone);
    const connector = connectDesktop(desktop, {
      allowInput,
      tlsPin: desktop.tlsPin ?? phone.tlsPin,
      tlsCert: desktop.tlsCert,
      onStatus: status => safeStatus(status, page.setStatus),
    });
    if (open) openBrowser(page.url);
    return {
      invitation: phone,
      connector,
      page,
      close: async () => { connector.close(); await page.close(); },
    };
  } catch (error) {
    if (page) await page.close().catch(() => {});
    throw error;
  }
}

/** Start the local TLS relay, attach the runtime connector and serve its QR. */
export async function startLanBrowserPairing({ state, statePath, name, address, advertiseAddress, port,
  allowInput = false, open = true } = {}) {
  let local;
  let page;
  let lastStatus = 'connecting';
  try {
    local = await startLanPairing({ state, statePath, name, address, advertiseAddress, port, allowInput,
      onStatus: status => { lastStatus = status; page?.setStatus(status); } });
    page = await startQrPage(local.invitation);
    page.setStatus(lastStatus);
    const originalClose = local.close;
    if (open) openBrowser(page.url);
    return {
      ...local,
      page,
      setStatus: page.setStatus,
      close: async () => { await page.close(); await originalClose(); },
    };
  } catch (error) {
    if (page) await page.close().catch(() => {});
    if (local) await local.close().catch(() => {});
    throw error;
  }
}

function usage() {
  return `Pebrel PC pairing helper

Relay mode:
  node pairing.mjs --config computer-DEVICE.json --invitation phone-DEVICE.txt [--allow-input]

LAN mode:
  node pairing.mjs --lan [--config private/lan-DEVICE.json] [--name NAME]
    [--address ADDRESS] [--advertise ADDRESS] [--port PORT] [--allow-input]

Options:
  --no-browser  Keep the loopback QR page available without opening a browser.
  --output DIR  Store generated LAN state and invitation in DIR (default: private).
  --device ID   Select a device when --config points at relay.config.json.
`;
}

function readDesktopSource(file) {
  if (!file) throw new Error('desktop_config_required');
  return readJsonFile(file);
}

function buildLanState(args, configPath, outputDirectory) {
  if (configPath && existsSync(configPath)) {
    const data = readJsonFile(configPath);
    if (data.mode === 'lan') return { state: data, statePath: configPath };
    if (Array.isArray(data.devices)) {
      const device = argument(args, '--device');
      const selected = device ? data.devices.find(item => item?.id === device) : data.devices.length === 1 ? data.devices[0] : null;
      if (!selected) throw new Error('desktop_device_required');
      const state = createLanState({
        name: argument(args, '--name', selected.name || data.name || 'Pebrel PC'),
        device: selected.id, desktopToken: selected.desktopToken, mobileToken: selected.mobileToken,
        address: argument(args, '--address'), advertiseAddress: argument(args, '--advertise'),
        port: portArgument(args, 0),
      });
      const statePath = path.join(outputDirectory, `lan-${state.device}.json`);
      return { state, statePath };
    }
    throw new Error('invalid_lan_state');
  }
  const state = createLanState({
    name: argument(args, '--name', 'Pebrel PC'),
    address: argument(args, '--address'), advertiseAddress: argument(args, '--advertise'),
    port: portArgument(args, 0),
  });
  const statePath = path.join(outputDirectory, `lan-${state.device}.json`);
  return { state, statePath };
}

export async function runCli(args = process.argv.slice(2)) {
  if (hasArgument(args, '--help') || !args.length) { console.log(usage()); return; }
  const allowInput = hasArgument(args, '--allow-input');
  const open = !hasArgument(args, '--no-browser');
  const outputDirectory = path.resolve(argument(args, '--output', 'private'));
  if (hasArgument(args, '--lan') || argument(args, '--mode') === 'lan') {
    const configPath = argument(args, '--config') ?? argument(args, '--lan-config') ?? argument(args, '--relay-config');
    const { state, statePath } = buildLanState(args, configPath, outputDirectory);
    let page;
    let lastStatus = 'connecting';
    const local = await startLanPairing({
      state, statePath,
      name: argument(args, '--name'), address: argument(args, '--address'),
      advertiseAddress: argument(args, '--advertise'),
      port: portArgument(args, state.port ?? 0), allowInput,
      onStatus: status => { lastStatus = status; page?.setStatus(status); },
    });
    page = await startQrPage(local.invitation);
    page.setStatus(lastStatus);
    const invitationPath = path.join(outputDirectory, `phone-${local.state.device}.txt`);
    writeInvitation(invitationPath, local.invitation);
    if (open) openBrowser(page.url);
    console.log(`Pebrel LAN pairing ready for ${local.state.name}`);
    console.log(`Scan the QR in your browser at ${page.url}`);
    console.log(`Invitation saved to ${invitationPath}`);
    await new Promise(resolve => {
      const stop = () => resolve();
      for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, stop);
    });
    await page.close();
    await local.close();
    return;
  }
  const configPath = argument(args, '--config') ?? argument(args, '--desktop-config') ?? argument(args, '--relay-config');
  const invitationPath = argument(args, '--invitation') ?? argument(args, '--phone') ?? argument(args, '--phone-invitation') ??
    adjacentInvitation(configPath, configPath && readJsonFile(configPath).device);
  if (!configPath || !invitationPath) throw new Error('desktop_config_and_invitation_required');
  const pairing = await startRelayPairing({
    config: readDesktopSource(configPath), invitation: readInvitation(invitationPath), allowInput, open,
  });
  console.log(`Pebrel pairing ready for ${pairing.invitation.name}`);
  console.log(`Scan the QR in your browser at ${pairing.page.url}`);
  console.log('Status updates are shown here and in the browser.');
  await new Promise(resolve => {
    const stop = () => resolve();
    for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, stop);
  });
  await pairing.close();
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCli().catch(error => { console.error(`Pairing failed: ${error.message}`); process.exitCode = 1; });
}

export { createInvitation, parseInvitation, readInvitation, writeInvitation, writePrivateJson,
  startLanPairing, createLanState, startQrPage, openBrowser };
