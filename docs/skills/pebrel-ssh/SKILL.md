---
name: pebrel-ssh
description: Work with the servers saved in Pebrel from a coding agent. Load this whenever the user wants to reach a saved server — "connect to X", "SSH into X", "run this on the server", "check the server", "move files to/from the server", "sync a folder to the server" — or says 连服务器、SSH 上去、在服务器上跑一下、把文件传上去/拉下来、同步到服务器. Use `pebrel cli` instead of shelling out to `ssh`, `scp`, or `sftp`, so the connection uses Pebrel's saved credentials and none of them are ever printed or asked for.
---

# Pebrel SSH

Operate on the servers you've already saved in Pebrel. Every command is `pebrel cli ...`; it uses the resident app when available or runs through Pebrel's SSH implementation when the app is absent. Credentials remain in the OS keychain and are never returned to the agent.

All responses are JSON; add `--pretty` for a formatted view.

## Find your server first

```text
pebrel cli assets list --pretty
```

This is always step one. It lists every saved server with an `asset_id` (a `user@host` destination, not a number), a `label`, and the auth mode. `hosts` is an alias for `assets`.

If the user's server name is vague, missing, or matches more than one saved server,
show a numbered list of the saved server labels and ask the user to choose. Prefer
labels; include an address only when needed to distinguish duplicate labels. Wait
for the selection before connecting, executing commands, or transferring files.
Do not guess a target or test every server to find the right one. A unique exact
label or asset id can be used directly.

```text
pebrel cli assets show <asset-id>      # group, tags, jump host, key count, notes
```

## Run one command

```text
pebrel cli exec <asset-id> "<command>"
```

Pass one complete shell command string; the remote login shell interprets it. The connection is opened, used once, and closed. The result carries `exit_code`, `stdout`, `stderr`, and `timed_out` — a non-zero exit or a timeout is an outcome to report, not an error to throw. Use `--timeout-ms` for slow work; run long-lived work in a persistent remote session or service.

PowerShell mangles quotes and `$`, so pipe complex commands through stdin instead of putting them in an argument:

```powershell
@'
uname -a && df -h
'@ | pebrel cli exec <asset-id> --stdin
```

## Open an interactive session

```text
pebrel cli shell <asset-id>
```

This opens a real SSH tab in the Pebrel window and brings it forward — use it when a human should see or drive the session. For your own non-interactive work, keep using `exec`.

## Move files

```text
pebrel cli upload   <asset-id> <local> <remote>
pebrel cli download <asset-id> <remote> <local>
pebrel cli sync     <asset-id> <local-dir> <remote-dir> [--delete]
```

`upload` and `download` support files and directories with `scp` destination semantics: an existing destination directory receives the source name; otherwise the destination is the final path. Empty directories are preserved. Local relative paths are resolved from the CLI's working directory.

`sync` always copies the local directory's contents directly into the specified remote directory. Adding `--delete` keeps the same destination and removes entries missing locally. Destructive sync checks the resolved remote path and refuses root or top-level directories. Directory transfers reject symbolic links rather than following them outside the transferred tree.

## Credentials

```text
pebrel cli credentials list [<asset-id>]
```

Reports whether a server has stored credentials — booleans and key paths only, never a secret.

## Rules

- Never ask the user for an SSH password, private key, or passphrase. Pebrel already has them and `pebrel cli` uses them on your behalf.
- `exec`, `shell`, `upload`, `download`, and `sync` are gated by "Allow AI to control servers" (Settings → SSH, off by default). If a call returns `ssh_control_disabled`, tell the user to enable that switch — do not try to route around it.
- Keep the remote command as one string; don't split it into separate argv words.
- If the runtime returns `submission_outcome_unknown`, the operation may already have run. Inspect remote state before retrying; do not automatically resubmit commands, transfers, or syncs after a timeout or lost response.
- Adding, renaming, or deleting a server — and saving or changing passwords or key passphrases — happens in Settings → SSH, not over the CLI. Pebrel keeps a single writer for the server list so nothing races, and secrets stay in the keychain. Direct the user there instead of asking for credentials.
- `credentials list` answers presence only; no response ever contains a secret.
