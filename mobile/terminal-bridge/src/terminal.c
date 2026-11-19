#include "state.h"
#include <limits.h>
#include <stdlib.h>
#include <string.h>

static void write_reply(GhosttyTerminal terminal, void *user, const uint8_t *bytes, size_t len) {
    (void)terminal;
    PebrelVT *s = user;
    if (len > s->reply.cap - s->reply.len) {
        s->overflow = true;
    } else if (len) {
        memcpy(s->reply.ptr + s->reply.len, bytes, len);
        s->reply.len += len;
    }
}

void pebrel_vt_free(PebrelVT *s) {
    if (!s) return;
    ghostty_mouse_event_free(s->event);
    ghostty_mouse_encoder_free(s->mouse);
    ghostty_key_event_free(s->key);
    ghostty_key_encoder_free(s->keys);
    ghostty_render_state_row_cells_free(s->cells);
    ghostty_render_state_row_iterator_free(s->rows);
    ghostty_render_state_free(s->render);
    ghostty_terminal_free(s->terminal);
    free(s);
}

PebrelVT *pebrel_vt_new(uint16_t columns, uint16_t rows, uint16_t history) {
    if (!columns || !rows || columns > 4096 || rows > 4096 || history > 5000) return NULL;
    PebrelVT *s = calloc(1, sizeof(*s));
    if (!s) return NULL;
    s->columns = columns;
    s->height = rows;
    s->history = history;
    s->cell_w = s->cell_h = 1;
    s->full = true;
    // The pinned engine budgets storage in bytes. Bound the visible window separately.
    size_t budget = history ? (size_t)history * 401 * 16 + 131072 : 0;
    if (budget > 16777216) budget = 16777216;
    GhosttyTerminalOptions options = {columns, rows, budget};
    size_t no_images = 0;
    if (ghostty_terminal_new(NULL, &s->terminal, options) ||
        ghostty_render_state_new(NULL, &s->render) ||
        ghostty_render_state_row_iterator_new(NULL, &s->rows) ||
        ghostty_render_state_row_cells_new(NULL, &s->cells) ||
        ghostty_key_encoder_new(NULL, &s->keys) || ghostty_key_event_new(NULL, &s->key) ||
        ghostty_mouse_encoder_new(NULL, &s->mouse) || ghostty_mouse_event_new(NULL, &s->event) ||
        ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_USERDATA, s) ||
        ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_WRITE_PTY, (const void *)write_reply) ||
        ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT, &no_images)) {
        pebrel_vt_free(s);
        return NULL;
    }
    return s;
}

bool pebrel_vt_resize(PebrelVT *s, uint16_t cols, uint16_t rows, uint16_t w, uint16_t h) {
    if (!cols || !rows || cols > 4096 || rows > 4096 || !w || !h) return false;
    if (ghostty_terminal_resize(s->terminal, cols, rows, w, h)) return false;
    s->columns = cols;
    s->height = rows;
    s->cell_w = w;
    s->cell_h = h;
    s->full = true;
    return true;
}

int pebrel_vt_feed(PebrelVT *s, const uint8_t *input, size_t len, uint8_t *reply, size_t cap) {
    if ((!reply && cap) || (!input && len) || cap > INT_MAX) return -1;
    s->overflow = false;
    s->reply = (GhosttyBuffer){reply, cap, 0};
    if (len) ghostty_terminal_vt_write(s->terminal, input, len);
    int result = s->overflow ? -1 : (int)s->reply.len;
    s->reply = (GhosttyBuffer){0};
    return result;
}

static GhosttyColorRgb rgb(uint32_t value) {
    return (GhosttyColorRgb){(uint8_t)(value >> 16), (uint8_t)(value >> 8), (uint8_t)value};
}
bool pebrel_vt_colors(PebrelVT *s, uint32_t fg, uint32_t bg, uint32_t cursor, const uint32_t ansi[16]) {
    GhosttyColorRgb palette[256];
    if (ghostty_terminal_get(s->terminal, GHOSTTY_TERMINAL_DATA_COLOR_PALETTE_DEFAULT, palette)) return false;
    for (int i = 0; i < 16; ++i) palette[i] = rgb(ansi[i]);
    GhosttyColorRgb f = rgb(fg), b = rgb(bg), c = rgb(cursor);
    s->full = true;
    return !ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, &f) &&
           !ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, &b) &&
           !ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, &c) &&
           !ghostty_terminal_set(s->terminal, GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, palette);
}

GhosttyTerminalScrollbar pebrel_vt_scrollbar(PebrelVT *s) {
    GhosttyTerminalScrollbar bar = {0};
    ghostty_terminal_get(s->terminal, GHOSTTY_TERMINAL_DATA_SCROLLBAR, &bar);
    uint64_t first = bar.total > bar.len + s->history ? bar.total - bar.len - s->history : 0;
    if (bar.offset < first) {
        GhosttyTerminalScrollViewport move = {.tag = GHOSTTY_SCROLL_VIEWPORT_DELTA};
        move.value.delta = (int64_t)(first - bar.offset);
        ghostty_terminal_scroll_viewport(s->terminal, move);
        bar.offset = first;
    }
    bar.total -= first;
    bar.offset -= first;
    return bar;
}
void pebrel_vt_scroll(PebrelVT *s, int32_t lines) {
    GhosttyTerminalScrollViewport move = {.tag = GHOSTTY_SCROLL_VIEWPORT_DELTA};
    move.value.delta = lines;
    ghostty_terminal_scroll_viewport(s->terminal, move);
    pebrel_vt_scrollbar(s);
    s->full = true;
}
