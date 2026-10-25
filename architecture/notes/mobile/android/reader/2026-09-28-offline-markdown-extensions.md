# Offline mathematical and Mermaid rendering

## Status
Implemented; parser tests and LDPlayer formula/diagram rendering verified.

## Context
File and Agent readers share CommonMark and a restricted WebView. Math needs to
be protected before Markdown consumes underscores and escapes. Runtime CDN loads
would break offline reading and expand the document's access beyond local assets.

## Evidence
- `ReaderMath.kt` uses CommonMark 0.30's actual inline/block extension interfaces.
- `ReaderDocument.kt` escapes document HTML and preserves copy targets separately.
- `reader/extensions-vendor.json` records package integrity and asset hashes.
- LDPlayer WebView 91 rejected Mermaid's `class { static {} }` at parse time.
  The same browser reported missing `Object.hasOwn` and `structuredClone` APIs.

## Decision
- Bundle KaTeX 0.18.9 and Mermaid 11.17.2 with licenses and local font assets.
- Use esbuild 0.28.2 for syntax lowering and selected core-js 3.50.0 polyfills.
  Explicitly lower class static blocks: this WebView contradicted the compiler's
  version-based support inference. Keep the complete Mermaid implementation.
- Lock the reproducible build inputs under `third_party/reader-build`.
- Keep network/file access disabled and an explicit asset allowlist/CSP.
  KaTeX uses `trust=false`; Mermaid uses strict security and bounded text/edges.
- Render diagrams serially, ignore results belonging to removed documents, and
  bound the SVG cache. Keep source available when parsing/rendering fails.
- Use dedicated code-surface tokens. A theme's general document container may
  intentionally equal its background, which is unsuitable as the code-block fill.

## Rejected alternatives
- Post-processing arbitrary rendered Markdown text for math: loses original
  escapes, treats code/currency as formulas and corrupts copy semantics.
- Downgrading Mermaid to avoid a browser syntax error: gives up library fixes
  rather than adapting the known deployment target.
- Loading remote libraries or granting documents local filesystem access.

## Consequences
The APK includes bounded extra JS/font resources. Source copies preserve the
CommonMark source text, whose line endings are normalized by that parser.
Math and diagrams still require a functioning system WebView; one emulator is
not evidence of universal WebView compatibility.

## Validation
Existing reader tests cover escaped HTML, math/code/currency separation, exact
copy data and Mermaid source/CSP. Real LDPlayer screenshots show inline/display
math, a flowchart and a sequence diagram with Chinese labels. The code-surface
regression has a targeted theme test; visual acceptance is recorded separately.

## Supersedes
None.

## Revisit when
The supported WebView baseline changes or upstream syntax/API support makes the
explicit compatibility transforms unnecessary.
