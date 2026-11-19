#include "state.h"

bool pebrel_vt_begin(PebrelVT *s, PebrelFrame *f) {
    GhosttyTerminalScrollbar bar = pebrel_vt_scrollbar(s);
    GhosttyRenderStateDirty dirty;
    GhosttyRenderStateCursorVisualStyle style;
    GhosttyTerminalScreen screen;
    bool visible, in_viewport;
    s->colors.size = sizeof(s->colors);
    if (ghostty_render_state_update(s->render, s->terminal) ||
        ghostty_render_state_colors_get(s->render, &s->colors) ||
        ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_DIRTY, &dirty) ||
        ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VISIBLE, &visible) ||
        ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_HAS_VALUE, &in_viewport) ||
        ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VISUAL_STYLE, &style) ||
        ghostty_terminal_get(s->terminal, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen) ||
        ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &s->rows)) return false;
    *f = (PebrelFrame){.columns=s->columns, .rows=s->height, .cursor_visible=visible && in_viewport,
        .alternate=screen == GHOSTTY_TERMINAL_SCREEN_ALTERNATE, .cursor_style=style,
        .background=packed(s->colors.background), .total=bar.total, .offset=bar.offset,
        .cursor_color=packed(s->colors.cursor_has_value ? s->colors.cursor : s->colors.foreground)};
    if (in_viewport) {
        ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_X, &f->cursor_x);
        ghostty_render_state_get(s->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_Y, &f->cursor_y);
    }
    s->full |= dirty == GHOSTTY_RENDER_STATE_DIRTY_FULL;
    return true;
}

bool pebrel_vt_next_row(PebrelVT *s, bool *dirty) {
    if (!ghostty_render_state_row_iterator_next(s->rows)) return false;
    if (ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY, dirty)) return false;
    *dirty |= s->full;
    return true;
}

ptrdiff_t pebrel_vt_row(PebrelVT *s, PebrelCell *cells, size_t columns, uint8_t *text, size_t cap) {
    if (columns < s->columns || (!text && cap) || !cells) return -1;
    if (ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &s->cells)) return -1;
    size_t used = 0, x = 0;
    while (ghostty_render_state_row_cells_next(s->cells)) {
        if (x >= columns) return -1;
        GhosttyCell raw;
        GhosttyCellWide wide;
        GhosttyStyle style = {.size=sizeof(style)};
        GhosttyColorRgb fg = s->colors.foreground, bg = s->colors.background;
        if (ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &raw) ||
            ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &wide) ||
            ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &style)) return -1;
        ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, &fg);
        ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, &bg);
        GhosttyBuffer bytes = {.ptr=used < cap ? text + used : NULL, .cap=used < cap ? cap-used : 0};
        uint8_t width = wide == GHOSTTY_CELL_WIDE_WIDE ? 2 : wide == GHOSTTY_CELL_WIDE_NARROW ? 1 : 0;
        if (width) {
            GhosttyResult result = ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &bytes);
            if (result != GHOSTTY_SUCCESS && result != GHOSTTY_OUT_OF_SPACE) return -1;
            if (!bytes.len) {
                if (used < cap) text[used] = ' ';
                bytes.len = 1;
            }
        }
        cells[x++] = (PebrelCell){.start=used, .length=bytes.len, .width=width,
            .foreground=packed(style.inverse ? bg : fg), .background=packed(style.inverse ? fg : bg),
            .flags=(style.bold ? PEBREL_BOLD : 0) | (style.italic ? PEBREL_ITALIC : 0) |
                (style.underline ? PEBREL_UNDERLINE : 0) | (style.strikethrough ? PEBREL_STRIKE : 0) |
                (style.faint ? PEBREL_FAINT : 0) | (style.invisible ? PEBREL_INVISIBLE : 0)};
        used += bytes.len;
    }
    if (used <= cap) {
        bool clean = false;
        ghostty_render_state_row_set(s->rows, GHOSTTY_RENDER_STATE_ROW_OPTION_DIRTY, &clean);
    }
    return (ptrdiff_t)used;
}

void pebrel_vt_end(PebrelVT *s) {
    GhosttyRenderStateDirty clean = GHOSTTY_RENDER_STATE_DIRTY_FALSE;
    ghostty_render_state_set(s->render, GHOSTTY_RENDER_STATE_OPTION_DIRTY, &clean);
    s->full = false;
}
