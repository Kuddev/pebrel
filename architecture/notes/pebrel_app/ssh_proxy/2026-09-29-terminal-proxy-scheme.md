# Terminal proxy environment keeps the selected scheme

## Status

Implemented; pending review.

## Context

New Windows local terminals receive `http_proxy`, `https_proxy`, and `all_proxy` from the
saved network settings. A first version rewrote `socks5://host:port` to
`http://host:port` for the first two variables so Windows PowerShell's
`WebRequest` could use a mixed listener. That assumption is not true of every
SOCKS endpoint.

## Evidence

curl uses a protocol-specific proxy variable ahead of `ALL_PROXY`
(https://curl.se/docs/manpage.html). An HTTP URL in `http_proxy` therefore
sends HTTP proxy requests to a SOCKS-only port, and the original scheme left
in `all_proxy` does not correct it. The managed PowerShell adapter is verified
with HTTP URLs; other URL schemes remain available through the environment for
clients whose own networking stack supports them.

## Decision

Write the validated URL, with its original scheme, into all three variables.
Set `PEBREL_HTTP_PROXY` to that URL only when it is HTTP, for both a custom
address and an enabled system proxy. A selected SOCKS endpoint writes an empty
marker to shadow an inherited HTTP marker even in an incremental environment.
The PowerShell startup script consumes that marker and does not read
`ssh_proxy_url`. Jump and command proxies stay out of the environment because
Rust already rejects them. The Windows adapter preserves inherited WSLENV entries
only for incremental environments; complete registry snapshots remain authoritative.
Do not add `PEBREL_HTTP_PROXY` to `WSLENV`.

## Rejected alternatives

- Rewriting SOCKS to HTTP on the same host and port. A mixed listener may
  accept both, but a SOCKS-only listener will not.
- Leaving the original scheme only in `all_proxy`. curl and git prefer
  `http_proxy` / `https_proxy` when those are set.
- Parsing `ssh_proxy_url` again in the PowerShell script. That path accepts
  forms the environment exporter already refuses, and it missed the system
  proxy that Rust had already selected.

## Consequences

A SOCKS setting reaches curl and git as SOCKS. The PowerShell initialization
overrides request defaults only for an HTTP URL; it leaves other protocols to
the client's networking stack. An already open terminal does not change.
HTTP URL credentials are decoded into native proxy
credentials; they are removed from the URL passed to the request cmdlets. Proxy
initialization uses a local script scope and does not print credential-bearing
URLs when initialization fails.

## Validation

`terminal_proxy_assignments` and `apply_terminal_proxy_env` cover scheme
preservation, the HTTP marker, IPv6 brackets, credential bytes, and removal of
a stale marker. The Windows prompt script test requires `PEBREL_HTTP_PROXY`
and forbids reading the raw proxy settings. The production `proxy.ps1` fragment is
also executed by Windows PowerShell 5 and PowerShell 7 tests against a loopback
HTTP proxy, covering anonymous requests, a Basic authentication challenge and an
empty marker with a SOCKS environment.

## Supersedes

None.

## Revisit when

The managed adapter gains verified support for another proxy scheme or the
native request clients change their proxy configuration contracts.
