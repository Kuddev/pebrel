# Reader presentation language

## Status

Proposed for [#322](https://github.com/Kuddev/pebrel/issues/322).

## Context

The answer reader displayed Chinese controls and notices regardless of the selected
language. Document preparation also inserted Chinese text for omitted images and
empty captions, while image loading returned already formatted errors. Those stored
strings could not follow a later language change without rebuilding the document.

## Evidence

The [document preparation code](../../../../nebula_app/src/assistant_answer/document.rs)
owns image replacement positions and load limits. The
[reader](../../../../nebula_app/src/gpui_shell/terminal/answer_reader.rs) owns the
immutable snapshot and the Copy source action. Translation must therefore happen at
presentation, while generated markers must still be distinguished from source fences.
The [native captures](../../../../docs/answer-reader-language.md) show the same answer
under English and Chinese controls, including a rejected network image.

## Decision

Keep the captured answer immutable. Reader controls and answer notices resolve typed
message identifiers through the existing language setting. Image errors retain their
failure category until presentation; decoder diagnostics remain literal arguments.

Document preparation emits validated image placeholders for both loadable and omitted
images. One ordered position list identifies generated placeholders; only the bounded
image list enters the decode queue. The renderer supplies translated omission notices
and default captions. Source-provided captions remain unchanged.

## Rejected alternatives

- Translating or replacing text in the captured answer would corrupt Copy source.
- Rebuilding the whole reader on a language change would repeat work and lose state.
- Looking up errors by their Chinese text would make presentation depend on wording.
- Treating arbitrary matching code fences as image markers would bypass provenance checks.

## Consequences

The original source, image count and byte limits, path containment checks, sequential
decode queue, and result revision guards keep their existing authority. The internal
reader-document interface carries placeholder positions rather than presentation text.
No stored session, hook payload, remote protocol or language registry changes.

## Validation

Tests cover generated versus forged markers, omitted-image limits, sanitized localized
path failures, and Copy source through real controls across languages and reader modes.
The 25 answer model/document tests and 4 reader tests pass. Native Windows acceptance
at 192 DPI covers English and Chinese labels and layout. Existing Markdown/math,
file-access and answer ownership tests remain applicable.

## Supersedes

None.

## Revisit when

The document model gains another generated element type or image-loading policy changes.
Preserve the distinction between captured content and application presentation.
