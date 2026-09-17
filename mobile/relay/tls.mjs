import { createHash, timingSafeEqual, X509Certificate } from 'node:crypto';
import { checkServerIdentity as defaultCheckServerIdentity } from 'node:tls';

const PIN_PREFIX = 'sha256/';
const PIN_BYTES = 32;
const PIN_TEXT = /^[A-Za-z0-9+/]{43}=$/;

/** Validate the invitation's canonical SHA-256/SPKI pin representation. */
export function validateTlsPin(value) {
  if (typeof value !== 'string' || !value.startsWith(PIN_PREFIX)) throw new Error('invalid_tls_pin');
  const encoded = value.slice(PIN_PREFIX.length);
  if (!PIN_TEXT.test(encoded)) throw new Error('invalid_tls_pin');
  let bytes;
  try { bytes = Buffer.from(encoded, 'base64'); } catch { throw new Error('invalid_tls_pin'); }
  if (bytes.length !== PIN_BYTES || bytes.toString('base64') !== encoded) throw new Error('invalid_tls_pin');
  return value;
}

function certificateObject(certificate) {
  if (certificate instanceof X509Certificate) return certificate;
  if (certificate?.raw) return new X509Certificate(certificate.raw);
  if (Buffer.isBuffer(certificate) || typeof certificate === 'string') return new X509Certificate(certificate);
  throw new Error('certificate_unavailable');
}

export function certificateSpki(certificate) {
  return certificateObject(certificate).publicKey.export({ type: 'spki', format: 'der' });
}

/** Return the canonical pin for a PEM/DER X.509 certificate. */
export function spkiPin(certificate) {
  return `${PIN_PREFIX}${createHash('sha256').update(certificateSpki(certificate)).digest('base64')}`;
}

/** Compare a peer certificate's SPKI with an exact invitation pin. */
export function matchesSpkiPin(pin, certificate) {
  validateTlsPin(pin);
  const expected = Buffer.from(pin.slice(PIN_PREFIX.length), 'base64');
  const actual = createHash('sha256').update(certificateSpki(certificate)).digest();
  return expected.length === actual.length && timingSafeEqual(expected, actual);
}

/**
 * Keep Node's normal hostname check and add the exact SPKI check used by LAN
 * invitations. Returning an Error lets Node abort the TLS handshake normally.
 */
export function pinnedServerIdentity(pin) {
  validateTlsPin(pin);
  return (hostname, certificate) => {
    const hostnameError = defaultCheckServerIdentity(hostname, certificate);
    if (hostnameError) return hostnameError;
    try {
      if (!matchesSpkiPin(pin, certificate)) return new Error('tls_pin_mismatch');
    } catch { return new Error('tls_pin_mismatch'); }
    return undefined;
  };
}

/** Build only the TLS options the connector is allowed to use. */
export function pinnedTlsOptions({ pin, certificate } = {}) {
  const options = {};
  if (certificate && !pin) throw new Error('tls_pin_required_for_custom_certificate');
  if (certificate) options.ca = certificate;
  if (pin) {
    validateTlsPin(pin);
    options.rejectUnauthorized = true;
    options.checkServerIdentity = pinnedServerIdentity(pin);
  }
  return options;
}
