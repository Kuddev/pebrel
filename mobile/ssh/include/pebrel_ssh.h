#ifndef PEBREL_SSH_H
#define PEBREL_SSH_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Each operation returns NULL on success or an owned fixed error code.
 * Release every returned string (including errors) with pebrel_string_free.
 * Provide valid, non-overlapping buffers and output pointers for each call.
 * Input strings are NUL-terminated UTF-8; a NULL byte pointer is valid at length zero.
 * Calls may block. Close on another thread cancels pending session operations. */
typedef int64_t PebrelHandle;
typedef struct { uint32_t columns, rows, width, height; } PebrelGeometry;
typedef struct { const uint8_t *data; size_t len; } PebrelBytes;
void pebrel_string_free(char *value);
/* The caller owns and clears the credential buffers; Rust zeroizes its copies. */
char *pebrel_ssh_create(const char *host, uint16_t port, const char *user, PebrelBytes password,
                        const char *fingerprint, PebrelBytes private_key, PebrelHandle *out);
char *pebrel_ssh_next_event(PebrelHandle id, char **out);
char *pebrel_ssh_answer_host_key(PebrelHandle id, bool accept);
char *pebrel_ssh_open_shell(PebrelHandle id, PebrelGeometry size);
char *pebrel_ssh_open_exec(PebrelHandle id, const char *command);
char *pebrel_ssh_open_pty_exec(PebrelHandle id, const char *command, PebrelGeometry size);
char *pebrel_ssh_query(PebrelHandle id, const char *command, char **out);
/* count is zero at EOF. Maximum read is 16 KiB, maximum write is 8 KiB. */
char *pebrel_ssh_read(PebrelHandle id, bool stderr_stream, uint8_t *bytes, size_t capacity, size_t *count);
char *pebrel_ssh_write(PebrelHandle id, PebrelBytes bytes);
char *pebrel_ssh_resize(PebrelHandle id, PebrelGeometry size);
char *pebrel_ssh_await_exit(PebrelHandle id, int32_t *out);
char *pebrel_ssh_sftp(PebrelHandle id, const char *request, char **out);
char *pebrel_ssh_close(PebrelHandle id);

char *pebrel_link_create(PebrelBytes host, PebrelBytes secret, PebrelBytes context, PebrelHandle *out);
/* hello needs 128 bytes. seal needs len + 28 * ceil(len / 61440) bytes, and
 * writes [u32 big-endian packet size | packet] records. open needs 2 MiB
 * and returns count zero until the last packet of a message arrives.
 * Capacity checks precede operations that advance Noise nonces. */
char *pebrel_link_hello(PebrelHandle id, uint8_t *out, size_t capacity, size_t *count);
char *pebrel_link_finish(PebrelHandle id, PebrelBytes response, bool *established);
char *pebrel_link_verification_code(PebrelHandle id, char **out);
char *pebrel_link_seal(PebrelHandle id, PebrelBytes input, uint8_t *out, size_t capacity, size_t *count);
char *pebrel_link_open(PebrelHandle id, PebrelBytes packet, uint8_t *out, size_t capacity, size_t *count);
char *pebrel_link_close(PebrelHandle id);
#ifdef __cplusplus
}
#endif
#endif
