# Connection onboarding completion

## Status
Implemented; device acceptance remains separate from automated UI checks.

## Context
The first-launch connection choice offers a computer, an SSH host, a local
terminal, and an explicit skip. Returning from a cancelled connection form must
not discard the choice page, and skipping must not show it again on every launch.

## Evidence
`MainActivity.Workspace` owns navigation and existing connection forms.
`session/DisplayPreferences.kt` owns small persisted UI preferences.
`ui/HomeSessionsTest.kt` exercises entry actions and preference reloads.

## Decision
Store one boolean, `connection_onboarding_completed`, in the existing UI
preferences. Mark completion after a valid computer invitation enters its
connection flow, after an SSH host is saved, after local-terminal creation, or
when the user explicitly skips. Cancelling forms or storage permission leaves
the choice page active. This flag means the introduction was completed, not
that a network connection succeeded.

## Rejected alternatives
Inferring completion from an empty host/session list would repeatedly interrupt
users who deliberately skipped. A new onboarding store or navigation framework
would duplicate existing ownership for a single preference.

## Consequences
The new flag defaults to false. Existing session, credential and terminal
preference formats are unchanged. Home creation actions reuse the same flows.

## Validation
The existing `HomeSessionsTest` covers the four home actions, three onboarding
choices, explicit skip, large-text scrolling and persisted completion.
Actual pairing, SSH authentication and remote deployment remain their existing
flows and require their own live endpoints for end-to-end acceptance.

## Supersedes
None.

## Revisit when
Onboarding gains resumable multi-step setup or a user-requested replay flow.
