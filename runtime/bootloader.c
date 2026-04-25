/*
 * llm_os v0.01 bootloader.
 *
 * One-shot. Forks llama-server, sends a boot prompt to the OpenAI-compatible
 * /v1/completions endpoint, waits for the model to emit `<|ready|>`, prints
 * the server PID to stdout, and exits. The long-running session is owned by
 * the I/O daemon (`iod.rs`, Week 2 D1) which talks to the same server.
 *
 *   stdout: a single line `<server-pid>` on success.
 *   stderr: human-readable status; redirect to log.
 *   exit:   0 on ready, 1 on any failure (server killed before exit).
 *
 * The bootloader does NOT set `--grammar` on the server. The grammar is
 * applied per-request by the daemon (llama-server supports `grammar` in
 * the `/v1/completions` JSON body). This avoids needing to add a `<|ready|>`
 * production to `isa.gbnf` solely for the boot sequence — see refinement
 * §3 Week 2.
 *
 * POSIX-only. Tested target: Raspberry Pi OS aarch64 / Linux x86_64.
 * Does not compile on Windows (uses fork/execvp and POSIX sockets).
 *
 * Build:
 *     cc -O2 -Wall -Wextra -o runtime/bootloader runtime/bootloader.c
 *
 * Usage:
 *     runtime/bootloader \
 *         --model /path/to/qwen3-2b-q4.gguf \
 *         --port  8080 \
 *         [--ctx-size 32768] \
 *         [--boot-timeout 30] \
 *         [--server-bin llama-server]
 */

#define _POSIX_C_SOURCE 200809L

#include <arpa/inet.h>
#include <errno.h>
#include <getopt.h>
#include <netinet/in.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define READY_SENTINEL "<|ready|>"
#define BOOT_PROMPT \
    "You are LLM-OS v0.01. " \
    "Your job right now is exactly one thing: emit the literal text " \
    READY_SENTINEL " and stop. Do not explain. Do not preface. " \
    "Output: " READY_SENTINEL

static volatile pid_t g_server_pid = 0;

static void logf(const char *fmt, ...) {
    va_list ap;
    fprintf(stderr, "[bootloader] ");
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
}

static void kill_server(void) {
    if (g_server_pid > 0) {
        kill(g_server_pid, SIGTERM);
        // Brief grace period; if it doesn't go, escalate.
        struct timespec ts = {.tv_sec = 0, .tv_nsec = 250 * 1000 * 1000};
        nanosleep(&ts, NULL);
        kill(g_server_pid, SIGKILL);
        waitpid(g_server_pid, NULL, WNOHANG);
        g_server_pid = 0;
    }
}

static int spawn_server(const char *bin, const char *model, int port, int ctx_size) {
    pid_t pid = fork();
    if (pid < 0) {
        logf("fork failed: %s", strerror(errno));
        return -1;
    }
    if (pid == 0) {
        // Child: become the llama-server.
        char port_s[16], ctx_s[16];
        snprintf(port_s, sizeof port_s, "%d", port);
        snprintf(ctx_s, sizeof ctx_s, "%d", ctx_size);
        char *argv[] = {
            (char *)bin,
            (char *)"-m", (char *)model,
            (char *)"-c", ctx_s,
            (char *)"--port", port_s,
            (char *)"--host", (char *)"127.0.0.1",
            (char *)"--no-webui",
            NULL,
        };
        execvp(bin, argv);
        // Only reachable on exec failure.
        fprintf(stderr, "[bootloader] execvp(%s) failed: %s\n", bin, strerror(errno));
        _exit(127);
    }
    g_server_pid = pid;
    return 0;
}

// Block-connect to 127.0.0.1:port, returning a connected fd or -1.
static int dial(int port) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    struct sockaddr_in sa = {0};
    sa.sin_family = AF_INET;
    sa.sin_port = htons(port);
    sa.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (connect(fd, (struct sockaddr *)&sa, sizeof sa) < 0) {
        close(fd);
        return -1;
    }
    return fd;
}

// Wait until the server accepts a TCP connection on port. Returns 0 on ready,
// -1 on timeout. Polls at 200ms intervals.
static int wait_for_listen(int port, int timeout_sec) {
    struct timespec interval = {.tv_sec = 0, .tv_nsec = 200 * 1000 * 1000};
    int tries = (timeout_sec * 1000) / 200;
    for (int i = 0; i < tries; i++) {
        // Reap zombie if server crashed.
        int status;
        pid_t r = waitpid(g_server_pid, &status, WNOHANG);
        if (r == g_server_pid) {
            logf("server exited before binding (status=%d)", status);
            g_server_pid = 0;
            return -1;
        }
        int fd = dial(port);
        if (fd >= 0) {
            close(fd);
            return 0;
        }
        nanosleep(&interval, NULL);
    }
    return -1;
}

static ssize_t write_all(int fd, const char *buf, size_t len) {
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, buf + off, len - off);
        if (n < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        off += (size_t)n;
    }
    return (ssize_t)off;
}

