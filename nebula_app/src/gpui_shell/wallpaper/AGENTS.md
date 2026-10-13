# Wallpaper resources

- `image_loader.rs` owns file/decode/output budgets and serialized preparation;
  runtime and draft preview callers reuse it rather than creating another decoder policy.
- No renderer callback reads a file, parses settings, decodes images, or waits on a mutex.
- Preview resources belong to their editor entity. Keep generation cancellation,
  latest-source coalescing, and owned-cache retirement on release; borrowed runtime
  images retain their original owner.
- Bounds, alpha, and channel order must remain consistent with the shared wallpaper layout.
- Resource/lifecycle rationale: [bounded draft preview](../../../../architecture/notes/nebula_app/gpui_shell/wallpaper/2026-10-02-draft-background-preview.md).
