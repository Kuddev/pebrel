# Architecture decisions / 架构决策记录

## Process

Record decisions that change dependency direction, core ownership, persistent
interfaces, threading/lifetime, performance contracts or governance itself. Do not
require a new record for each typo or ordinary bug fix. Records are reviewable;
an accepted decision may be superseded when evidence changes.

Use: **Status; Context; Evidence; Decision; Alternatives; Consequences; Validation;
Revisit condition.** Identify the accountable maintainer in the PR review. A policy
change must include a failing legitimate example when correcting a false positive,
plus a violation that must remain rejected. Do not use a policy edit to conceal an
unrelated feature's growth. Remote approval/enforcement is not implied by this log.

## ADR-0001 — Evidence-based architecture contracts

- **Status:** Adopted in the working-tree implementation, 2026-09-05; pending normal
  repository review and server-side activation. Initial owner entry: `@Kuddev`.
- **Context:** Multiple rendering adapters share behavior; a large contribution
  volume needs stable boundaries. Existing source-size documentation was ignored
  by Git and the old scanner could inspect local build/probe artifacts.
- **Evidence:** [Primary-source review](engineering-evidence.md), workspace manifests,
  feature-selected module aliases, and positive/negative checker fixtures.
- **Decision:** Keep the modular application and 2000-line hard / 800-line advisory
  limits. Share a precise legacy inventory, enforce production crate directions,
  and verify pure i18n independently. Review module cohesion and lifecycle manually.
- **Alternatives rejected:** An abrupt 800-line hard limit (65 additional oversized
  legacy files); import-name blacklists (incorrect under `#[path]`/cfg); mandatory
  microservices/traits; machine-specific nanosecond CI thresholds.
- **Consequences:** Ordinary PRs cannot silently expand debt. Large legacy changes
  may require a responsibility extraction. Valid source-layout or policy changes
  can require an explicit governance update and new fixtures, not a skip flag.
- **Validation:** The checker suite covers legitimate and forbidden dependencies,
  base ratcheting, paths, parsing and scan errors. CI runs those tests before using
  the checker. Real product compilation and human review remain separate evidence.
- **Revisit condition:** A reproducible legitimate change is rejected, the source
  layout changes, or measured review cost outweighs a threshold's benefit. Correct
  the narrow rule with tests; do not treat this ADR as immutable proof of quality.

## ADR-0002 — Static extensible UI translation

- **Status:** Implemented in the working tree, 2026-09-05; not a new release claim.
- **Context:** Shared language choices and translations must not add parsing,
  locking or string allocation to ordinary UI text lookup.
- **Decision:** One registry, build-time validated catalogs, typed static lookups,
  English fallback for partial locales, and a separate parameter-formatting path.
- **Consequences:** Adding a language is a registry/catalog change and requires a
  build. Initial coverage is partial; live downloadable language packs and complete
  locale-aware formatting are not promised.
- **Validation:** Compile production lookup/generator code in the isolated contract
  workspace; test invalid catalogs, fallback, locale matching and zero allocations.
- **Revisit condition:** A real requirement for runtime packs, richer plural/date
  formatting or RTL layouts warrants a separately measured design. See the
  [internationalization contract](internationalization.md).

## ADR-0003 - Pebrel 1.6 identity migration

- **Status:** Requested by the maintainer in this working session, 2026-09-07;
  implementation and native package verification in progress.
- **Context:** Display branding alone left users with a Nebula installation
  directory, executable, command and configuration files. Renaming those interfaces
  also affects upgrades, stored credentials and managed integrations.
- **Decision:** Ship `pebrel.exe`, `pebrel-hook.exe`, the `pebrel` command and Pebrel
  configuration names. Keep the existing Inno AppId to identify the same product.
  Migrate a registered installation whose final directory component is
  `Nebula Terminal` to the sibling `Pebrel` directory. Preserve other custom
  directory names and explicit installer directory choices. Only remove known
  installer-owned legacy files; preserve unknown files. Copy legacy configuration
  into the new data directory without overwriting newer files or deleting the
  source, so absolute imports into the old directory remain valid. Serialize
  migration with an exclusive lock, publish copied files atomically, and record
  success only after the copy completes; the success marker prevents subsequent
  launches from restoring files the user deliberately removed. New configuration
  takes precedence over legacy data. Migration failures must be visible and
  retryable. Read old credential and integration identifiers as compatibility
  inputs, while writing new names.
- **Repository:** The existing repository was renamed to `Kuddev/pebrel` on
  2026-09-07, retaining repository ID `1289958986`. GitHub redirects the old
  repository and Git clone/fetch/push URLs. Keep `Kuddev/nebula` unused so that
  creating a new repository at that path cannot take over the redirects. GitHub
  Pages and callers of an action through the old repository name need separate
  migration; this repository had no Pages site or action manifest at verification.
- **Boundaries:** Historical release notes, release assets, upstream attribution,
  source directory names and library crate identifiers are not rewritten as if
  the old releases had different names. New user-facing artifacts and
  documentation use Pebrel. Existing Runtime API and hook protocol names remain
  stable for clients already using them.
- **Release compatibility:** Old clients select an exact `NebulaTerminal-...` asset
  name. Version 1.6.0 supplied that alias with identical installer bytes. On
  2026-09-12, the maintainer explicitly retired this alias starting with 1.7.0.
  The published 1.6.0 update resolver already prefers the Pebrel filename;
  clients that require the old name must download the current installer from the
  Release page. Current CI publishes only the Pebrel installer, and the shared
  manifest rejects an old-name extra asset from 1.7.0 onward. Historical manifests
  still require their original assets and byte identity, covered by positive and
  negative release-helper tests. This does not retire configuration or protocol
  compatibility readers. Repository redirects do not create aliases for renamed
  asset filenames; keep existing published assets and tags intact.
