# Native result block presentation

## Status

Implemented locally. Native interaction and parser regressions pass. Desktop
visual acceptance and release/platform validation remain separate.

## Context

The approved terminal layout uses natural table columns, UI-font headings,
numbered code blocks and transient controls. Component defaults center button
labels, stretch Markdown tables and provide neither code line numbers nor the
required heading font. Replacing text with unselectable labels would lose
existing reading behavior.

## Evidence

The pinned component supports Markdown block extensions and selectable HTML/text
views. Its button activation requires both key down and key up; its mouse click
deliberately does not transfer keyboard focus. GPUI keyed state exists only while
its element is rendered in consecutive frames. A retained result can leave that
window without losing its source or user-selected code language.

## Decision

Keep the result as the owner of code entities, language choices and copy feedback.
Eviction releases them with the source. Parse table samples in the existing owned
background task; keep scroll handles and font measurements on the UI thread.
Cache measurements by text/font/width and materialize only visible data rows and
code line numbers. The terminal continues to own vertical scrolling.

Compile custom headings/tables in their original Markdown context before passing
inline content to selectable HTML views. Fragments such as `# title` inside a
table remain text. Blocks requiring full-document references or scientific inline
rendering retain the existing document renderer. Raw HTML/media remains literal.
Code reconstruction uses a sufficiently long tilde fence: a language name with a
backtick cannot invalidate it and turn code text into a fetchable image.

Only keys owned by the result container are stopped. Child inputs retain native
character/IME processing; the terminal's existing focus check prevents PTY input.
The filter Escape path clears its query and returns focus to the result.

## Rejected alternatives

- Per-frame extensions or transient code entities: repeat parsing or reset choices.
- Reparse isolated inline fragments as block Markdown: changes their meaning.
- Rewrite shared component internals for terminal-only presentation: unnecessarily
  changes unrelated document and settings views.
- Stop every key at the parent: Windows then skips character-message translation.

## Consequences

Code and structured tables retain original copy content. Custom presentation has
bounded state per retained result and additional cold layout work. Synthetic debug
scroll timing is evidence about that fixture, not a release GPU performance claim.

## Validation

Existing native tests cover column ordering, real filter typing, Tab/Enter release,
Escape, copy feedback, raw payloads, viewport retention and PTY isolation. Added
regressions cover unusual code-language fences, inline table semantics and code
entity retention across an alternate-screen round trip. Native checks passed 20
tests with one existing manual timing sample ignored; independent i18n checks pass.

## Supersedes

None. Applies the existing native-result retention and single-viewport decisions.

## Revisit when

The component exposes equivalent semantic block styling or measured document
workloads justify replacing the custom presentation while retaining these behaviors.
