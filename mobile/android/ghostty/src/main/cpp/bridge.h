#pragma once
#include <jni.h>
#include <ghostty/vt.h>
#include <algorithm>
#include <cstdint>
#include <string>
#include <vector>

// All access is serialized by GhosttyCore. No Java/global references outlive a call.
struct Terminal {
    GhosttyTerminal vt = nullptr;
    GhosttyRenderState render = nullptr;
    GhosttyRenderStateRowIterator rows = nullptr;
    GhosttyRenderStateRowCells cells = nullptr;
    GhosttyKeyEncoder encoder = nullptr;
    GhosttyKeyEvent event = nullptr;
    std::vector<uint8_t> replies;
    bool overflow = false;
    bool force = true;
    bool title_changed = false;
    ~Terminal();
};

inline Terminal* terminal(jlong handle) { return reinterpret_cast<Terminal*>(handle); }
inline bool checked(JNIEnv* env, GhosttyResult result) {
    if (result == GHOSTTY_SUCCESS) return true;
    env->ThrowNew(env->FindClass("java/lang/IllegalStateException"), "Ghostty operation failed");
    return false;
}
inline jbyteArray bytes(JNIEnv* env, const uint8_t* data, size_t count) {
    auto result = env->NewByteArray(static_cast<jsize>(count));
    if (result && count) env->SetByteArrayRegion(result, 0, count, reinterpret_cast<const jbyte*>(data));
    return result;
}
inline jint argb(GhosttyColorRgb color) { return 0xff000000u | (color.r << 16) | (color.g << 8) | color.b; }
inline GhosttyColorRgb rgb(jint value) {
    return {static_cast<uint8_t>(value >> 16), static_cast<uint8_t>(value >> 8), static_cast<uint8_t>(value)};
}
#define JNI_METHOD(name) Java_io_github_kuddev_pebrel_terminal_NativeBridge_##name