- **Validation:** The installer migration passed 77 isolated native fixture
  checks and a complete Inno Setup syntax build on 2026-09-07. This covers owned
  files, custom directories, shortcuts, PATH, locks and retry behavior; it does
  not stand in for upgrading the user's real installation. Targeted terminal/SSH,
  config migration and update selection tests, architecture contracts, and a
  fresh Windows GPUI ZIP/installer build are the remaining release checks.
  Package tests must use isolated user state. The SSH startup regression checks
  device-attributes delivery; live Helix behavior still needs user acceptance.
- **Revisit condition:** Remove compatibility readers only after support for old
  clients and persisted configurations is explicitly retired.

## ADR-0004 - Current conversation identity at workspace save

- **Status:** Implemented for the maintainer-reported restore defect, 2026-09-07;
  validation is part of the 1.6.0 release candidate checks.
- **Context:** A Codex pane can identify its foreground program without receiving
  a session ID from the CLI hook. Saving only the program restores the tab but
  cannot construct an exact conversation resume command.
- **Decision:** Keep hook identities authoritative. For WSL and Linux, match the
  process environment to both the Pebrel instance and pane, then read only the
  first metadata record of open Codex rollout files. Accept one main conversation
  whose filename and metadata agree. Do not choose by working directory or time.
  Native Windows and macOS keep the existing hook path.
- **Lifecycle:** Queries use the existing background executor, a two-second time
  limit and bounded output. Results carry the foreground command generation.
  Window close and the application Quit action allow up to three seconds before
  saving and stopping panes; the UI thread remains responsive. A refreshed
  inferred identity is required at close. Hook identities are not replaced.
- **Persistence:** Reuse the existing optional `source` and `session_id` fields;
  there is no new file format or dependency. A missing or ambiguous ID cannot
  launch a different conversation as a fallback. Existing autosave and the final
  operating-system shutdown callback save the identities already available.
- **Validation:** File save/load and exact resume commands, primary-thread
  metadata selection, WSL user arguments, pane/instance isolation, output limits,
  stale-result rejection, and a subprocess deadline have focused regressions.
- **Revisit condition:** Replace the procfs fallback when a supported CLI session
  identity API covers these launches consistently.

## ADR-0005 - Molecular diagrams and file hover previews

- **Status:** Requested in the current working session, 2026-09-08; implementation
  and validation in progress, pending normal review.
- **Context:** Markdown and AI CLI output need the same SMILES interpretation.
  File tree previews must not decode large images or PDFs during row rendering.
- **Decision:** Keep SMILES parsing and SVG depiction in one presentation-independent
  application module, using exact `chematic-smiles` / `chematic-depict` 1.0.9 pins
  (MIT OR Apache-2.0). Use the existing GPUI SVG image pipeline. Disable the
  depiction crate's optional PNG/PDF backends; they add unrelated renderers.
  Bound input, molecule size and cache capacity, perform layout in background work,
  and preserve source text on invalid or unsupported input. UI adapters own their
  pending work; discarded views cannot receive stale results.
- **Preview boundary:** Native Windows PDF first-page rendering lives under
  `platform`, through Windows.Data.Pdf using the already-resolved `windows` 0.61.3
  family. Image decoding and thumbnail caching share a bounded application adapter.
  Cache identity includes path, modification time and length; no preview changes
  the user's file or creates a persistent format.
- **Alternatives:** Handwritten SMILES geometry would duplicate specialized ring
  and stereochemistry rules. A Python/Node subprocess or a web service would add
  deployment or availability requirements. A whole PDF viewer is outside hover
  preview responsibility.
- **Validation:** Regression fixtures cover common rings, branches, charges and
  invalid SMILES; unsupported stereo notation preserves source because the pinned
  depiction backend does not project atom parity / alkene direction. Preview checks
  cover dimension bounds, cache invalidation and
  real first-page rendering. Real product compilation and UI inspection are
  reported separately from these behavior tests.
- **Revisit condition:** Remove or replace an adapter when upstream support covers
  the same behavior, or measured parsing/rendering cost exceeds the bounded use.

## ADR-0006 — Remote file workflows and persistent host organization

- **Status:** Implemented in the working tree on 2026-09-08 for the maintainer's
  requested workflows. Native compilation and focused regression checks passed;
  full GPUI/Explorer/server acceptance and normal maintainer review remain separate.
- **Context:** Upload drops only target the currently visible directory; Windows
  has no outbound remote-file drag. Remote files cannot enter the editable document
  lifecycle. The saved-host MRU also serves as the long-term host list.
- **Decision:** Keep transfer policy and I/O in the existing SSH/SFTP capability.
  A gesture captures the source and destination identities; asynchronous directory
  checks and native dialogs cannot retarget it when the focused pane changes.
  Windows outbound drag uses a scoped native OLE adapter and virtual file streams;
  its worker owns the drag, materialization, cancellation and temporary resources.
  The GPUI thread never waits for file contents. Reuse the already pinned Windows
  bindings rather than adding another native framework or changing the GPUI fork.
  The worker associates its input queue with the captured source window thread
  for the native gesture, then detaches it. File descriptors are prepared lazily,
  and initialization rechecks the physical button state before entering OLE.
  The direct `windows-core` 0.61.2 edge is required by the official COM implement
  macro and shares the version already resolved by `windows` 0.61.3.
- **Documents:** Share text decoding, BOM/newline handling and conditional-save
  semantics between local and remote documents. The editor owns a fixed document
  source, dirty buffer and in-flight revision. SFTP reads and writes run on the
  existing network runtime, use bounded text snapshots and preserve drafts on
  failure. Source conflicts and unsupported replacement semantics remain visible.
