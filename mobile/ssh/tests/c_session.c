#include "pebrel_ssh.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(char *error) {
    if (error) {
        fprintf(stderr, "SSH operation failed: %s\n", error);
        pebrel_string_free(error);
        exit(1);
    }
}

int main(int argc, char **argv) {
    assert(argc == 5);
    FILE *file = fopen(argv[3], "rb");
    assert(file);
    uint8_t key[65537];
    size_t length = fread(key, 1, sizeof(key), file);
    assert(length > 0 && length < sizeof(key));
    fclose(file);
    PebrelHandle id = 0;
    check(pebrel_ssh_create("127.0.0.1", atoi(argv[1]), argv[2], (PebrelBytes){0}, "",
                            (PebrelBytes){key, length}, &id));
    for (size_t i = 0; i < length; ++i) ((volatile uint8_t *)key)[i] = 0;
    bool verified = false;
    for (;;) {
        char *event = NULL;
        check(pebrel_ssh_next_event(id, &event));
        bool connected = strcmp(event, "connected") == 0;
        if (strncmp(event, "verify:", 7) == 0) {
            assert(strcmp(event+7, argv[4]) == 0);
            check(pebrel_ssh_answer_host_key(id, true));
            verified = true;
        } else if (strncmp(event, "error:", 6) == 0) {
            fprintf(stderr, "%s\n", event);
            return 1;
        }
        pebrel_string_free(event);
        if (connected) break;
    }
    assert(verified);
    check(pebrel_ssh_open_shell(id, (PebrelGeometry){80, 24, 640, 384}));
    const char command[] = "printf 'PEBREL_%s_RESULT\\n' ABI; exit 0\n";
    check(pebrel_ssh_write(id, (PebrelBytes){(const uint8_t *)command, sizeof(command)-1}));
    char output[16385] = {0};
    size_t used = 0;
    for (;;) {
        size_t n = 0;
        check(pebrel_ssh_read(id, false, (uint8_t *)output+used, sizeof(output)-1-used, &n));
        if (!n) break;
        used += n;
        assert(used < sizeof(output)-1);
    }
    assert(strstr(output, "PEBREL_ABI_RESULT"));
    int32_t status = -1;
    check(pebrel_ssh_await_exit(id, &status));
    assert(status == 0);
    check(pebrel_ssh_close(id));
    char *closed = pebrel_ssh_resize(id, (PebrelGeometry){80, 24, 640, 384});
    assert(closed && strcmp(closed, "CLOSED") == 0);
    pebrel_string_free(closed);
    puts("OpenSSH C ABI shell round trip passed");
}
