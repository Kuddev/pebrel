# Local interactive shell colors without rewriting user configuration

## Status

Implemented in the local PTY startup adapter.

## Context

The local PTY already advertised xterm-256color/truecolor, but Android's system
`ls` did not emit colors by default. Changing the terminal renderer would not fix
an uncolored byte stream.

## Evidence

The device's Toybox `ls --help` supports `--color=auto`, with colors for Unix file
types. `SessionTransport.kt` owns local startup and `pty.cpp` supplies execve's
environment; the terminal core already decodes ANSI colors.

## Decision

Generate an app-owned startup file inside private app storage on the IO worker.
Replace it atomically and pass its path through `ENV` to the interactive system
shell. Enable `ls --color=auto`, then source an existing user `.mkshrc` so explicit
user preferences retain precedence. Do not overwrite the user's startup files.

## Rejected alternatives

- Force `--color=always`: contaminates pipes and redirected file output.
- Add colors to rendered text by guessing filenames: changes terminal semantics.
- Rewrite `$HOME/.mkshrc`: takes ownership of the user's configuration.
- Advertise GNU `LS_COLORS` extension rules: this device's Toybox implements Unix
  file-type colors rather than that GNU configuration contract.

## Consequences

Directories, links and executables are distinguishable in interactive listings;
plain files retain their normal color. This is separate from dedicated file-type
icons in the graphical browser. Explicit shell aliases may override the default.

## Validation

`GhosttyEngineTest` runs the real local PTY and inspects rendered colors for a
directory, link and executable, then checks redirected listing bytes contain no ANSI.

## Supersedes

None.

## Revisit when

The local shell or bundled command utilities change their startup/color contract.