- **Hosts:** Existing profiles remain the authority for explicitly managed hosts;
  recent connections remain bounded separately. Optional organization metadata is
  backward compatible, excludes credentials, and survives profile edits/renames.
  UI search and grouping consume cached profile data, indexing profiles once per
  filter pass instead of scanning every profile for every candidate. Import/export must validate
  before changing persisted state and must never serialize private credentials.
  Profile saves reuse the existing OS-handle lock and atomic state writer, and
  reject a snapshot that differs from the file last loaded by that writer.
- **Alternatives:** A second SSH transport, renderer-specific persistence engine,
  synchronous download in a drag callback, or enlarged MRU alone would preserve
  the workflow gaps or duplicate existing contracts.
- **Validation:** Windows GPUI product compilation, architecture, i18n, settings
  and line-budget checks passed. Production-source regressions cover destination
  identity, cancellation, names, encoding, host retention and import. Eight remote
  save cases use the real SFTP codec over an injected peer, including permission
  failure, publication rollback and competing writes. A separately invoked desktop
  OLE test copies a Unicode filename and its contents between owned test windows.
  These tests do not exercise real Explorer, GPUI gestures or SSH authentication.
  Full-workspace formatting still reports differences in other working-tree
  changes; the files modified for these workflows have no remaining format findings.
- **Revisit condition:** Replace the OLE adapter when the pinned GPUI exposes an
  equivalent Windows virtual-file drag with the same ownership contract. Revisit
  persistent host identity before adding shared/cloud mutation or credential export.

## 中文说明

记录重大取舍而非每次小修复；事实与测试能推翻旧决定。规范误伤、安全修复与旧预算冲突时，
先记录问题和最小修订，维护者审查后更新合同；不能将“只减不增”变成拒绝纠正规范的理由。

## ADR-0007 — Editable documents, reusable layouts and pane drop targets

- **Status:** Implemented in the working tree, 2026-09-08, for the requested editor,
  recipe and quick-terminal workflows; pending normal maintainer review.
- **Context:** The code viewer accepted edits without a save operation. Markdown
  had a separate read-only lifecycle. Dragging a tab into a split workspace always
  split the root. The quick window used fixed monitor dimensions on creation.
- **Decision:** A GPUI file editor owns its input buffer, dirty state and scoped
  asynchronous loads/saves. One renderer-independent document snapshot implements
  UTF-8/BOM/newline handling, external-change checks and temporary-file replacement.
  Partial/invalid previews cannot be saved. Preview image URL rewriting never
  changes the source buffer. Markdown headings and source positions come from the
  existing Markdown parser. Root blocks remain intact and receive reference
  definitions when rendered as individually navigable preview blocks.
- **Layout authority:** Recipes wrap the existing session schema with a name and
  format version, stored under the application settings directory. They reuse
  `restore_tab` with AI resume disabled and exclude agent identity from stored
  trees. They save terminal layout/launch identity, not command history or output.
  Reads and writes belong to the recipe view's background tasks. Restoring creates
  a separate regular window. Bounds, count and version validation precede restore.
- **Split authority:** `nebula_split::SplitTree::dock_at_leaf` owns subtree grafting.
  Local/cross-window gestures carry a stable destination pane ID and direction;
  preview rectangles use the same tree layout math as the resulting split.
- **Preferences:** `nebula_settings` owns the quick-window mode and optional logical
  dimensions. The window registry samples only normal, settled window bounds,
  saves changes through the existing preference writer and clamps restored sizes
  to the current display. Animation positions do not become saved dimensions.
  New built-in terminal/chrome palette data also lives in the shared settings crate.
- **Alternatives:** A second editor persistence implementation, Markdown source
  modification for navigation, root-only docking, or replaying command history
  from a layout would add inconsistent state or change the requested semantics.
- **Validation:** Regression coverage targets conditional saves, BOM/CRLF and
  Unicode, truncation, heading positions/references, recipe round trips, malformed
  preferences, palette contrast and grafting a fifth pane without resizing the
  three unrelated panes. Native product checks and actual UI results are reported
  separately; this record does not assert release or service-side enforcement.
- **Revisit condition:** Add command replay or externally imported recipes only
  with a deliberate replay contract. Replace the preview block adapter when the
  pinned TextView exposes an equivalent public heading/navigation API.

## ADR-0008 — Formula bitmap admission and preview view lifetime

- **Status:** Implemented for the maintainer's memory reduction request,
  2026-09-09; validation and normal review tracked separately.
- **Context:** The shared 48 MiB scientific cache bounded completed resources,
  but two workers could allocate formula bitmaps before cache eviction. Reading
  created lazy preview views that remained retained after switching to source.
- **Decision:** Reuse bitmap geometry preflight for both worker admission and
  allocation. Reserve formula output bytes against the existing cache allowance
  before starting a worker; shrink the LRU allowance while reservations live and
  release reservations on success/failure. Compose alpha directly in the final
  BGRA allocation. Source mode releases parsed preview views while preserving
  the outline, source, scroll position and editor undo state.
- **Boundaries:** This does not change worker count, persistence, terminal
  identity or dependencies. It does not cap the entire process: compiler/glyph
  scratch, GPU copies, queued inputs, other images and references outside the
  cache remain separate costs. Molecular rendering is currently disabled.
- **Alternatives:** Evicting only after allocation retains the peak; shrinking
  the cache constant alone does not reserve in-flight output. Evicting arbitrary
  actively selected Markdown blocks would lose selection state.
- **Validation:** Regression tests cover fractional-DPI preflight/allocation,
  alpha overlap, reservations exceeding available bytes, failure release,
  cold-entry eviction and source/preview transitions. Native product checks and
  any measured memory reduction are reported separately.
