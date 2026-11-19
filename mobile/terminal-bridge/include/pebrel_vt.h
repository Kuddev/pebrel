#ifndef PEBREL_VT_H
#define PEBREL_VT_H
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Serialize access, including frame iteration, on the terminal owner's thread. */
typedef struct PebrelVT PebrelVT;
typedef struct {
    uint16_t columns, rows, cursor_x, cursor_y;
    bool cursor_visible, alternate;
    int cursor_style;
    uint32_t background, cursor_color;
    uint64_t total, offset;
} PebrelFrame;
typedef struct {
    size_t start, length; /* UTF-8 byte range in the row's text buffer */
    uint32_t foreground, background; /* 0xRRGGBB */
    uint8_t width, flags;
} PebrelCell;
enum { PEBREL_BOLD=1, PEBREL_ITALIC=2, PEBREL_UNDERLINE=4, PEBREL_STRIKE=8,
       PEBREL_FAINT=16, PEBREL_INVISIBLE=32 };

PebrelVT *pebrel_vt_new(uint16_t columns, uint16_t rows, uint16_t history);
void pebrel_vt_free(PebrelVT *vt);
bool pebrel_vt_resize(PebrelVT *vt, uint16_t columns, uint16_t rows, uint16_t cell_w, uint16_t cell_h);
/* Feed consumes the input once. -1 reports reply overflow; never replay the input. */
int pebrel_vt_feed(PebrelVT *vt, const uint8_t *input, size_t len, uint8_t *reply, size_t capacity);
bool pebrel_vt_colors(PebrelVT *vt, uint32_t foreground, uint32_t background, uint32_t cursor, const uint32_t ansi[16]);
bool pebrel_vt_begin(PebrelVT *vt, PebrelFrame *frame);
bool pebrel_vt_next_row(PebrelVT *vt, bool *dirty);
/* Returns required text bytes, or -1 on failure. Retry the current row with sufficient space. */
ptrdiff_t pebrel_vt_row(PebrelVT *vt, PebrelCell *cells, size_t columns, uint8_t *text, size_t capacity);
void pebrel_vt_end(PebrelVT *vt);
void pebrel_vt_scroll(PebrelVT *vt, int32_t lines);
/* Key uses USB HID keyboard page usage IDs; mods are shift=1, ctrl=2, alt=4, super=8. */
int pebrel_vt_key(PebrelVT *vt, uint16_t hid, uint16_t mods, const char *text, size_t len,
                  uint32_t unshifted, char *out, size_t capacity);
/* On the primary screen this scrolls history. On an alternate screen it returns encoded mouse/arrow bytes. */
int pebrel_vt_wheel(PebrelVT *vt, int lines, uint16_t x, uint16_t y, char *out, size_t capacity);
int pebrel_vt_paste(PebrelVT *vt, char *input, size_t len, char *out, size_t capacity);
#ifdef __cplusplus
}
#endif
#endif
