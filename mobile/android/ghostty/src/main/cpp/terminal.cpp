#include "bridge.h"
#include <memory>

Terminal::~Terminal() {
    ghostty_key_event_free(event);
    ghostty_key_encoder_free(encoder);
    ghostty_render_state_row_cells_free(cells);
    ghostty_render_state_row_iterator_free(rows);
    ghostty_render_state_free(render);
    ghostty_terminal_free(vt);
}

static void write_reply(GhosttyTerminal, void* userdata, const uint8_t* data, size_t length) {
    auto* state = static_cast<Terminal*>(userdata);
    if (length > 65536 - state->replies.size()) { state->overflow = true; return; }
    state->replies.insert(state->replies.end(), data, data + length);
}

static void title_changed(GhosttyTerminal, void* userdata) {
    static_cast<Terminal*>(userdata)->title_changed = true;
}

extern "C" JNIEXPORT jlong JNICALL JNI_METHOD(create)(JNIEnv* env, jobject, jint cols, jint rows) {
    auto state = std::make_unique<Terminal>();
    if (!checked(env, ghostty_terminal_new(nullptr, &state->vt,
            {static_cast<uint16_t>(cols), static_cast<uint16_t>(rows), 2000})) ||
        !checked(env, ghostty_render_state_new(nullptr, &state->render)) ||
        !checked(env, ghostty_render_state_row_iterator_new(nullptr, &state->rows)) ||
        !checked(env, ghostty_render_state_row_cells_new(nullptr, &state->cells)) ||
        !checked(env, ghostty_key_encoder_new(nullptr, &state->encoder)) ||
        !checked(env, ghostty_key_event_new(nullptr, &state->event))) return 0;
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_USERDATA, state.get());
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_WRITE_PTY, reinterpret_cast<const void*>(write_reply));
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_TITLE_CHANGED, reinterpret_cast<const void*>(title_changed));
    // Images are not rendered by this adapter. Do not retain invisible image payloads.
    size_t no_images = 0;
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT, &no_images);
    return reinterpret_cast<jlong>(state.release());
}

extern "C" JNIEXPORT void JNICALL JNI_METHOD(destroy)(JNIEnv*, jobject, jlong handle) { delete terminal(handle); }

extern "C" JNIEXPORT jbyteArray JNICALL JNI_METHOD(feed)(JNIEnv* env, jobject, jlong handle, jbyteArray input, jint count) {
    auto* state = terminal(handle);
    std::vector<uint8_t> buffer(count);
    env->GetByteArrayRegion(input, 0, count, reinterpret_cast<jbyte*>(buffer.data()));
    if (env->ExceptionCheck()) return nullptr;
    state->replies.clear();
    state->overflow = false;
    ghostty_terminal_vt_write(state->vt, buffer.data(), buffer.size());
    if (state->overflow) {
        env->ThrowNew(env->FindClass("java/io/IOException"), "Terminal response exceeded bounded queue");
        return nullptr;
    }
    return bytes(env, state->replies.data(), state->replies.size());
}

extern "C" JNIEXPORT jbyteArray JNICALL JNI_METHOD(title)(JNIEnv* env, jobject, jlong handle) {
    auto* state = terminal(handle);
    if (!state->title_changed) return nullptr;
    state->title_changed = false;
    GhosttyString title{};
    ghostty_terminal_get(state->vt, GHOSTTY_TERMINAL_DATA_TITLE, &title);
    return bytes(env, title.ptr, std::min(title.len, size_t{512}));
}

extern "C" JNIEXPORT void JNICALL JNI_METHOD(resize)(JNIEnv* env, jobject, jlong handle, jint cols, jint rows, jint cw, jint ch) {
    auto* state = terminal(handle);
    checked(env, ghostty_terminal_resize(state->vt, cols, rows, cw, ch));
    state->force = true;
}

extern "C" JNIEXPORT void JNICALL JNI_METHOD(scroll)(JNIEnv*, jobject, jlong handle, jint delta) {
    auto* state = terminal(handle);
    GhosttyTerminalScrollViewport value{};
    value.tag = delta == INT32_MAX ? GHOSTTY_SCROLL_VIEWPORT_BOTTOM : GHOSTTY_SCROLL_VIEWPORT_DELTA;
    value.value.delta = delta;
    ghostty_terminal_scroll_viewport(state->vt, value);
    state->force = true;
}