- **Revisit condition:** Extend reservations to other resource types when they
  are enabled/profiled; add viewport eviction only with preserved selection and
  measured parsed-view accounting. No 50 MB process-wide guarantee is implied.

## ADR-0009 — Optional in-app AI message toasts

- **Status:** Requested by the maintainer, 2026-09-11; implemented in the working
  tree, with native validation pending.
- **Decision:** Add the default-on `ai_toasts` preference to `nebula_settings` and
  its existing persistence/reset contracts. The GPUI adapter caches it with other
  runtime settings. Disabling it hides only in-app AI completion/confirmation
  cards; native system notifications, tab indicators and terminal state retain
  their existing behavior. Source identity reuses the shared agent registry.
- **Lifetime:** Existing cards use the component notification identity and
  dismissal lifecycle. A settings observer dismisses only AI cards across open
  windows; deferred startup delivery rechecks the preference. No second queue,
  background service, dependency or per-event settings-file read is introduced.
- **Validation:** Regression coverage includes defaults, parsing, round trips,
  reset, independent delivery channels, search, and component-card dismissal.
  Native compilation and UI results must be reported separately.
- **Revisit condition:** Add separate system-notification or per-agent controls
  only when requested, rather than expanding the meaning of this persisted key.

## ADR-0010 — Administrator shells hosted by the GPUI product

- **Status:** Requested by the maintainer and implemented, 2026-09-13. Native
  Windows behavior tests passed; interactive UAC acceptance remains manual.
- **Context:** Elevating a shell executable directly opens an external console.
  Elevating Pebrel without preserving the explicit startup command can instead
  reach the ordinary resident process and lose both privilege and Shell selection.
- **Decision:** The launcher requests Windows UAC for the current Pebrel executable
  on a worker. It passes the selected Shell's argument vector and working directory
  through the existing CLI, using Windows argument quoting without a command shell.
  GPUI startup consumes that command as its first terminal. An already elevated
  process creates the terminal in its current workspace.
- **Ownership:** A process-token check separates elevated instances from ordinary
  resident forwarding. Elevated instances use the existing loopback API transport
  with a private endpoint supplied only in local PTY child environments, including
  WSL passthrough. They do not publish a privileged bearer token in `runtime.port`,
  acquire its ownership lock, restore/write the shared session, or hide on close.
  Ordinary discovery files and settings formats remain compatible. Token-query
  failure uses the isolated policy; launch failure remains visible to the user.
- **Lifetime and cost:** UAC runs off the UI thread, one request per workspace at
  a time. Completion updates only a still-live workspace and does not dismiss a
  subsequently opened picker. Token status is cached; endpoint injection runs only
  during PTY creation. No application dependency or additional server is added.
- **Validation:** Tests cover native argument parsing, explicit-command startup,
  ordinary-session exclusion, private discovery/authentication, right-click versus
  launch, keyboard dismissal, and SSH target stability. Automated coverage does not
  imply a completed UAC desktop acceptance test.
- **Revisit condition:** Supporting an elevated pane inside an existing ordinary
  process requires a separately reviewed broker and authenticated PTY transport.
  A future privileged-residency feature must have explicit recovery/discovery.

The two pane preferences in the same request reuse `nebula_settings`: mouse focus
has an optional GUI override of the compatible TOML setting (default off), while
inactive-pane dimming defaults on to preserve the existing appearance. Both are
cached by the GPUI settings adapter; pointer movement and rendering do not read
settings files.

## ADR-0011 — Editable theme snapshots and preview before application

- **Status:** Implemented in the working tree, 2026-09-14. Windows GPUI product
  build and native interaction checks have run; this is not a release claim.
- **Context:** The fixed built-in theme enum cannot represent user-created themes.
  Users need to start from an existing theme, adjust common settings, preview text
  colors and exchange themes without making the selection page an editor.
- **Decision:** Keep built-in identities compatible. Shared, dependency-free theme
  values and validation belong to `nebula_settings`; versioned JSON, external format
  adapters and library I/O belong to the application's `theme_library` capability.
  A custom theme is an independent snapshot with a stable library identity and a
  built-in fallback. Copying or renaming does not edit its source or create a runtime
  inheritance chain. Existing atomic replacement and OS handle locks protect writes;
  a revision check rejects a competing edit. Unknown native extension data survives
  a round trip. External input is bounded static data and never executes configuration
  scripts, includes or commands.
- **Interaction:** The theme dialog's lower-left custom action opens a dedicated
  editor: existing template, name, visible common settings, then collapsed advanced
  settings. Theme and icon category selections use the same rounded treatment.
  The three suggested foreground swatches contain the original color and readable
  cool/warm alternatives; a fourth multicolor swatch opens arbitrary color selection.
  Foreground changes affect the preview and are saved with explicit application.
  Back returns to the preserved theme picker selection and filter; it confirms
  discarding editor changes before returning. Cancel, close and Escape separately
  exit the workflow and confirm before discarding a changed draft. Saving alone
  writes an independent library copy while leaving the active snapshot unchanged.
  Font selection and color palettes belong to the editor draft, with component
  popovers above the editor surface. Text and numeric inputs use a focusable
  underline; inherited cursor color presentation follows foreground edits.
- **Runtime and cost:** Resolve the active theme on settings changes and read prepared
  values during rendering. Theme defaults, explicit personal preferences and per-session
  OSC overrides remain distinct; changing defaults must not erase session overrides or
  reinterpret truecolor RGB as palette indices. Font and geometry overrides are optional.
  Import, export and library writes use scoped background work with stale-result checks.
  No extra production dependency or resident worker is justified by this feature.
- **Alternatives:** Extending the built-in enum with mutable global data, keeping a
  second renderer-specific validation model, or translating each pair of formats
  independently would couple unrelated lifecycles and duplicate behavior.
