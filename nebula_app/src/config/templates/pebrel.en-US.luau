local pebrel = require 'pebrel'
local config = pebrel.config_builder()

-- Pebrel Luau configuration. Run `pebrel config check` after editing to validate syntax and fields.
-- Field names and enum values use stable English identifiers; comments explain safe edits.

-- Number of scrollback lines to retain. Larger values consume more memory.
config.scrolling = {
    history = 10000,
}

-- Font example. Remove the leading `--` from each line below to enable it.
-- config.font = {
--     normal = { family = 'Maple Mono Normal NF CN', style = 'Regular' },
--     size = 13.0,
--     builtin_box_drawing = true,
-- }

-- Window example. opacity ranges from 0.0 to 1.0.
-- config.window = {
--     opacity = 0.96,
--     decorations = 'Full',
--     dynamic_title = true,
--     padding = { x = 8, y = 8 },
-- }

-- Cursor example.
-- config.cursor = {
--     style = { shape = 'Beam', blinking = 'On' },
--     unfocused_hollow = true,
-- }

-- Terminal foreground, background, and the ANSI palette can be overridden independently.
-- config.colors = {
--     primary = { foreground = '#d8dee9', background = '#16181d' },
-- }

-- The default shell can be a program string or a program with arguments.
-- config.terminal = {
--     shell = { program = 'pwsh.exe', args = { '-NoLogo' } },
-- }

-- Linux example: select a shell for the current platform while sharing this file with Windows.
-- if pebrel.platform.os == 'linux' then
--     config.terminal = { shell = '/bin/bash' }
-- end

-- Environment variables injected into every new terminal process.
-- config.env = {
--     EDITOR = 'nvim',
-- }

-- Quick-launch profiles. An empty list must use pebrel.array(), not a plain empty table {}.
-- config.profiles = pebrel.array {
--     { name = 'Production SSH', command = 'ssh', args = { 'user@server' } },
--     { name = 'Project', command = 'pwsh.exe', args = { '-NoLogo' }, cwd = 'D:/src/project' },
-- }

-- Large configurations can use sibling modules; for example, theme.luau can return a colors table:
-- local theme = require 'theme'
-- config.colors = theme
-- Required Luau files are automatically added to the live-reload watch list.

-- Unknown fields are strict by default. This can assist migration but should not stay disabled:
-- config:set_strict_mode(false)

-- Native UI defaults. Saved Settings values take priority; remove a saved key to inherit.
-- Script edits take effect after restarting Pebrel. Sizes are logical pixels.
-- config.ui = {
--     theme = 'Nord',
--     font_size = 16,
--     ui_font_size = 13,
--     tabs_position = 'sidebar',
--     sidebar_width = 230,
--     density = 'compact',
--     opacity = 0.96,
--     blur = 'none',
--     tab_bar_opacity = 0.92,
--     pane_card_radius = 10,
--     pane_card_gutter = 6,
--     pane_card_shadow = true,
--     pane_card_divider = 1,
--     cursor_motion = 'smooth',
--     scroll_speed = 1.0,
--     scrollbar_visibility = 'hover',
--     background_image = 'wallpapers/background.png',
--     background_image_opacity = 0.38,
-- }

-- Shader files are relative to this config; paths alone do not enable execution.
-- Saved Settings values take priority. Restart after editing this file.
-- config.shaders = {
--     background = { enabled = true, path = 'shaders/background.glsl',
--                    speed = 1.0, frame_rate = 20, parameter = 0.5 },
--     terminal = { enabled = true, paths = { 'shaders/scanlines.frag' },
--                  animation = 'focused', speed = 1.0, frame_rate = 20,
--                  parameters = { 0.4, 0.0, 0.0, 0.0 } },
-- }

return config
