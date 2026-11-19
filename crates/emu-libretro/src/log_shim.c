/*
 * libretro `retro_log_printf_t` implementation (ADR 0009).
 *
 * Rust stable cannot define a variadic function, so this is the only C in emu-libretro: format the message into
 * a stack buffer and hand it to the Rust sink, which emits a `tracing` event. Callable from any thread and at any
 * time: it touches no global state.
 */
#include <stdarg.h>
#include <stdio.h>

/* Defined in src/log.rs. `msg` is NUL-terminated and only valid during the call. */
extern void emu_libretro_log_sink(int level, const char *msg, int truncated);

/* Includes the terminating NUL: messages longer than 1023 bytes are truncated. */
#define EMU_LIBRETRO_LOG_BUFFER 1024

/* `level` is `enum retro_log_level` (int-sized). The format string comes from the core, never from this file. */
void emu_libretro_log_printf(int level, const char *fmt, ...)
{
    char buffer[EMU_LIBRETRO_LOG_BUFFER];
    va_list args;
    int written;

    if (fmt == NULL) {
        return;
    }

    va_start(args, fmt);
    /* C99 vsnprintf writes at most sizeof(buffer) bytes including the NUL and returns the untruncated length. */
    written = vsnprintf(buffer, sizeof(buffer), fmt, args);
    va_end(args);

    if (written < 0) {
        /* Encoding error: the buffer content is unspecified, do not forward it. */
        emu_libretro_log_sink(level, "<libretro log message could not be formatted>", 0);
        return;
    }
    /* Defensive: pre-C99 runtimes may leave the buffer unterminated on truncation. */
    buffer[sizeof(buffer) - 1] = '\0';
    emu_libretro_log_sink(level, buffer, written >= (int)sizeof(buffer));
}
