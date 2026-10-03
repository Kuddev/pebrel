# Terminal bypass lists stay with the selected proxy

## Status

Implemented; pending review.

## Context

New local terminals receive the selected proxy URL in `http_proxy`, `https_proxy`,
and `all_proxy`. The saved custom bypass list and the Windows `ProxyOverride`
value were already applied to SSH connections, but the child environment did
not receive `no_proxy`. A parent process could therefore keep bypassing hosts
the selected proxy is supposed to cover, or a terminal could send bypassed
names through the proxy.

The address box also persisted two values the user did not finish choosing.
A field that contained only a scheme was saved as an empty URL and cleared.
`socks5h://` was stored as `socks5://` because the protocol list had no
separate entry. SSH treats those two schemes as the same handshake, but curl
and git do not: `socks5h` asks the proxy to resolve the name, and `socks5`
resolves it locally.

## Evidence

curl documents `NO_PROXY` as a comma-separated list, with a leading dot for a
domain suffix and CIDR for address blocks
(https://curl.se/docs/manpage.html). WinINET `ProxyOverride` uses semicolons,
`<local>`, and trailing `*` forms such as `192.168.*` and `*.example.com`.
`SshProxyConfig::resolve` already understands those spellings for SSH. They
are not the curl spelling.

`socks5h` keeps hostname resolution on the proxy. Rewriting it to `socks5`
changes which machine performs DNS for every terminal client, even though the
SSH parser can keep treating both as a SOCKS5 server.

## Decision

When a proxy URL is actually exported, also export `no_proxy`. Custom mode
uses `ssh_proxy_no_proxy`. System mode uses the bypass list from the same
`system_proxy()` read as the URL. Translate `<local>` to `localhost`,
`*.suffix` to `.suffix`, and an IPv4 prefix wildcard (`10.*`, `192.168.*`,
`a.b.c.*`) to the matching CIDR. Join with commas and drop case-insensitive
duplicates. An empty bypass list writes an empty `no_proxy` so an inherited
value cannot keep bypassing. Off mode, and any selection that exports no proxy
URL, leaves `no_proxy` untouched. `no_proxy` is added to `WSLENV` only when
this export set it. `PEBREL_HTTP_PROXY` stays out of `WSLENV`.

`socks5h` is its own manual-protocol choice and round-trips as `socks5h://`.
`socks://` remains an alias of `socks5://`. A typed string that is only a
recognized scheme stays in the address box and is not persisted.

The PowerShell adapter still uses `PEBREL_HTTP_PROXY` only. `no_proxy` does
not change its `WebProxy` bypass list.

## Rejected alternatives

- Rewriting SOCKS to HTTP so `Invoke-WebRequest` follows the bypass list.
  The selected scheme is the terminal contract; PowerShell HTTP support stays
  on the existing marker.
- Putting `no_proxy` in `TERMINAL_PROXY_VARIABLES`, which would forward the
  name on every WSL handoff even when no proxy is injected.
- Mapping `socks5h` onto the SOCKS5 dropdown. The next address edit would
  persist `socks5://` and drop remote DNS.
- Clearing the address box when the typed text is only a scheme. The user is
  still entering the host.
- Teaching the PowerShell script to read `no_proxy`. That script is verified
  for HTTP proxy URLs only.

## Consequences

SSH bypass matching is unchanged and still sees WinINET's own spellings.
Terminal clients that honor `no_proxy` see the curl spelling. A prefix
wildcard that is not an IPv4 pattern is copied through unchanged. `https://`
is still not a proxy scheme. An explicit empty address still clears a saved
URL.

## Validation

`network_proxy_model` tests cover a scheme-only value, a `socks5h://`
round-trip, and a later host-only edit. `ssh_proxy` tests cover the custom
bypass export, an empty bypass clearing `NO_PROXY`, off mode leaving the
environment alone, and `WSLENV` listing `no_proxy` without
`PEBREL_HTTP_PROXY`.

## Supersedes

None. The scheme marker in
[`2026-09-29-terminal-proxy-scheme.md`](2026-09-29-terminal-proxy-scheme.md)
still decides when `PEBREL_HTTP_PROXY` is set.

## Revisit when

The managed PowerShell adapter learns a verified way to apply `no_proxy`, or
another proxy scheme is added to the address box.
