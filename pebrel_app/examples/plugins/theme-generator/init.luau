-- Returns a theme spec: a built-in base plus the values to change. The host
-- validates it; installed with the theme-library grant, it is added to the
-- theme library and never applied automatically.
local function channel(value: number): string
    return string.format('%02x', math.clamp(math.floor(value + 0.5), 0, 255))
end

return {
    ocean = function(ctx, args: { depth: number? })
        local depth = math.clamp(args.depth or 0.5, 0, 1)
        local background = '#' .. channel(4) .. channel(24 + 40 * (1 - depth)) .. channel(48 + 60 * (1 - depth))
        return {
            base = 'Nord',
            name = 'Ocean',
            terminal = { background = background },
        }
    end,
}
