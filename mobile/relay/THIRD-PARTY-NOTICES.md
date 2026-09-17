# Third-party notices

The PC pairing helper uses the following pinned npm packages. The complete
source archives and integrity values are recorded in `package-lock.json`; the
packages are installed locally with `npm ci --omit=dev --ignore-scripts` and
the browser page does not load a script, stylesheet or image from the network.

## qrcode 1.5.4

`qrcode` is MIT licensed. It generates the invitation SVG in the Node process,
so the QR shown by the loopback page remains available when the computer has no
internet connection. The helper uses its public `toString(..., { type: 'svg' })`
API and does not ship an external CDN URL.

## selfsigned 2.4.1

`selfsigned` is MIT licensed. It creates the short lived RSA certificate used
only by the optional LAN listener. The helper includes SAN entries for the
advertised address and local connector addresses, computes the SPKI pin from
the resulting X.509 certificate, and never disables TLS verification.

`selfsigned` depends on `node-forge` 1.3.1, which is distributed under the
BSD-3-Clause OR GPL-2.0 license. No `node-forge` source is copied into Pebrel;
its pinned npm archive remains the licensing and integrity record.

## ws 8.18.3

`ws` is MIT licensed and is the existing bounded WebSocket implementation used
by the relay and desktop connector.
