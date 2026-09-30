#include "bridge.h"

static void append_utf16(std::vector<jchar>& target, uint32_t codepoint) {
    if (codepoint > 0x10ffff || (codepoint >= 0xd800 && codepoint <= 0xdfff)) codepoint = 0xfffd;
    if (codepoint <= 0xffff) target.push_back(codepoint);
    else {
        codepoint -= 0x10000;
        target.push_back(0xd800 + (codepoint >> 10));
        target.push_back(0xdc00 + (codepoint & 0x3ff));
    }
}

extern "C" JNIEXPORT void JNICALL JNI_METHOD(render)(JNIEnv* env, jobject, jlong handle,
        jobjectArray output_rows, jintArray metadata) {
    auto* state = terminal(handle);
    if (!checked(env, ghostty_render_state_update(state->render, state->vt))) return;
    uint16_t columns = 0, rows = 0, cx = 0, cy = 0;
    bool visible = false, in_viewport = false;
    GhosttyRenderStateDirty dirty{};
    GhosttyRenderStateCursorVisualStyle cursor_style{};
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_COLS, &columns);
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_ROWS, &rows);
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_DIRTY, &dirty);
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VISIBLE, &visible);
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_HAS_VALUE, &in_viewport);
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_X, &cx);
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_Y, &cy);
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_CURSOR_VISUAL_STYLE, &cursor_style);
    GhosttyRenderStateColors colors{};
    colors.size = sizeof(colors);
    if (!checked(env, ghostty_render_state_colors_get(state->render, &colors))) return;
    if (env->GetArrayLength(output_rows) != rows) {
        env->ThrowNew(env->FindClass("java/lang/IllegalArgumentException"), "Incorrect viewport row count");
        return;
    }
    jint meta[] = {columns, rows, cx, cy, visible && in_viewport, argb(colors.background),
        argb(colors.cursor_has_value ? colors.cursor : colors.foreground), static_cast<jint>(cursor_style)};
    env->SetIntArrayRegion(metadata, 0, 8, meta);
    auto row_class = env->FindClass("io/github/kuddev/pebrel/terminal/TerminalRow");
    if (!row_class) return;
    auto constructor = env->GetMethodID(row_class, "<init>", "(Ljava/lang/String;[I)V");
    if (!constructor) return;
    ghostty_render_state_get(state->render, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &state->rows);
    int y = -1;
    std::vector<uint32_t> codepoints;
    std::vector<jchar> text;
    std::vector<jint> cells(columns * 6);
    while (ghostty_render_state_row_iterator_next(state->rows)) {
        ++y;
        bool row_dirty = false;
        ghostty_render_state_row_get(state->rows, GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY, &row_dirty);
        if (!state->force && dirty != GHOSTTY_RENDER_STATE_DIRTY_FULL && !row_dirty) continue;
        text.clear();
        ghostty_render_state_row_get(state->rows, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &state->cells);
        int x = 0;
        while (ghostty_render_state_row_cells_next(state->cells)) {
            GhosttyCell raw{};
            GhosttyCellWide width{};
            ghostty_render_state_row_cells_get(state->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &raw);
            ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &width);
            auto foreground = colors.foreground, background = colors.background;
            ghostty_render_state_row_cells_get(state->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, &foreground);
            ghostty_render_state_row_cells_get(state->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, &background);
            GhosttyStyle style{};
            style.size = sizeof(style);
            ghostty_render_state_row_cells_get(state->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &style);
            if (style.inverse) std::swap(foreground, background);
            const int cell_width = width == GHOSTTY_CELL_WIDE_WIDE ? 2 :
                (width == GHOSTTY_CELL_WIDE_NARROW ? 1 : 0);
            const int start = text.size();
            uint32_t length = 0;
            ghostty_render_state_row_cells_get(state->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN, &length);
            if (cell_width) {
                if (length) {
                    codepoints.resize(length);
                    ghostty_render_state_row_cells_get(state->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF, codepoints.data());
                    for (auto codepoint : codepoints) append_utf16(text, codepoint);
                } else text.push_back(' ');
            }
            auto* cell = cells.data() + x * 6;
            cell[0] = start;
            cell[1] = text.size() - start;
            cell[2] = cell_width;
            cell[3] = argb(foreground);
            cell[4] = argb(background);
            cell[5] = (style.bold ? 1 : 0) | (style.italic ? 2 : 0) | (style.underline ? 4 : 0) |
                (style.strikethrough ? 8 : 0) | (style.faint ? 16 : 0) | (style.invisible ? 32 : 0);
            ++x;
        }
        auto row_text = env->NewString(text.data(), text.size());
        auto row_cells = env->NewIntArray(cells.size());
        if (!row_text || !row_cells) return;
        env->SetIntArrayRegion(row_cells, 0, cells.size(), cells.data());
        auto row = env->NewObject(row_class, constructor, row_text, row_cells);
        if (!row) return;
        env->SetObjectArrayElement(output_rows, y, row);
        env->DeleteLocalRef(row);
        env->DeleteLocalRef(row_text);
        env->DeleteLocalRef(row_cells);
        bool clean = false;
        ghostty_render_state_row_set(state->rows, GHOSTTY_RENDER_STATE_ROW_OPTION_DIRTY, &clean);
    }
    state->force = false;
    const GhosttyRenderStateDirty clean = GHOSTTY_RENDER_STATE_DIRTY_FALSE;
    ghostty_render_state_set(state->render, GHOSTTY_RENDER_STATE_OPTION_DIRTY, &clean);
}
