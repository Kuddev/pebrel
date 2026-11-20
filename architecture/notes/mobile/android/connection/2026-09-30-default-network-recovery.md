# Recover desktop links when Android changes its default network

## Status
Accepted for implementation.

## Context
A connected desktop can appear ready after Wi-Fi changes to another network.
The old socket may take a long time to fail. An existing delayed retry also
should not delay recovery after a new default route becomes available.

## Evidence
`DesktopReconnect` already owns foreground retry scheduling and backoff.
`SessionRepository.desktopFailed` removes a client by identity, so late callbacks
cannot remove a replacement. Pairing approvals and certificate failures have
separate states that must survive recovery.

## Decision
Observe the default network only while the application is foregrounded. Compare
the route again on resume, since callbacks are unregistered in the background.
Use the existing failure/reconnect path to retire eligible old connections and
replace pending backoff with one immediate attempt. Keep socket disposal on IO.
Recent inbound traffic suppresses a redundant resume probe; idle connections
retain the existing five-second probe allowance.

## Rejected alternatives
Waiting for TCP timeout leaves a stale ready state. Replacing all connections
would restart pairing approval and retry certificate/authentication failures.
Reducing every probe timeout to 1.5 seconds would penalize slow but live links
without evidence that such a deadline is appropriate.

## Consequences
Only previously connected relay/LAN desktops are recovered automatically.
Drafts and computer identity remain; terminal input is never replayed. This
does not add automatic SSH terminal reconnection or background polling.

## Validation
Existing Android retry and repository recovery tests cover immediate replacement,
old callbacks, certificate failures, background pause and preservation of drafts.
Device Wi-Fi/cellular switching remains a manual acceptance requirement.

## Supersedes
None.

## Revisit when
The transport can safely migrate a live connection between Android networks.
