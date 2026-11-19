#include "state.h"
#include <limits.h>
#include <string.h>

static GhosttyKey key_for_usage(uint16_t hid) {
    if (hid >= 4 && hid <= 29) return GHOSTTY_KEY_A + hid - 4;
    if (hid >= 30 && hid <= 38) return GHOSTTY_KEY_DIGIT_1 + hid - 30;
    if (hid >= 58 && hid <= 69) return GHOSTTY_KEY_F1 + hid - 58;
    static const GhosttyKey edits[] = {
        GHOSTTY_KEY_DIGIT_0, GHOSTTY_KEY_ENTER, GHOSTTY_KEY_ESCAPE, GHOSTTY_KEY_BACKSPACE,
        GHOSTTY_KEY_TAB, GHOSTTY_KEY_SPACE, GHOSTTY_KEY_MINUS, GHOSTTY_KEY_EQUAL, GHOSTTY_KEY_BRACKET_LEFT,
        GHOSTTY_KEY_BRACKET_RIGHT, GHOSTTY_KEY_BACKSLASH, GHOSTTY_KEY_INTL_BACKSLASH, GHOSTTY_KEY_SEMICOLON,
        GHOSTTY_KEY_QUOTE, GHOSTTY_KEY_BACKQUOTE, GHOSTTY_KEY_COMMA, GHOSTTY_KEY_PERIOD, GHOSTTY_KEY_SLASH,
        GHOSTTY_KEY_CAPS_LOCK
    };
    if (hid >= 39 && hid <= 57) return edits[hid - 39];
    static const GhosttyKey navigation[] = {
        GHOSTTY_KEY_INSERT, GHOSTTY_KEY_HOME, GHOSTTY_KEY_PAGE_UP, GHOSTTY_KEY_DELETE, GHOSTTY_KEY_END,
        GHOSTTY_KEY_PAGE_DOWN, GHOSTTY_KEY_ARROW_RIGHT, GHOSTTY_KEY_ARROW_LEFT, GHOSTTY_KEY_ARROW_DOWN,
        GHOSTTY_KEY_ARROW_UP
    };
    if (hid >= 73 && hid <= 82) return navigation[hid - 73];
    return GHOSTTY_KEY_UNIDENTIFIED;
}

int pebrel_vt_key(PebrelVT *s, uint16_t hid, uint16_t mods, const char *text, size_t len,
                  uint32_t unshifted, char *out, size_t capacity) {
    if ((!text && len) || (!out && capacity) || capacity > INT_MAX) return -1;
    ghostty_key_encoder_setopt_from_terminal(s->keys, s->terminal);
    ghostty_key_event_set_key(s->key, key_for_usage(hid));
    ghostty_key_event_set_action(s->key, GHOSTTY_KEY_ACTION_PRESS);
    ghostty_key_event_set_mods(s->key, mods);
    ghostty_key_event_set_consumed_mods(s->key, 0);
    ghostty_key_event_set_utf8(s->key, text ? text : "", len);
    ghostty_key_event_set_unshifted_codepoint(s->key, unshifted);
    size_t written;
    GhosttyResult result = ghostty_key_encoder_encode(s->keys, s->key, out, capacity, &written);
    ghostty_key_event_set_utf8(s->key, "", 0);
    return result == GHOSTTY_SUCCESS ? (int)written : -1;
}

int pebrel_vt_wheel(PebrelVT *s, int lines, uint16_t x, uint16_t y, char *out, size_t cap) {
    GhosttyTerminalScreen screen;
    if (ghostty_terminal_get(s->terminal, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen)) return -1;
    if (screen == GHOSTTY_TERMINAL_SCREEN_PRIMARY) {
        pebrel_vt_scroll(s, lines);
        return 0;
    }
    if (!lines) return 0;
    if (lines < -32) lines = -32;
    if (lines > 32) lines = 32;
    bool tracking = false;
    ghostty_terminal_get(s->terminal, GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING, &tracking);
    char packet[128];
    size_t count = 0;
    if (tracking) {
        GhosttyMouseEncoderSize size = {.size=sizeof(size), .cell_width=s->cell_w, .cell_height=s->cell_h,
            .screen_width=(uint32_t)s->columns*s->cell_w, .screen_height=(uint32_t)s->height*s->cell_h};
        ghostty_mouse_encoder_setopt_from_terminal(s->mouse, s->terminal);
        ghostty_mouse_encoder_setopt(s->mouse, GHOSTTY_MOUSE_ENCODER_OPT_SIZE, &size);
        ghostty_mouse_event_set_action(s->event, GHOSTTY_MOUSE_ACTION_PRESS);
        ghostty_mouse_event_set_button(s->event, lines < 0 ? GHOSTTY_MOUSE_BUTTON_FOUR : GHOSTTY_MOUSE_BUTTON_FIVE);
        ghostty_mouse_event_set_mods(s->event, 0);
        GhosttyMousePosition pos = {
            ((x < s->columns ? x : s->columns-1) + .5f) * s->cell_w,
            ((y < s->height ? y : s->height-1) + .5f) * s->cell_h
        };
        ghostty_mouse_event_set_position(s->event, pos);
        if (ghostty_mouse_encoder_encode(s->mouse, s->event, packet, sizeof(packet), &count)) return -1;
    } else {
        bool enabled = false;
        ghostty_terminal_mode_get(s->terminal, GHOSTTY_MODE_ALT_SCROLL, &enabled);
        if (!enabled) return 0;
        int n = pebrel_vt_key(s, lines < 0 ? 82 : 81, 0, "", 0, 0, packet, sizeof(packet));
        if (n < 0) return -1;
        count = (size_t)n;
    }
    size_t repeats = (size_t)(lines < 0 ? -lines : lines);
    if (!count) return 0;
    if (!out || count * repeats > cap) return -1;
    for (size_t i = 0; i < repeats; ++i) memcpy(out + i * count, packet, count);
    return (int)(count * repeats);
}

int pebrel_vt_paste(PebrelVT *s, char *input, size_t len, char *out, size_t cap) {
    bool bracketed = false;
    size_t written;
    if ((!input && len) || (!out && cap) || cap > INT_MAX ||
        ghostty_terminal_mode_get(s->terminal, GHOSTTY_MODE_BRACKETED_PASTE, &bracketed)) return -1;
    return ghostty_paste_encode(input, len, bracketed, out, cap, &written) == GHOSTTY_SUCCESS ? (int)written : -1;
}
