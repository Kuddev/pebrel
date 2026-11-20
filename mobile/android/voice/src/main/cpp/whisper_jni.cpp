#include <jni.h>
#include <whisper.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

namespace {
struct Job { std::atomic<bool> cancelled{false}; };
std::mutex registry_mutex;
std::unordered_map<jlong, std::shared_ptr<Job>> jobs;
jlong next_id = 1;
std::once_flag logging;

std::shared_ptr<Job> find(jlong id) {
    std::lock_guard<std::mutex> lock(registry_mutex);
    const auto found = jobs.find(id);
    return found == jobs.end() ? nullptr : found->second;
}
void fail(JNIEnv* env) {
    // Never include model paths, audio, or recognized text in diagnostics.
    env->ThrowNew(env->FindClass("java/io/IOException"), "local_transcription_failed");
}
bool abort_inference(void* data) { return static_cast<Job*>(data)->cancelled.load(); }
void discard_log(enum ggml_log_level, const char*, void*) {}
struct Samples {
    std::vector<float> value;
    ~Samples() { std::fill(value.begin(), value.end(), 0.f); }
};
}

extern "C" JNIEXPORT jlong JNICALL
Java_io_github_kuddev_pebrel_voice_NativeWhisper_create(JNIEnv* env, jobject) {
    try {
        std::lock_guard<std::mutex> lock(registry_mutex);
        if (jobs.size() >= 2) throw std::runtime_error("busy");
        const auto id = next_id++;
        jobs.emplace(id, std::make_shared<Job>());
        return id;
    } catch (...) { fail(env); return 0; }
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_kuddev_pebrel_voice_NativeWhisper_cancel(JNIEnv*, jobject, jlong id) {
    if (auto job = find(id)) job->cancelled = true;
}

extern "C" JNIEXPORT void JNICALL
Java_io_github_kuddev_pebrel_voice_NativeWhisper_destroy(JNIEnv*, jobject, jlong id) {
    std::lock_guard<std::mutex> lock(registry_mutex);
    const auto found = jobs.find(id);
    if (found != jobs.end()) { found->second->cancelled = true; jobs.erase(found); }
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_io_github_kuddev_pebrel_voice_NativeWhisper_transcribe(
        JNIEnv* env, jobject, jlong id, jstring path, jfloatArray input) {
    try {
        auto job = find(id);
        if (!job || !path || !input) throw std::runtime_error("invalid_job");
        if (job->cancelled) return env->NewByteArray(0);
        const auto size = env->GetArrayLength(input);
        if (size < 4800 || size > 960000) throw std::runtime_error("invalid_audio");
        Samples samples;
        samples.value.resize(size);
        env->GetFloatArrayRegion(input, 0, size, samples.value.data());
        if (env->ExceptionCheck()) return nullptr;
        for (float value : samples.value) {
            if (!std::isfinite(value) || value < -1.f || value > 1.f) throw std::runtime_error("invalid_audio");
        }
        const char* utf = env->GetStringUTFChars(path, nullptr);
        if (!utf) return nullptr;
        std::string model;
        try { model.assign(utf); } catch (...) { env->ReleaseStringUTFChars(path, utf); throw; }
        env->ReleaseStringUTFChars(path, utf);
        std::call_once(logging, [] { whisper_log_set(discard_log, nullptr); });
        auto context_params = whisper_context_default_params();
        context_params.use_gpu = false;
        std::unique_ptr<whisper_context, decltype(&whisper_free)> context(
            whisper_init_from_file_with_params(model.c_str(), context_params), whisper_free);
        if (job->cancelled) return env->NewByteArray(0);
        if (!context) throw std::runtime_error("model_unavailable");
        auto params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
        params.n_threads = std::max(1u, std::min(4u, std::thread::hardware_concurrency()));
        params.language = "auto";
        params.translate = false;
        params.no_context = true;
        params.no_timestamps = true;
        params.print_progress = false;
        params.print_realtime = false;
        params.print_timestamps = false;
        params.print_special = false;
        params.suppress_blank = true;
        params.suppress_nst = true;
        params.abort_callback = abort_inference;
        params.abort_callback_user_data = job.get();
        const auto result = whisper_full(context.get(), params, samples.value.data(), size);
        if (job->cancelled) return env->NewByteArray(0);
        if (result != 0) throw std::runtime_error("inference_failed");
        std::string text;
        for (int i = 0; i < whisper_full_n_segments(context.get()); ++i) {
            if (whisper_full_get_segment_no_speech_prob(context.get(), i) > .8f) continue;
            const auto segment = whisper_full_get_segment_text(context.get(), i);
            if (segment) text += segment;
            if (text.size() > 32768) throw std::runtime_error("transcript_too_long");
        }
        auto output = env->NewByteArray(text.size());
        if (output) env->SetByteArrayRegion(output, 0, text.size(), reinterpret_cast<const jbyte*>(text.data()));
        return output;
    } catch (...) { fail(env); return nullptr; }
}