- **Validation:** Required evidence includes original-theme preservation, foreground
  persistence/cancellation, malformed input and unknown fields, conflicting saves,
  native palette behavior, keyboard/real control interaction and approved-layout
  comparison. A contrast calculation for opaque default foreground/background is
  not a guarantee for arbitrary transparency, syntax colors or displays. Browser
  prototype checks and native GPUI acceptance are reported separately. The working
  implementation has passed 54 settings-model tests, 24 document/store/format
  contracts (including 90 static color round trips), 14 catalog/allocation tests,
  and Windows native theme interaction checks. Actual window inspection exposed
  and corrected a hidden font popup and stale inherited cursor HEX presentation.
  Window captures cover the theme picker, common editor, font selector, palette,
  and Back confirmation; these are Windows results, not cross-platform visual
  or universal performance guarantees.
- **Revisit condition:** Dynamic inheritance, downloaded resources, additional format
  semantics or system appearance slot changes require their own compatibility and
  ownership evidence; they do not silently expand this snapshot contract.


## ADR-0012 — Bounded background images and explicit atlas retirement

- **Status:** Requested by the maintainer, 2026-09-14; implementation and validation in progress.
- **Evidence:** With blur disabled, twenty window size changes retained an additional
  84 MiB (card) / 190 MiB (window-cover) of dedicated GPU memory on the local Windows
  QA binary. The pinned GPUI atlas does not retire image IDs when Rust Arcs drop.
- **Decision:** Keep one bounded CPU RenderImage shared by the background layers and
  windows. Use the existing renderer authority for fit/alignment and GPUI image
  bounds for GPU sampling; resizing no longer produces new images. Opacity is a
  layer property. Explicitly retire replaced image IDs across window atlases and
  invalidate replayed scenes. Platform atlas copies remain per window.
- **Loading and ownership:** The App visual-effects state owns one active background
  executor job and one latest desired source. Generation checks cancel superseded
  work before expensive stages and prevent stale publication; dropping the owner
  invalidates outstanding work. Metadata and decoding run off the UI thread. There
  is no new service, thread pool, dependency, persisted format or AI lifecycle change.
- **Memory policy:** Retained BGRA is limited to 8 MiB and an edge of 2048 pixels;
  encoded input is streamed with a 64 MiB file limit, and decoder/output admission
  is limited to 128 MiB. Integer thumbnailing precedes RGBA conversion and avoids
  a full-image floating-point resize buffer. These are owned-resource limits, not
  a whole-process or undocumented decoder-scratch guarantee. Existing native-fit
  geometry remains independent of reduced texture resolution.
- **Tradeoff:** High-resolution wallpaper detail is reduced and inputs over the
  admission limits receive a visible error. The window can appear with its normal
  base color while the background loads. These favor the user's explicit memory
  and responsiveness priorities.
- **Validation:** Targeted decode/lifetime/geometry tests, Windows GPUI build,
  repeated-size memory probes and real card/crop/opacity visual checks are required.
  Results are recorded separately and are not implied by this decision.
- **Revisit condition:** Replace manual atlas retirement if upstream introduces
  equivalent ownership-aware image resources. Adopt target-size native decoding
  only with verified peak accounting and compatibility evidence.


## ADR-0013 — Bounded, on-demand filename search

- **Date:** 2026-09-14
- **Context:** Opening Files previously crawled up to 500,000 paths even with an
  empty query. The index retained its full array and several strings per path.
  Users requested approximately 10–15 MiB of sustained browsing/search overhead,
  unchanged matching options and reuse during repeated panel use.
- **Decision:** Keep filename matching in the shared side-panel model. Empty
  queries only enumerate the visible tree. Nonempty queries reuse one bounded
  cache and stream uncached paths through the same matcher/ranker. Cache capacity
  never determines search coverage. Plain, case-sensitive, whole-word and regex
  matching retain their existing semantics and best-first ordering.
- **Ownership and budgets:** Each existing worker owns one root cache (6 MiB);
  all caches share an 8 MiB allocation quota. Published rows carry a lease into
  their consuming view (512 KiB per snapshot, 2 MiB shared). One process-wide
  execution lock bounds simultaneous traversal/ranking buffers; UI threads never
  acquire it. Ranking retains at most 1,000 candidates and 1 MiB. Query regex and
  path parsing have independent bounds. These are allocation budgets, not a
  promise that OS working set or total application memory equals those values.
- **Lifecycle:** A latest-request mailbox replaces the unbounded command queue.
  Revisions cancel obsolete traversal and reject stale publication. Clearing or
  closing Files releases result rows, while a bounded warm cache survives reopening.
  Root changes replace that cache. Local nonrecursive watches are installed before
  enumeration, capped at 128 directories / 64 KiB of path storage. Caches with
  incomplete watch coverage (including WSL) expire after two seconds. WSL search
  executes `find` directly with `wsl.exe --exec`, streams NUL records through a
  four-record channel and terminates its owned command on cancellation/timeout.
  Direct execution preserves paths and `find` arguments containing shell syntax.
  It never terminates terminal sessions.
- **Tradeoff:** Unchanged directories that fit in the watched cache avoid repeat
  walks. Larger trees reuse a prefix for early results but require streaming for
  full coverage. Search reports partial results at its existing 500,000-entry
  ceiling, depth 64, inaccessible paths or result limits. Explicit refresh remains
  available. No new dependency, persisted index or daemon is introduced.
- **Validation:** Matching, watch changes, cancellation, panel reopening, root
  replacement, bounded ranking and allocation ownership have focused regressions.
  An opt-in Windows stress test drives the production panel through multiple
  directories, clear/query/reopen cycles and records actual Vec/String capacities,
  the Windows array heap block and process private commit after allocator warmup.
  Runtime evidence must accompany any claim about the sustained memory target.
