#pragma once
#include "pebrel_vt.h"
#include <ghostty/vt.h>
struct PebrelVT {
    GhosttyTerminal terminal;
    GhosttyRenderState render;
    GhosttyRenderStateRowIterator rows;
    GhosttyRenderStateRowCells cells;
    GhosttyRenderStateColors colors;
    GhosttyKeyEncoder keys;
    GhosttyKeyEvent key;
    GhosttyMouseEncoder mouse;
    GhosttyMouseEvent event;
    GhosttyBuffer reply;
    bool overflow, full;
    uint16_t columns, height, cell_w, cell_h, history;
};
static inline uint32_t packed(GhosttyColorRgb c) {
    return ((uint32_t)c.r << 16) | ((uint32_t)c.g << 8) | c.b;
}
GhosttyTerminalScrollbar pebrel_vt_scrollbar(PebrelVT *s);
