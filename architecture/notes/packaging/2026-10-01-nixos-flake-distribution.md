# NixOS flake as an independent distribution path

## Status

Implemented in the working tree; the release workflow and general CI do not build or
validate it. Structural and runtime evidence is attached manually with the change.

## Context

NixOS users asked for a native install path, and `github:Kuddev/pebrel#pebrel` is the
documented entry in `INSTALL.md`. The flake introduces a second packaging path with
its own build graph, dependency/toolchain selection, runtime RPATH set, source-version
authority and asset layout, so `packaging/AGENTS.md` requires a recorded decision
rather than an undocumented parallel package.

## Evidence

- `flake.nix` defines `packages.x86_64-linux.pebrel` (and `default`) plus
  `overlays.default`; no other output is responsible for a release asset.
- The `version` binding in `flake.nix` is derived from `nebula_app/Cargo.toml`, and the
  flake builds the repository source (`src = self`) rather than a separately published
  artifact.
- Toolchain selection: the flake follows the `nixos-unstable` input, locked to an exact
  revision in `flake.lock`, and builds with
  `rustPackages_1_98.rustPlatform.buildRustPackage`. This is independent of the host's
  NixOS channel: a stable or unstable host builds from the locked revision.
  `rust-toolchain.toml` (1.97.1) is not consumed by the Nix build, which selects the
  nixpkgs Rust package instead.
- `runtimeLibs` plus the `postFixup` `patchelf --add-rpath` step are a flake-owned
  runtime dependency set. GPUI/winit `dlopen` X11/Wayland/Vulkan/OpenSSL/git2/sqlite/
  zstd at runtime, so these libraries are both build inputs and the RPATH set.
- `postInstall` installs the desktop entry, metainfo, completions, license files and the
  icon. It places `extra/logo/nebula.png` (1024x1024) into `hicolor/1024x1024/apps/`
  instead of resizing to 256x256 as `scripts/package-linux.sh` does with ImageMagick;
  `Icon=io.github.kuddev.pebrel` resolves by name, so the size directory must match the
  actual bitmap.
- `cargoBuildFlags` in `flake.nix` builds both `--bin=pebrel` and `--bin=pebrel-hook`
  from `nebula_hook`. The application resolves the helper from the executable's own
  directory (`nebula_app/src/ai_hook/local.rs`, `helper_path_from_exe`), and
  `scripts/package-linux.sh` requires and installs the same helper.
- No workflow under `.github/workflows/` references the flake; the packaging jobs build
  AppImage, DEB and portable archives only.

## Decision

The flake is an **independent, source-built, x86_64-linux path**. It owns its build
graph, toolchain selection, runtime library set, RPATH and installation layout. It does
**not** own release artifacts, the update protocol or the official payload list, and
must not become their source. Shared packed content stays owned by its current
authorities: the desktop entry and metainfo by `packaging/linux/`, completions by
`extra/completions/`, and the release asset set by `scripts/package-linux.sh` and the
release workflow. The flake consumes these files; it does not fork them.

**Dependency and toolchain selection.** The `nixpkgs` input follows `nixos-unstable`,
but `flake.lock` pins the exact revision, so the build is reproducible; refresh it
deliberately with `nix flake update`, not implicitly. Build with the packaged Rust
platform and `--locked`. Disable vendored `-sys` downloads
(`OPENSSL_NO_VENDOR`, `LIBGIT2_NO_VENDOR`, `LIBSQLITE3_SYS_USE_PKG_CONFIG`,
`ZSTD_SYS_USE_PKG_CONFIG`) and link the Nix-provided libraries instead. The
`runtimeLibs` list and RPATH are deliberately part of this path, not inherited from the
AppImage/DEB bundling.

**Package contents.** `$out` must contain, under one prefix:

- `bin/pebrel` — the GPUI product binary (`--features nebula/gpui-shell`).
- `bin/pebrel-hook` — the AI hook helper, beside the application binary, because the
  runtime lookup and `scripts/package-linux.sh` both require it there.
- `share/applications/io.github.kuddev.pebrel.desktop`,
  `share/metainfo/io.github.kuddev.pebrel.metainfo.xml`,
  `share/icons/hicolor/1024x1024/apps/io.github.kuddev.pebrel.png`,
  `share/{bash-completion,fish,zsh}` completions,
  and the `share/doc/pebrel/licenses/*` notices.

Omitting the helper silently disables local Agent hook installation; an absent
`runtime/` subdirectory is acceptable because the same-directory lookup is the
supported Linux location.

