# Windows Explorer helper

- Windows-only, standalone Cargo workspace. It has a separate COM-surrogate
  lifetime; do not link the GUI or terminal core into Explorer.
- Build/install entry points are the context-menu scripts under `scripts/`.
- Use the existing installer labels as the translation authority.
- Registration is per user, owned by the installer and its native helper. Never change Developer Mode, import
  certificates, terminate Explorer, or remove another installation's package.
- Ownership and deployment rationale:
  [local context-menu integration](../../architecture/notes/tools/windows-context-menu/2026-11-08-local-explorer-extension.md).
- Installer lifecycle and signing boundary:
  [automatic registration](../../architecture/notes/packaging/2026-11-17-explorer-menu-lifecycle.md).
