# Remote preview streams and writable document snapshots have separate lifetimes

## Status

Implemented. Native package and interaction validation limits accompany the candidate build.

## Context

The SSH file drawer sent every regular file to a text editor. Images consequently
appeared as binary text, and text only became visible after the entire bounded
read completed. Local image identity also consisted solely of a filesystem path.

## Evidence

- `workspace/documents.rs` now routes supported raster extensions to the image tab.
- `ssh_sftp/preview.rs` supplies coalesced progress and bounded immutable prefixes.
- `file_editor/loading.rs` separates provisional text from `LoadedDocument`.
- `doc_tabs/image_source.rs` owns the source, cancellation and decoder generation.
- `workspace/runtime_tabs.rs` distinguishes local files from remote image bytes.

## Decision

Read remote files on the existing authenticated SSH runtime in 64 KiB chunks.
Validate regular-file type, canonical target and metadata around the read.
Text retains the existing 8 MiB editing limit and 45 second deadline; larger text
remains a read-only prefix. Images accept at most 64 MiB encoded data, use a 120
second read deadline and cap decoded frames at 128 MiB. A shared two-slot
semaphore bounds concurrent image decoders on the existing background executor;
image construction does not require the scientific renderer's global assets.

A watch channel coalesces byte counts. Exponential prefix checkpoints limit
copying and repeated decoding. Text shows at most 256 KiB provisionally, trimming
incomplete UTF-8 tails. Image prefixes stop at 16 MiB and may be painted only if
the existing bounded decoder accepts them. A format requiring more data keeps
the progress indicator until a decodable frame arrives; this is not a guarantee
of progressive scan rendering for every format. GIF preview uses the first frame.

Each view owns the load task, progress subscription, cancellation and generation.
Completion and progress check their generation before applying. Closing a view
cancels its network read; a bounded decode already executing can finish without
publishing to a destroyed view. Reload clears the former writable snapshot.
Only a fully loaded, valid text snapshot enables editing and saving.

Remote document saves still use their captured authenticated SFTP session,
canonical target, byte/metadata preconditions and existing staged publication.
Cancellation does not abort a save during publication; rollback remains owned
by the shared transaction. BOM and line endings retain the shared text contract.

Image geometry receives dimensions from the decoded frame, with no filesystem
probe on the UI thread. Remote tab identity includes the SSH destination and
path. The Runtime reader reads the tab's immutable encoded bytes and generation,
never a remote path on the local disk; its independent 16 MiB budget is unchanged.

## Rejected alternatives

- Treating a remote image as text cannot provide an image preview.
- Downloading to a persistent cache before opening adds another copy/lifetime.
- Allowing edits to a partial buffer risks writing a prefix over the full file.
- Applying detached asynchronous results without a generation permits stale loads.
- Reconnecting a saved draft by alias could redirect the write after settings change.

## Consequences

No new runtime, daemon, codec dependency or persistent cache is added. Encoded
remote image bytes remain in memory while its tab is open. Partial image output
is provisional and never makes Runtime report a complete file. Read errors and
cancellation remain visible and retryable; drafts retain the existing save errors.

## Validation

Source and resource validation accompanies the implementation commit. The final
candidate uses the real Windows GPUI product build. Regression suites are not run
at the user's request; compilation does not establish native pointer, remote
permission, disconnect or every-codec visual acceptance.

## Supersedes

None. The existing decoded-frame geometry and remote publication contracts remain.

## Revisit when

Supporting another codec, progressive scan decoding, remote rich-document assets,
files beyond these budgets, or a shared source identity across independent tabs.