- **Replacement condition:** Revisit the budgets or an OS-backed index only with
  measured query latency, completeness and sustained allocation evidence. Do not
  restore eager full-tree indexing to improve a synthetic latency number.

## ADR-0014 — Parallel release validation with one product build graph

- **Date:** 2026-09-14
- **Context:** The 1.7.0 Windows release job took 41m28s despite a full dependency
  cache hit. Its logs show three application test compilations (4m44s, 9m16s and
  4m07s), a 15m08s release build, and 4m16s of additional dependency compilation
  during packaging. The user requested a slowest-job target of ten minutes.
- **Decision:** Run the complete Rust workspace with the product interaction
  feature enabled in one unfiltered invocation. Run native tests independently
  from package construction; asset aggregation depends on both. Each native
  platform still runs the complete Python helper and harness suites. Separate
  test and release cache keys prevent concurrent jobs from replacing one another's
  compiled workload. Dependency archives and Git objects are shared within each
  platform; each workload saves only the profiles it uses (native tests also keep
  release-check metadata). A fallback
  reads existing combined caches during migration. Cargo still validates source,
  profile and feature fingerprints before reuse. Each source revision saves an
  immutable entry while restoring compatible earlier revisions; a partially built
  cache from a failed revision cannot prevent later successful cache updates.
- **Scheduling:** Stable releases call the same complete four-platform native
  workflow used for contributions, including architecture, translation allocation
  contracts and the release-workspace check. Release branch pushes omit a duplicate
  automatic invocation; the release aggregation still requires the called suite.
- **Test profile:** An explicit CI-only profile removes developer-preview
  optimization and debug information from test compilation, including named
  dependency overrides. It retains debug assertions and overflow checks. The
  native suite also checks the actual product feature configuration, since GPUI
  test support changes dependency features. Release optimization is unaffected.
- **Product compilation:** The application keeps O3 and Thin LTO and uses 16
  codegen units to parallelize its large translation unit. Other package settings
  retain their existing values. Both Windows packagers call one explicit builder
  for the product and its packaged hook, preserving the same feature graph.
  Packaging still invokes Cargo and validates source freshness and binary identity.
- **Contract clarification:** The old packaging test required the literal
  `--workspace --exclude nebula`, rejecting a valid explicit selection of the two
  shipped binaries. It now verifies the actual selected packages, binaries and
  product feature, plus failure propagation and restoration of the caller's target.
  No test filtering, freshness bypass, size-budget increase or dependency change
  is part of this decision.
- **Validation:** The release pipeline runs native tests, packaging fixtures,
  package-size/identity checks and installed or mounted conformance on all four
  platforms. Actual job timings determine whether the target is met; cache input
  changes and first compilation must be reported separately. A configured timeout
  or parallel scheduling alone is not evidence of a ten-minute successful build.
- **Windows host privileges:** The first hosted run exposed administrator-token
  dependence in ordinary-window persistence fixtures and runtime discovery.
  Persistence tests now explicitly select ordinary-window state while retaining
  privileged isolation tests. Native conformance launches under a restricted copy
  of the runner's own token, verifies that elevation was removed and preserves
  the desktop, environment and child exit status. It does not weaken the product's
  administrator isolation or create a separate user account. Native launch tests
  cover elevation, literal argument passing and successful/failed child exit.
- **Revisit condition:** Retain only changes whose complete CI run and package
  checks pass. Reconsider codegen partitioning if artifact size or runtime
  measurements regress, and remove redundant caches if restore/save cost grows.

## ADR-0015 — Native font ownership and terminal resource reclamation

- **Status:** Existing local fixes selected for commit at the maintainer's request,
  2026-09-15. This records ownership contracts, not a release or process-memory claim.
- **Context:** Registering immutable font bytes without a DirectWrite owner creates
  a full private copy. Retained caller-side ConPTY pipe handles prevent output EOF
  after a terminal closes, leaving reader tasks and buffers alive. Process-ID reuse
  can also attach an unrelated older process to a newly created terminal's tree.
- **Font decision:** Pin the existing GPUI fork and its component consumers to the
  matching font-owner revisions. A COM owner retains borrowed static bytes or the
  original owned buffer until the last native consumer releases it. Preserve font
  data, fallback behavior and lifetime; do not remove CJK coverage to reduce memory.
  All GPUI dependency edges retain one source identity and exact revision.
- **PTY decision:** Close caller-owned pipe ends after ConPTY has duplicated them.
  Own the process handle, primary-thread handle, process attribute list and loaded
  console library separately. Keep output draining during teardown and failed spawn.
  Wait for cancellation and completion of native exit callbacks before releasing
  their context; an unconfirmed cancellation retains the context and reports an
  error instead of permitting native code to access freed memory.
- **Process boundary:** The platform adapter reads identifiers and creation times
  in one bounded Windows snapshot, without opening protected processes. The shared
  process-tree rules reject an older child's edge to a younger reused parent PID.
  Unknown creation times preserve the edge and existing busy-process protection.
  Unix retains its existing process listing behavior. No resident polling service,
  persistence change or additional production crate is introduced.
- **Validation:** Native font regressions cover borrowed and owned buffers, original
  pointers, final-reference release and invalid lengths. Terminal regressions cover
  idle and busy close, failed launch, in-flight callbacks and native handle recovery
  using the packaged console host. Process tests cover PID reuse, real busy children,
  unknown timestamps and malformed native records. The maintained memory stress
  tool distinguishes peak, warmup and retained growth, and fails incomplete runs or
  forced cleanup. Historical native results and new checks must be reported with
  their actual source/build identities; budgets are not whole-process guarantees.
