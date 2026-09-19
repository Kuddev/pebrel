#include "bridge.h"
#include <cerrno>
#include <csignal>
#include <cstdlib>
#include <fcntl.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>

static void io_error(JNIEnv* env, const char* message) {
    env->ThrowNew(env->FindClass("java/io/IOException"), message);
}

extern "C" JNIEXPORT jintArray JNICALL JNI_METHOD(ptyOpen)(JNIEnv* env, jobject, jstring directory, jint cols, jint rows) {
    const char* raw = env->GetStringUTFChars(directory, nullptr);
    if (!raw) return nullptr;
    const std::string cwd(raw);
    env->ReleaseStringUTFChars(directory, raw);
    const std::string home = "HOME=" + cwd;
    const std::string temporary = "TMPDIR=" + cwd;
    const char* environment[] = {"PATH=/system/bin:/system/xbin", "TERM=xterm-256color", "COLORTERM=truecolor",
        "LANG=C.UTF-8", home.c_str(), temporary.c_str(), nullptr};
    const char* arguments[] = {"/system/bin/sh", "-i", nullptr};
    int master = posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC);
    char slave_name[128];
    if (master < 0) { io_error(env, "Cannot allocate local PTY"); return nullptr; }
    if (grantpt(master) || unlockpt(master) || ptsname_r(master, slave_name, sizeof(slave_name))) {
        close(master); io_error(env, "Cannot initialize local PTY"); return nullptr;
    }
    struct winsize size{};
    size.ws_col = cols;
    size.ws_row = rows;
    ioctl(master, TIOCSWINSZ, &size);
    const pid_t child = fork();
    if (child < 0) { close(master); io_error(env, "Cannot start local shell"); return nullptr; }
    if (child == 0) {
        // Only async-signal-safe libc/syscalls after fork in the multithreaded JVM.
        close(master);
        if (setsid() < 0) _exit(126);
        int slave = open(slave_name, O_RDWR);
        if (slave < 0 || ioctl(slave, TIOCSCTTY, 0) < 0) _exit(126);
        for (int fd = 0; fd < 3; ++fd) if (dup2(slave, fd) < 0) _exit(126);
        if (slave > 2) close(slave);
        if (chdir(cwd.c_str()) < 0) _exit(126);
        sigset_t mask;
        sigemptyset(&mask);
        sigprocmask(SIG_SETMASK, &mask, nullptr);
        execve(arguments[0], const_cast<char* const*>(arguments), const_cast<char* const*>(environment));
        _exit(127);
    }
    jint values[] = {master, child};
    auto result = env->NewIntArray(2);
    if (!result) { close(master); kill(child, SIGKILL); waitpid(child, nullptr, 0); return nullptr; }
    env->SetIntArrayRegion(result, 0, 2, values);
    return result;
}

extern "C" JNIEXPORT void JNICALL JNI_METHOD(ptyResize)(JNIEnv* env, jobject, jint fd, jint cols, jint rows, jint cw, jint ch) {
    struct winsize size{};
    size.ws_col = cols;
    size.ws_row = rows;
    size.ws_xpixel = std::min(cols * cw, 65535);
    size.ws_ypixel = std::min(rows * ch, 65535);
    if (ioctl(fd, TIOCSWINSZ, &size) < 0) io_error(env, "Cannot resize local PTY");
}

extern "C" JNIEXPORT void JNICALL JNI_METHOD(ptyStop)(JNIEnv*, jobject, jint pid) {
    if (pid > 0) {
        kill(-pid, SIGHUP);
        kill(-pid, SIGKILL);
        kill(pid, SIGKILL); // Also covers close racing with the child's setsid.
    }
}

extern "C" JNIEXPORT jint JNICALL JNI_METHOD(ptyWait)(JNIEnv* env, jobject, jint pid) {
    int status = 0;
    pid_t result;
    // The Kotlin transport holds its PID ownership lock for this nonblocking reap.
    // Close cannot signal a PID after it has been reaped and potentially reused.
    do { result = waitpid(pid, &status, WNOHANG); } while (result < 0 && errno == EINTR);
    if (result == 0) return INT32_MIN;
    if (result < 0) { io_error(env, "Cannot reap local shell"); return -1; }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
