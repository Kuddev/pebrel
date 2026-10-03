# Portable theme packages

- `mod.rs` owns the manifest, portable-path contract, and package byte limits;
  callers reuse those rules and do not trust catalog declarations as validation.
- `envelope.rs` bounds and validates the ZIP directory before general reader
  allocation. Preserve protection against Zip64 and embedded-archive fallback.
- `archive.rs` streams resource verification, export, and staged installation.
  Never extract undeclared paths or activate settings as a side effect of import.
- Package operations are cold I/O. GUI callers must use a background executor
  with an owner and stale-result handling; render callbacks do not read archives.
- Resource directories currently survive library-document deletion. Do not add
  cleanup based only on a directory named by untrusted document metadata.
- Format and ownership rationale: [portable resources](../../../../architecture/notes/nebula_app/theme_library/package/2026-10-02-portable-resource-package.md).