**Version authority.** Keep one explicit version: `nebula_app/Cargo.toml`. The flake
reads it instead of carrying a second literal, so a flake build cannot ship a source
revision that differs from the checkout.

**Release-update steps.**

- On a version bump, update `nebula_app/Cargo.toml` (already part of the release
  process); the flake needs no version edit.
- Platform scope is explicit: `system`/`meta.platforms` are `x86_64-linux`. Other
  architectures fail with a missing-attribute error rather than a silent stub.

**Maintaining the Cargo source hash.** `cargoHash` is a fixed-output hash of the
vendored dependency set, so it depends on `Cargo.lock`, not on application source. It
only changes when dependencies change:

1. Set `cargoHash = p.lib.fakeHash;`.
2. Run `nix build .#pebrel` (no `--keep-going` needed for a single failing hash).
3. Copy the reported `got:` SRI value back into `cargoHash`.

`nix-update --flake` can perform this automatically. `importCargoLock` is not used: it
keys `outputHashes` per git package, and this lock pulls dozens of crates from the
`Kuddev/zed` and `Kuddev/gpui-component` repositories.

## Rejected alternatives

- Making the flake the release authority, or generating release assets from it:
  duplicates the payload list and freshness contract that `package-linux.sh` owns.
- Pinning a published release tag inside the flake while building application source
  from the checkout: the pin and the built source can diverge, so flake edits appear to
  build successfully without packaging the corresponding code.
- Carrying a second `version` literal in `flake.nix`: two version authorities drift from
  `Cargo.toml`.
- Shipping only the application binary and leaving the Agent helper to a separate
  mechanism: the runtime lookup and `package-linux.sh` both require it beside the
  executable.
- Resizing the icon with ImageMagick to keep the 256x256 directory: adds a heavy build
  dependency to preserve a directory name that carries no lookup obligation.
- `importCargoLock` with per-git-package `outputHashes`: dozens of manual hashes for the
  Zed/gpui-component crates.
- `cargoLock` with `allowBuiltinFetchGit`, or otherwise building impurely: requires
  `--impure` and breaks the documented `github:...#pebrel` install.
- Following `nixos-unstable` without committing `flake.lock`: an unrecorded toolchain
  and library set.
- Supporting additional architectures before a host is validated: an unverified
  platform would advertise a package that was never exercised.

## Consequences

The flake must be updated independently of the release workflow, and no CI job builds
or tests it. A broken flake is invisible to the merge gate, so the manual checks below
are the only guarantee. Conversely, the flake can move with the repository source
without waiting for a packaged release, and carries no extra dependency for contributors
who do not use Nix.

The locked input is independent of the host channel: `nix build` and `nix profile add`
use the flake's own nixpkgs, so a NixOS stable and a NixOS unstable host install the
same revision, at the cost of a second glibc/library set in the store. Consuming
`overlays.default` instead builds against the caller's `pkgs` and avoids that
duplication, but then depends on that nixpkgs providing the selected Rust package.

`INSTALL.md` describes the flake path separately from the numbered Preview packages;
the documented command resolves the repository's default-branch flake and therefore only
works once this path is merged and published upstream.

## Validation

General CI does not run Nix, so the following evidence must be produced manually and
attached to the change. Structural build and runtime acceptance are separate claims:

- **Build/install.** `nix build .#pebrel` succeeds, and installing into a profile
  (`nix profile add .#pebrel`) succeeds without a conflicting entry.
- **Package contents.** `result/bin/pebrel` and `result/bin/pebrel-hook` both exist and
  are executable; the desktop entry, metainfo, completions and the 1024x1024 icon are
  present under `result/share/`.
- **Application launch.** `result/bin/pebrel --version` matches the package version and
  `--help` advertises `--gpui`; the GUI starts under the supported session (X11 or
  Wayland). Screenshots or a terminal transcript identify the build revision.
- **Helper availability.** `result/bin/pebrel-hook` is executable, and the application
  finds it via `helper_path_from_exe` (no "hook helper missing" warning when installing
  an Agent integration).

These steps are platform acceptance, not something the repository's general CI
substitutes for. A dirty Git tree warning is expected when building uncommitted local
changes.

## Supersedes

None.

## Revisit when

- `nixpkgs` (or another maintained channel) takes over Pebrel packaging.
- The release workflow begins producing or publishing Nix artifacts.
- A second architecture (for example `aarch64-linux`) is requested and a host is
  available to validate it.
- The flake layout must match the official package exactly, or the flake gains an update
  protocol of its own.