extern "C" JNIEXPORT void JNICALL JNI_METHOD(colors)(JNIEnv* env, jobject, jlong handle, jintArray input) {
    auto* state = terminal(handle);
    jint values[19];
    env->GetIntArrayRegion(input, 0, 19, values);
    if (env->ExceptionCheck()) return;
    auto fg = rgb(values[0]), bg = rgb(values[1]), cursor = rgb(values[2]);
    GhosttyColorRgb palette[256];
    ghostty_terminal_get(state->vt, GHOSTTY_TERMINAL_DATA_COLOR_PALETTE_DEFAULT, &palette);
    for (int i = 0; i < 16; ++i) palette[i] = rgb(values[i + 3]);
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, &fg);
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, &bg);
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, &cursor);
    ghostty_terminal_set(state->vt, GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, &palette);
    state->force = true;
}

// Android keycodes are deliberately mapped here, never to Ghostty enum ordinals in Kotlin.
static GhosttyKey key(jint code) {
    if (code >= 29 && code <= 54) return static_cast<GhosttyKey>(GHOSTTY_KEY_A + code - 29);
    if (code >= 7 && code <= 16) return static_cast<GhosttyKey>(GHOSTTY_KEY_DIGIT_0 + code - 7);
    if (code >= 131 && code <= 142) return static_cast<GhosttyKey>(GHOSTTY_KEY_F1 + code - 131);
    switch (code) {
        case 19: return GHOSTTY_KEY_ARROW_UP;
        case 20: return GHOSTTY_KEY_ARROW_DOWN;
        case 21: return GHOSTTY_KEY_ARROW_LEFT;
        case 22: return GHOSTTY_KEY_ARROW_RIGHT;
        case 61: return GHOSTTY_KEY_TAB;
        case 62: return GHOSTTY_KEY_SPACE;
        case 66: return GHOSTTY_KEY_ENTER;
        case 67: return GHOSTTY_KEY_BACKSPACE;
        case 92: return GHOSTTY_KEY_PAGE_UP;
        case 93: return GHOSTTY_KEY_PAGE_DOWN;
        case 111: return GHOSTTY_KEY_ESCAPE;
        case 112: return GHOSTTY_KEY_DELETE;
        case 122: return GHOSTTY_KEY_HOME;
        case 123: return GHOSTTY_KEY_END;
        case 124: return GHOSTTY_KEY_INSERT;
        default: return GHOSTTY_KEY_UNIDENTIFIED;
    }
}

extern "C" JNIEXPORT jbyteArray JNICALL JNI_METHOD(key)(JNIEnv* env, jobject, jlong handle,
        jint code, jint mods, jint action, jbyteArray text, jint unshifted) {
    auto* state = terminal(handle);
    std::vector<char> utf8(env->GetArrayLength(text));
    env->GetByteArrayRegion(text, 0, utf8.size(), reinterpret_cast<jbyte*>(utf8.data()));
    if (env->ExceptionCheck()) return nullptr;
    ghostty_key_encoder_setopt_from_terminal(state->encoder, state->vt);
    ghostty_key_event_set_key(state->event, key(code));
    ghostty_key_event_set_action(state->event, static_cast<GhosttyKeyAction>(action));
    ghostty_key_event_set_mods(state->event, mods);
    ghostty_key_event_set_consumed_mods(state->event, 0);
    ghostty_key_event_set_utf8(state->event, utf8.data(), utf8.size());
    ghostty_key_event_set_unshifted_codepoint(state->event, unshifted);
    char output[1024];
    size_t count = 0;
    if (!checked(env, ghostty_key_encoder_encode(state->encoder, state->event, output, sizeof(output), &count))) return nullptr;
    return bytes(env, reinterpret_cast<uint8_t*>(output), count);
}

extern "C" JNIEXPORT jboolean JNICALL JNI_METHOD(bracketedPaste)(JNIEnv*, jobject, jlong handle) {
    bool enabled = false;
    // DEC private mode 2004. GhosttyMode uses the high bit for DEC modes.
    ghostty_terminal_mode_get(terminal(handle)->vt, GHOSTTY_MODE_BRACKETED_PASTE, &enabled);
    return enabled;
}