- **Revisit condition:** Remove the font fork patch when upstream provides the same
  ownership contract. Revisit native adapters when supported platform interfaces
  provide equivalent process identity and resource-lifetime guarantees.


## ADR-0016 — Completion ownership follows the active connection

- **Date:** 2026-09-15
- **Context:** Launch-time Local/WSL/SSH pools do not follow a typed SSH/WSL
  connection. History and directory candidates consequently retain the outer
  pane's source. A remote command-done marker does not mean SSH has exited.
- **Decision:** Keep one shared completion context for both UI adapters. Record
  the connection command in the parent scope, then select a separate connection
  scope for subsequent commands. A known parent shell's prompt restores its scope.
  Clear ghost/popup candidates, dismissal state, input mirrors, cwd and pending
  directory requests on scope changes. Every cache/result retains its environment.
  Local directory history only accepts local reports. Empty/unreadable input and
  ordinary shell commands do not disable completion or learning.
- **Shell adapter:** Reuse OSC 1337 SetUserVar. `pebrel_shell` identifies a shell
  instance at its prompt. `pebrel_command` carries that instance and PSReadLine's
  accepted command, including simple PowerShell alias resolution. This handles
  recall and completed input that the key mirror cannot reconstruct. The SSH/WSL
  execution wrapper reports expanded argv as NUL-separated `pebrel_connection`
  fields, then restores the owner when the actual process returns. Thus a variable
  such as `$targetHost` does not merge different destinations, and return inside a
  compound command does not wait for the next prompt. PowerShell invokes the
  native application; bash/zsh share one payload and preserve user wrappers and
  redirected command output. Integration tokens are generated once per shell and remain unexported. Existing local
  PowerShell/bash/zsh, WSL bash hooks and SSH bootstrap hooks carry these signals.
  Preserve terminal event order across chunks so a cwd cannot move past the
  parent-context report. A new shell on the same host keeps that host's history.
- **Connection identity:** Existing JSONL files/schema remain authoritative. Typed
  SSH/WSL contexts use a `typed:` SHA-256 key in their respective history category,
  incorporating the parent history scope and argument boundaries. This separates
  bastion/config/port routes without reusing an outer SFTP channel for an inner
  host. Typed-connection path completion stays with the native shell. WSL launch
  resolves the default distro from Windows' Lxss registry; an unavailable name
  stays a distinct WSL scope, never Local, and its shell report can supply the
  actual distro. No new service or dependency is added.
- **Native completion:** Preserve PSReadLine prediction settings. The existing
  grid reconciliation yields when shell text occupies the space after the cursor,
  so native inline predictions keep their own acceptance keys. The Pebrel toggle
  only controls its candidates; it does not reconfigure the shell editor.
- **Scope:** This change addresses connection history and candidate ownership.
  It adds no deletion, exit-status filtering, command blacklist or input-quality
  learning switch. Existing records without provenance are retained. Return from
  nested connections is verified with parent shell integration; it is not inferred
  from an untagged exit code or claimed to cover arbitrary uninstrumented wrappers.
- **Validation:** Regressions cover local/SSH/WSL entry, parent return, nested
  reported shells, failed connections, options/ancestry separation, native accepted
  commands, same-host shells and candidate invalidation. Test delayed directory
  results against a changed context and parser order at every byte split. Native
  PowerShell hook tests validate emitted identity, accepted alias text and native
  prediction preservation. A Windows integration test sends execution/return
  reports through the production ConPTY and parses the resulting byte stream. Report
  actual executed checks separately from live remote-server validation.

## ADR-0017 — Scrollback allocation and user scrolling preferences

- **Date:** 2026-09-15
- **Context:** The first history expansion initialized at least 1,000 full-width
  rows per grid. Large column reductions retained oversized cell vectors. Users
  requested lower memory without reducing retained history or scrolling behavior.
- **Decision:** Keep history limits and ring indexing intact. Initialize ahead by
  one viewport, bounded to 32–128 rows, and reclaim surplus against the current
  viewport. During row shrinking, release capacity only when at least 32 cells and
  half the allocation are unused. Retain enough space for short reflow tails to
  reach the destination width without immediately growing again.
- **Cost:** Smaller batches increase ring normalization frequency; vector growth
  is still geometric. Row reclamation runs synchronously during resize and can
  increase a large shrink's latency. Allocation reduction is not a working-set or
  universal throughput guarantee. Existing-history scrolling does not allocate
  new history rows. Compare identical content, geometry and build profiles.
- **Preferences:** `nebula_settings` owns additive `scrollback_lines` and
  `scroll_speed` keys, validation and defaults. History offers seven values from
  1,000 to 100,000, defaults to 10,000 and is passed only to newly created terminal
  sessions; changing it cannot truncate open sessions. Wheel speed defaults to
  1.0 and is bounded to 0.25–4.0. GPUI reads its cached value, retains fractional
  input and leaves pixel-precise trackpad input, font zoom and completion scrolling
  independent. Dragging previews speed; release persists it with failure feedback.
  No smoothing timer, new dependency, worker or terminal persistence format is added.
- **Validation:** Cover rotated growth/reclamation, threshold boundaries, reflow
  content/cursor space, preference round trips/reset and actual selector/slider
  interactions. Record native product, allocation and timing results separately.
- **Revisit condition:** Reconsider batching or deferred reclamation if measured
  large-history output or resize latency becomes unacceptable; preserve the same
  history, ordering and input contracts when evaluating alternatives.

## ADR-0018 — Durable recovery targets and update handoff

- **Status:** Implementation authorized by the maintainer on 2026-09-15; native
  upgrade acceptance is required before declaring this behavior delivered.
