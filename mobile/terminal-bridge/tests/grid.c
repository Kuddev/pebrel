#include "pebrel_vt.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static int feed(PebrelVT *vt, const char *stream, char *reply, size_t capacity) {
    return pebrel_vt_feed(vt, (const uint8_t *)stream, strlen(stream), (uint8_t *)reply, capacity);
}

int main(void) {
    assert(!pebrel_vt_new(0, 2, 0));
    PebrelVT *vt = pebrel_vt_new(12, 3, 5);
    assert(vt);
    uint32_t ansi[16] = {0};
    ansi[1] = 0xad3120;
    assert(pebrel_vt_colors(vt, 0x444444, 0xfefefe, 0x883311, ansi));
    char reply[128] = {0};
    assert(feed(vt, "\033[31;1m中e\xcc\x81\033[0m\033[6n", reply, sizeof(reply)) == 6);
    assert(!memcmp(reply, "\033[1;4R", 6));
    PebrelFrame frame;
    assert(pebrel_vt_begin(vt, &frame));
    assert(frame.cursor_x == 3 && frame.cursor_y == 0 && frame.background == 0xfefefe);
    bool dirty;
    assert(pebrel_vt_next_row(vt, &dirty) && dirty);
    PebrelCell cells[12];
    uint8_t row[64];
    ptrdiff_t required = pebrel_vt_row(vt, cells, 12, NULL, 0);
    assert(required > 6 && required < 64);
    assert(pebrel_vt_row(vt, cells, 12, row, sizeof(row)) == required);
    assert(cells[0].width == 2 && cells[1].width == 0 && cells[2].length == 3);
    assert(cells[0].foreground == ansi[1] && (cells[0].flags & PEBREL_BOLD));
    assert(!memcmp(row, "中e\xcc\x81", 6));
    pebrel_vt_end(vt);
    assert(pebrel_vt_resize(vt, 16, 4, 8, 16));
    assert(feed(vt, "\033[?2004h", reply, sizeof(reply)) == 0);
    char paste[] = "ok";
    assert(pebrel_vt_paste(vt, paste, 2, reply, sizeof(reply)) == 14);
    assert(!memcmp(reply, "\033[200~ok\033[201~", 14));
    assert(pebrel_vt_key(vt, 6, 2, "c", 1, 'c', reply, sizeof(reply)) == 1 && reply[0] == 3);
    for (int i=0; i<16; ++i) assert(feed(vt, "line\r\n", reply, sizeof(reply)) == 0);
    assert(pebrel_vt_wheel(vt, -20, 0, 0, reply, sizeof(reply)) == 0);
    assert(pebrel_vt_begin(vt, &frame) && frame.total == 9 && frame.offset == 0);
    pebrel_vt_end(vt);
    assert(feed(vt, "\033[?1049h\033[?1000h\033[?1006h", reply, sizeof(reply)) == 0);
    assert(pebrel_vt_wheel(vt, -1, 0, 0, reply, sizeof(reply)) > 0);
    assert(!memcmp(reply, "\033[<64;", 6));
    pebrel_vt_free(vt);
    puts("VT grid, reply, style, history, key, mouse and paste passed");
}
