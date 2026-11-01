# Mobile interface principles

Scope: Android session overview, terminal/conversation views and message composition.
These are constraints for ongoing UI work, not a claim that every existing screen
has already been audited. The current implementation uses Jetpack Compose and
Material 3; the terminal and rich reader retain their existing rendering paths.

## Component ownership

- Prefer library controls for selection, menus, dialogs, lists and buttons. Keep
  business wrappers small; let the library own focus, semantics and press feedback.
- `HomeScreen` retains the horizontally scrollable session-preview gallery, full
  count and All sessions action. Local/SSH cards show the latest retained terminal
  text when the gallery opens; desktop cards retain their host, state and path.
  Every session remains reachable, and the whole card opens its exact target.
  Do not replace terminal previews with metadata-only rows as a style change.
- `SessionModeTabs` uses Material single-choice segmented buttons. Conversation
  and terminal are related views of the same pane, not separate application tabs.
  Only a change in selection navigates. Unsupported direct SSH sessions do not
  receive a nonfunctional conversation switch.
- `CommandComposer` owns draft and send state. Conversation typography is
  proportional; code and terminal input use the configured terminal face.
- Theme, typography and shapes belong to `PebrelTheme`. Screens consume semantic
  roles instead of defining private color schemes. The rich reader receives the
  same roles through `ReaderColors`.

## Organization and geometry

Apply the Apple HIG principles of grouping related controls, a clear content
hierarchy and progressive disclosure while retaining Android platform behavior.

- Separate navigation, content and input. Give each region one clear purpose.
- Preserve the session gallery and grow its cards with font size. Keep saved-host
  and computer lists aligned below it, with secondary paths subordinate.
- Keep tool output collapsed until requested. Assistant prose remains a readable
  document; user messages receive a distinct surface without enclosing every reply.
- Use theme shape roles: small (14 dp) for grouped mode controls, large (20 dp)
  for the conversation composer. Reader user-message corners use 14 px; code uses
  its existing 6 px treatment. These are local design choices, not Apple constants.
- Do not infer Apple's continuous-corner geometry from a numeric radius or claim
  that Android rounded rectangles reproduce UIKit rendering exactly.
- Prefer spacing and a quiet separator to nested cards, decorative badges or
  repeated borders. Glass effects require a functional reason and measured cost.

## Color and state

- Use MintLight on fresh installs; preserve saved themes and the system-follow option.
- Preserve all registered palettes. Rust theme
  definitions remain the palette authority; Android assets are generated from it.
- Distinguish background, raised/selected surface, primary/secondary text, border,
  action and error. A semantic name alone does not prove sufficient contrast.
- Radix's role-based color scales and Apple's semantic labels/materials are useful
  references for future palette work. They are not new installed themes.
- Selection must be visible without relying on hue alone. State needs readable
  text; an animated dot is insufficient.
- Prefer one primary action. Destructive actions remain explicit and distinguishable.

## Continuity and perceived speed

- Preserve unchanged message DOM, selection, expanded tools and scroll anchors.
  Follow new output only near the end and while no text is selected.
- Keep transcript and draft visible during disconnect/reconnect. A connection
  status must describe what is known, not imply successful execution.
- Sending clears a matching draft only after acknowledgment. Unknown delivery
  must remain uncertain; reconnect must not automatically repeat a command.
- Keep expensive parsing/rendering out of the input path. Do not add whole-page
  transitions or repeated highlighting for background transcript updates.
- The current conversation transport still polls. DOM reuse is not a claim of
  event-driven transport, 120 fps, or measured network-latency improvement.

## Platform and acceptance

- Android controls use Android semantics, back handling and at least 48 dp targets
  for the new segmented controls. HIG's 44 pt recommendation is not a dp conversion.
- Respect keyboard insets, font scaling, reduced motion and TalkBack. Preserve
  selection and drafts when changing modes or restoring connectivity.
- Check narrow (320 dp), ordinary phone and larger widths, both light/dark themes,
  long titles and 2× text. Native screenshots, browser reader checks, compilation
  and device interaction tests are distinct evidence; report which actually ran.
- Native direct SSH does not yet expose structured chat. Desktop-owned SSH chat
  depends on recognized agent/session metadata and an authenticated transcript reader;
  terminal escape sequences alone are not a conversation protocol.

## References

- [Apple HIG: Layout](https://developer.apple.com/design/human-interface-guidelines/layout)
- [Apple HIG: Segmented controls](https://developer.apple.com/design/human-interface-guidelines/segmented-controls)
- [Apple HIG: Buttons](https://developer.apple.com/design/human-interface-guidelines/buttons)
- [Apple HIG: Color](https://developer.apple.com/design/human-interface-guidelines/color)
- [Apple HIG: Materials](https://developer.apple.com/design/human-interface-guidelines/materials)
- [Compose Material 3](https://developer.android.com/develop/ui/compose/designsystems/material3)
- [Radix Colors: scale roles](https://www.radix-ui.com/colors/docs/palette-composition/understanding-the-scale)