// Send the boot prompt and stream-scan the response for READY_SENTINEL.
// Returns 0 if found, -1 on any failure (timeout, parse, network).
static int send_boot_and_wait_ready(int port, int boot_timeout) {
    int fd = dial(port);
    if (fd < 0) {
        logf("dial(%d) failed: %s", port, strerror(errno));
        return -1;
    }

    // Recv timeout — overall bound on how long we'll wait for the model.
    struct timeval rt = {.tv_sec = boot_timeout, .tv_usec = 0};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &rt, sizeof rt);

    // Build JSON body. Keep it small enough to fit in one allocation.
    // `n_predict: 32` caps generation; the stop sequence terminates earlier.
    char body[1024];
    int body_len = snprintf(body, sizeof body,
        "{\"prompt\":\"%s\","
        "\"stream\":true,"
        "\"n_predict\":32,"
        "\"temperature\":0.0,"
        "\"stop\":[\"%s\"]}",
        BOOT_PROMPT, READY_SENTINEL);
    if (body_len < 0 || (size_t)body_len >= sizeof body) {
        logf("boot prompt body did not fit");
        close(fd);
        return -1;
    }

    char headers[512];
    int hdr_len = snprintf(headers, sizeof headers,
        "POST /v1/completions HTTP/1.1\r\n"
        "Host: 127.0.0.1\r\n"
        "Content-Type: application/json\r\n"
        "Content-Length: %d\r\n"
        "Connection: close\r\n"
        "\r\n",
        body_len);
    if (hdr_len < 0 || (size_t)hdr_len >= sizeof headers) {
        logf("headers did not fit");
        close(fd);
        return -1;
    }

    if (write_all(fd, headers, (size_t)hdr_len) < 0
        || write_all(fd, body, (size_t)body_len) < 0) {
        logf("send failed: %s", strerror(errno));
        close(fd);
        return -1;
    }

    // Stream-scan the response body for READY_SENTINEL. We don't bother
    // parsing SSE event boundaries — the sentinel will appear in the
    // streamed JSON `content` field whether the server emits it as part
    // of the model output or as the trigger that fired the stop sequence.
    // This is a pragmatic v0.01 choice; refine if a future llama-server
    // strips the stop token from the stream.
    char buf[4096];
    size_t scan_keep = strlen(READY_SENTINEL) - 1;
    char tail[64] = {0};
    size_t tail_len = 0;

    for (;;) {
        ssize_t n = read(fd, buf, sizeof buf);
        if (n < 0) {
            if (errno == EINTR) continue;
            logf("read failed: %s", strerror(errno));
            close(fd);
            return -1;
        }
        if (n == 0) {
            // Connection closed without sentinel.
            logf("server closed stream without %s", READY_SENTINEL);
            close(fd);
            return -1;
        }
        // Concatenate carry-over tail + new buf, scan, then preserve the
        // last (sentinel_len - 1) bytes for the next iteration so we don't
        // miss a sentinel that straddles a read boundary.
        // For simplicity, allocate a small staging buffer.
        size_t total = tail_len + (size_t)n;
        char *stage = malloc(total + 1);
        if (!stage) {
            logf("malloc failed");
            close(fd);
            return -1;
        }
        memcpy(stage, tail, tail_len);
        memcpy(stage + tail_len, buf, (size_t)n);
        stage[total] = '\0';

        int found = (memmem(stage, total, READY_SENTINEL, strlen(READY_SENTINEL)) != NULL);
        if (found) {
            free(stage);
            close(fd);
            return 0;
        }

        // Carry the trailing scan_keep bytes to catch a straddling sentinel.
        size_t carry = total < scan_keep ? total : scan_keep;
        memcpy(tail, stage + total - carry, carry);
        tail_len = carry;
        free(stage);
    }
}

static void usage(const char *argv0) {
    fprintf(stderr,
        "Usage: %s --model PATH --port N [--ctx-size N] [--boot-timeout SEC] [--server-bin PATH]\n",
        argv0);
}

int main(int argc, char **argv) {
    const char *model = NULL;
    const char *server_bin = "llama-server";
    int port = 0;
    int ctx_size = 32768;
    int boot_timeout = 30;

    static struct option opts[] = {
        {"model",        required_argument, 0, 'm'},
        {"port",         required_argument, 0, 'p'},
        {"ctx-size",     required_argument, 0, 'c'},
        {"boot-timeout", required_argument, 0, 't'},
        {"server-bin",   required_argument, 0, 's'},
        {"help",         no_argument,       0, 'h'},
        {0, 0, 0, 0},
    };
    int idx = 0, c;
    while ((c = getopt_long(argc, argv, "m:p:c:t:s:h", opts, &idx)) != -1) {
        switch (c) {
            case 'm': model = optarg; break;
            case 'p': port = atoi(optarg); break;
            case 'c': ctx_size = atoi(optarg); break;
            case 't': boot_timeout = atoi(optarg); break;
            case 's': server_bin = optarg; break;
            case 'h': usage(argv[0]); return 0;
            default:  usage(argv[0]); return 1;
        }
    }
    if (!model || port <= 0 || port > 65535) {
        usage(argv[0]);
        return 1;
    }

    if (access(model, R_OK) != 0) {
        logf("model not readable: %s", model);
        return 1;
    }

    logf("forking %s for model=%s port=%d ctx=%d", server_bin, model, port, ctx_size);
    if (spawn_server(server_bin, model, port, ctx_size) < 0) return 1;

    if (wait_for_listen(port, boot_timeout) < 0) {
        logf("server did not bind within %ds", boot_timeout);
        kill_server();
        return 1;
    }
    logf("server bound; sending boot prompt");

    if (send_boot_and_wait_ready(port, boot_timeout) < 0) {
        logf("ready handshake failed");
        kill_server();
        return 1;
    }

    logf("ready (server pid=%d, port=%d)", (int)g_server_pid, port);
    // The server stays running for the I/O daemon to take over.
    printf("%d\n", (int)g_server_pid);
    fflush(stdout);
    return 0;
}