- **Context:** Launching setup before terminal shutdown races executable-file
  ownership. A failed snapshot write must be able to cancel exit. A resume command
  submitted to a PTY is not evidence that the provider opened the requested chat.
- **Recovery ownership:** The shared v4 session schema gains optional native file
  and per-pane launch fields. Older snapshots remain readable; per-tab launch is
  only the compatibility fallback. The terminal owns a durable recovery target
  separately from native-confirmed foreground identity. Failed verification or
  resume keeps that target available for retry, while an intentional exit after
  confirmation clears it. Provider metadata is read on the background executor,
  within bounded file/output/time budgets and the original execution environment.
  No conversation text, arbitrary environment map or authentication secret is saved.
- **Identity:** Pi metadata/header IDs are authoritative; process badges and
  timestamped filenames are not native IDs. Legacy timestamp IDs require an exact
  header match. Multiple matching files are an error, never a most-recent-file rule.
  Bridge process identity orders Pi session switches within one monotonic stream.
- **Persistence:** Quit freezes only after durable success. Failure leaves windows
  open and allows later checkpoints to include new native identity. Draft approval
  covers all participating windows, including drafts changed while prompts are up.
  Renderer adapters capture window state; shared persistence owns the write rule.
  Optional window boundaries partition the existing flat tab list in the same
  atomically replaced session document. Old readers retain that flat list; both
  immediate and next-start installation use the last durable window partition
  and reopen the active window last. Invalid partitions cancel installation.
  This preserves window/tab grouping, not desktop coordinates or window sizes
  that the current startup policy deliberately derives from preferences.
- **Download ownership:** Optional predownload grants no permission to install.
  One process-wide state serves prompts and settings. Generation checks reject
  cancelled/obsolete progress and completion. Disk metadata is a cache descriptor;
  reopening and installing reverify the package bytes using the existing official
  asset, proxy, length and SHA-256 contracts.
- **Handoff boundary:** A temporary Windows helper must live outside the target
  installation. It acquires exact process handles, verifies the package, and
  acknowledges readiness before an explicit durable commit allows exit/install.
  Setup starts only after participant exit, with a specific validated target
  directory and without process-name force termination. Kernel lifetime locks
  block competing installation/startup and cannot remain owned after a crash.
  Upgrade restore state and installation failures survive process exit. Inno
  in-place installation is not claimed to provide arbitrary mid-install rollback.
  Native path normalization, process creation time and hidden helper spawning
  belong to `platform::update_installation`; transaction state and commit authority
  remain in the updater. UI startup consults the existing installation capability.
- **Validation boundary:** Provider bridge execution, serialization/failure cases,
  actual GPUI lifecycle tests and native simulated upgrades are separate evidence.
  Metadata compilation alone does not validate restoration or installer behavior.
  Debug-only local rehearsal shares version eligibility with scheduled updates:
  it permits an exact-version reinstall, never a downgrade. Such a reinstall
  must replace the executable bytes; an unchanged executable after setup exits
  zero is a failure. This does not substitute for a full packaged upgrade test.

## ADR-0019 — Optional release contributor section

- **Status:** Approved by the maintainer on 2026-09-15.
- **Context:** A release without new PR contributors previously failed validation
  unless it listed a contributor. This encouraged crediting maintainers or issue
  reporters in the PR contributor area, contrary to the maintainer's policy.
- **Decision:** Omit Contributors when there are no PR contributors to credit.
  When present, the section still requires GitHub links, occurs once after both
  languages, and precedes SHA256. PR eligibility is checked against GitHub during
  release review; the local Markdown checker cannot establish it from a link.
- **Validation:** A bilingual release without Contributors passes. Empty,
  duplicated, unlinked or misplaced contributor sections fail; language and
  asset checksum contracts remain required.

## ADR-0020 — Saved command organization

- **Status:** User-requested working-tree implementation, 2026-09-16; pending
  repository review by `@Kuddev`. No release claim.
- **Context:** Users need named command groups, builtin defaults, drag assignment
  and removal without duplicating immutable builtin command templates.
- **Decision:** Add optional organization metadata to the existing version 1
  command store. Stable group IDs and command IDs own membership; names remain
  display text. Absent builtin membership means the builtin group; explicit null
  means ungrouped. Old stores load unchanged. Older applications can read command
  content but do not preserve this additional metadata when rewriting the store.
  The user also requested deletable builtins: an optional `deleted_builtins` set
  records stable IDs in the same store. Template content stays in the catalog;
  changing language, platform or grouping cannot bring a deleted entry back.
  Older applications also discard this set on rewrite.
- **Ownership:** `saved_commands` remains the sole validation and persistence
  authority. Every mutation locks, reloads, validates and atomically writes both
  commands and organization. No extra file, dependency, worker or service is added.
  The UI sorts and renders a snapshot; it never writes JSON itself.
- **Alternatives:** Per-command group fields would require materializing builtin
  templates and could freeze their platform/localization behavior. A separate
  group file would need a transaction spanning two files.
- **Consequences:** Deleting a group leaves its commands ungrouped; deleting a
  command removes its membership. Stale drag targets fail without changing disk.
  Deleted builtin IDs remain valid after catalog reduction, but cannot be assigned
  to a group. Concurrent mutations preserve deletions through the same transaction.
  Search and keyboard selection use the same displayed command order; headings
  never execute commands. Group names and counts use bounded validation.
- **Validation:** Regression coverage includes old stores, explicit builtin
  removal, restart persistence, stale targets and serialized multiwindow writes.
  Actual GPUI drag, menu and keyboard checks are reported separately from model
  tests; a successful compile is not visual acceptance.
- **Revisit condition:** Reconsider schema versioning if preserving organization
  through edits by older application versions becomes a supported requirement.
