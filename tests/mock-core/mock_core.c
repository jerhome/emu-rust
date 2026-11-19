/*
 * Deterministic mock libretro core for emu-rust tests (ADR 0009). Test code only, never shipped.
 *
 * The libretro declarations below were written from the public libretro API specification; libretro.h
 * itself is not vendored. Only the subset used here is declared (layouts must match libretro.h).
 *
 * Behaviour (documented in docs/design/phase2-frontend.md, "Core factice"):
 *  - 64x48 XRGB8888 image computed from a frame counter, a seed (FNV-1a of the content file), the option
 *    `mock_color` and the pad state of port 0. 735 audio frames per run (44100 Hz / 60 fps).
 *  - SAVE_RAM of 2 KiB; while Start is held, the frame counter is written to sram[0..4] and sram[4] = 0xA5.
 *  - Real 64-byte savestate: "MOCK", counter (u32 LE), seed (u32 LE), zero padding.
 *  - Options: v2 by default (1 category, 2 options) with fallback to v1 then to legacy SET_VARIABLES;
 *    -DMOCK_OPTIONS_V1 forces v1, -DMOCK_LEGACY_OPTIONS forces SET_VARIABLES.
 *  - GET_LOG_INTERFACE used (printf with arguments); SET_MESSAGE when the counter is 10.
 *  - load_game writes `mock-was-here.txt` into GET_SAVE_DIRECTORY.
 *  - -DMOCK_NEED_FULLPATH: need_fullpath = true (content is read from the path, data may be NULL).
 */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#if defined(_WIN32)
#define MOCK_API __declspec(dllexport)
#else
#define MOCK_API __attribute__((visibility("default")))
#endif

/* ---- libretro subset ---- */
#define RETRO_API_VERSION 1
#define RETRO_DEVICE_JOYPAD 1
#define RETRO_DEVICE_ID_JOYPAD_START 3
#define RETRO_MEMORY_SAVE_RAM 0
#define RETRO_REGION_NTSC 0
#define RETRO_PIXEL_FORMAT_XRGB8888 1

#define RETRO_ENVIRONMENT_GET_CAN_DUPE 3
#define RETRO_ENVIRONMENT_SET_MESSAGE 6
#define RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY 9
#define RETRO_ENVIRONMENT_SET_PIXEL_FORMAT 10
#define RETRO_ENVIRONMENT_GET_VARIABLE 15
#define RETRO_ENVIRONMENT_SET_VARIABLES 16
#define RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME 18
#define RETRO_ENVIRONMENT_GET_LOG_INTERFACE 27
#define RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY 31
#define RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION 52
#define RETRO_ENVIRONMENT_SET_CORE_OPTIONS 53
#define RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 67

#define RETRO_NUM_CORE_OPTION_VALUES_MAX 128

enum retro_log_level { RETRO_LOG_DEBUG = 0, RETRO_LOG_INFO, RETRO_LOG_WARN, RETRO_LOG_ERROR };

typedef void (*retro_log_printf_t)(enum retro_log_level level, const char *fmt, ...);
struct retro_log_callback { retro_log_printf_t log; };

struct retro_message { const char *msg; unsigned frames; };
struct retro_variable { const char *key; const char *value; };

struct retro_core_option_value { const char *value; const char *label; };

struct retro_core_option_definition {
    const char *key;
    const char *desc;
    const char *info;
    struct retro_core_option_value values[RETRO_NUM_CORE_OPTION_VALUES_MAX];
    const char *default_value;
};

struct retro_core_option_v2_category { const char *key; const char *desc; const char *info; };

struct retro_core_option_v2_definition {
    const char *key;
    const char *desc;
    const char *desc_categorized;
    const char *info;
    const char *info_categorized;
    const char *category_key;
    struct retro_core_option_value values[RETRO_NUM_CORE_OPTION_VALUES_MAX];
    const char *default_value;
};

struct retro_core_options_v2 {
    struct retro_core_option_v2_category *categories;
    struct retro_core_option_v2_definition *definitions;
};

struct retro_game_geometry {
    unsigned base_width, base_height, max_width, max_height;
    float aspect_ratio;
};
struct retro_system_timing { double fps, sample_rate; };
struct retro_system_av_info { struct retro_game_geometry geometry; struct retro_system_timing timing; };

struct retro_system_info {
    const char *library_name;
    const char *library_version;
    const char *valid_extensions;
    bool need_fullpath;
    bool block_extract;
};

struct retro_game_info { const char *path; const void *data; size_t size; const void *meta; };

typedef bool (*retro_environment_t)(unsigned cmd, void *data);
typedef void (*retro_video_refresh_t)(const void *data, unsigned width, unsigned height, size_t pitch);
typedef void (*retro_audio_sample_t)(int16_t left, int16_t right);
typedef size_t (*retro_audio_sample_batch_t)(const int16_t *data, size_t frames);
typedef void (*retro_input_poll_t)(void);
typedef int16_t (*retro_input_state_t)(unsigned port, unsigned device, unsigned index, unsigned id);

/* ---- mock state ---- */
#define WIDTH 64u
#define HEIGHT 48u
#define AUDIO_FRAMES 735u
#define SRAM_SIZE 2048u
#define STATE_SIZE 64u

static retro_environment_t env_cb;
static retro_video_refresh_t video_cb;
static retro_audio_sample_batch_t audio_batch_cb;
static retro_input_poll_t poll_cb;
static retro_input_state_t input_cb;
static retro_log_printf_t log_cb;

static uint32_t counter;
static uint32_t seed;
static bool game_loaded;
static uint8_t sram[SRAM_SIZE];
static uint32_t frame_pixels[WIDTH * HEIGHT];
static int16_t audio_buf[AUDIO_FRAMES * 2];

#ifdef MOCK_NEED_FULLPATH
#define NEED_FULLPATH true
#else
#define NEED_FULLPATH false
#endif

/* ---- options ---- */
#define COLOR_VALUES \
    { { "red", "Red" }, { "green", "Green" }, { "blue", "Blue" } }
#define SPEED_VALUES \
    { { "1", "x1" }, { "2", "x2" }, { "3", "x3" } }

#if !defined(MOCK_LEGACY_OPTIONS) && !defined(MOCK_OPTIONS_V1)
static struct retro_core_option_v2_category categories_v2[] = {
    { "video", "Video", "Video related options." },
    { NULL, NULL, NULL },
};
static struct retro_core_option_v2_definition definitions_v2[] = {
    { "mock_color", "Color", "Video color", "Dominant color channel of the image.",
      "Dominant color channel of the image.", "video", COLOR_VALUES, "red" },
    { "mock_speed", "Speed", "Frame speed", "Counter increment per frame.",
      "Counter increment per frame.", "video", SPEED_VALUES, "1" },
    { NULL, NULL, NULL, NULL, NULL, NULL, { { NULL, NULL } }, NULL },
};
static struct retro_core_options_v2 options_v2 = { categories_v2, definitions_v2 };
#endif

#if !defined(MOCK_LEGACY_OPTIONS)
static struct retro_core_option_definition definitions_v1[] = {
    { "mock_color", "Color", "Dominant color channel of the image.", COLOR_VALUES, "red" },
    { "mock_speed", "Speed", "Counter increment per frame.", SPEED_VALUES, "1" },
    { NULL, NULL, NULL, { { NULL, NULL } }, NULL },
};
#endif

static void declare_options(void) {
#if defined(MOCK_LEGACY_OPTIONS)
    static const struct retro_variable vars[] = {
        { "mock_color", "Color; red|green|blue" },
        { "mock_speed", "Speed; 1|2|3" },
        { NULL, NULL },
    };
    env_cb(RETRO_ENVIRONMENT_SET_VARIABLES, (void *)vars);
#else
#if !defined(MOCK_OPTIONS_V1)
    unsigned version = 0;
    if (!env_cb(RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION, &version)) version = 0;
    if (version >= 2 && env_cb(RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2, &options_v2)) return;
#endif
    if (env_cb(RETRO_ENVIRONMENT_SET_CORE_OPTIONS, definitions_v1)) return;
    {
        static const struct retro_variable vars[] = {
            { "mock_color", "Color; red|green|blue" },
            { "mock_speed", "Speed; 1|2|3" },
            { NULL, NULL },
        };
        env_cb(RETRO_ENVIRONMENT_SET_VARIABLES, (void *)vars);
    }
#endif
}

static const char *get_option(const char *key, const char *fallback) {
    struct retro_variable var = { key, NULL };
    if (env_cb && env_cb(RETRO_ENVIRONMENT_GET_VARIABLE, &var) && var.value) return var.value;
    return fallback;
}

static void put_u32(uint8_t *p, uint32_t v) {
    p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); p[2] = (uint8_t)(v >> 16); p[3] = (uint8_t)(v >> 24);
}
static uint32_t get_u32(const uint8_t *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static uint32_t fnv1a(uint32_t h, const uint8_t *data, size_t len) {
    size_t i;
    for (i = 0; i < len; i++) { h ^= data[i]; h *= 16777619u; }
    return h;
}

/* ---- libretro entry points ---- */
MOCK_API unsigned retro_api_version(void) { return RETRO_API_VERSION; }

MOCK_API void retro_set_environment(retro_environment_t cb) {
    struct retro_log_callback log;
    bool no_game = false;
    env_cb = cb;
    env_cb(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, &no_game);
    log.log = NULL;
    if (env_cb(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, &log)) log_cb = log.log;
    declare_options();
}
MOCK_API void retro_set_video_refresh(retro_video_refresh_t cb) { video_cb = cb; }
MOCK_API void retro_set_audio_sample(retro_audio_sample_t cb) { (void)cb; }
MOCK_API void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb) { audio_batch_cb = cb; }
MOCK_API void retro_set_input_poll(retro_input_poll_t cb) { poll_cb = cb; }
MOCK_API void retro_set_input_state(retro_input_state_t cb) { input_cb = cb; }

MOCK_API void retro_init(void) {
    counter = 0;
    seed = 0;
    memset(sram, 0, sizeof sram);
}
MOCK_API void retro_deinit(void) { log_cb = NULL; }

MOCK_API void retro_get_system_info(struct retro_system_info *info) {
    memset(info, 0, sizeof *info);
    info->library_name = "Mock Core";
    info->library_version = "1.0";
    info->valid_extensions = "bin|md|mock";
    info->need_fullpath = NEED_FULLPATH;
    info->block_extract = false;
}

MOCK_API void retro_get_system_av_info(struct retro_system_av_info *info) {
    memset(info, 0, sizeof *info);
    info->geometry.base_width = WIDTH;
    info->geometry.base_height = HEIGHT;
    info->geometry.max_width = WIDTH;
    info->geometry.max_height = HEIGHT;
    info->geometry.aspect_ratio = (float)WIDTH / (float)HEIGHT;
    info->timing.fps = 60.0;
    info->timing.sample_rate = 44100.0;
}

MOCK_API void retro_set_controller_port_device(unsigned port, unsigned device) { (void)port; (void)device; }

MOCK_API bool retro_load_game(const struct retro_game_info *game) {
    int fmt = RETRO_PIXEL_FORMAT_XRGB8888;
    const char *save_dir = NULL;
    uint32_t h = 2166136261u;
    size_t total = 0;

    if (!game) return false;
    if (!env_cb(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &fmt)) return false;

    if (game->data) {
        h = fnv1a(h, (const uint8_t *)game->data, game->size);
        total = game->size;
    } else if (game->path) {
        /* need_fullpath: read the file ourselves so the seed matches the in-memory variant. */
        uint8_t buf[4096];
        size_t n;
        FILE *f = fopen(game->path, "rb");
        if (!f) return false;
        while ((n = fread(buf, 1, sizeof buf, f)) > 0) { h = fnv1a(h, buf, n); total += n; }
        fclose(f);
    } else {
        return false;
    }
    seed = h;
    counter = 0;
    game_loaded = true;

    if (log_cb) log_cb(RETRO_LOG_INFO, "mock core: loaded %s (%u bytes, seed %08x)\n",
                       game->path ? game->path : "<memory>", (unsigned)total, (unsigned)seed);

    if (env_cb(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, &save_dir) && save_dir) {
        char path[1024];
        FILE *f;
        /* Fixed format, bounded by snprintf; a truncated path simply fails to open. */
        snprintf(path, sizeof path, "%s/mock-was-here.txt", save_dir);
        f = fopen(path, "wb");
        if (f) { fputs("mock\n", f); fclose(f); }
    }
    return true;
}

MOCK_API bool retro_load_game_special(unsigned type, const struct retro_game_info *info, size_t num) {
    (void)type; (void)info; (void)num;
    return false;
}

MOCK_API void retro_unload_game(void) { game_loaded = false; }

MOCK_API void retro_reset(void) { counter = 0; }

MOCK_API void retro_run(void) {
    unsigned x, y, i;
    unsigned mask = 0;
    const char *color = get_option("mock_color", "red");
    const char *speed = get_option("mock_speed", "1");
    unsigned step = (unsigned)(speed[0] - '0');
    unsigned dominant = !strcmp(color, "green") ? 1u : !strcmp(color, "blue") ? 2u : 0u;
    bool start;

    if (step < 1 || step > 3) step = 1;
    if (!game_loaded) return;

    poll_cb();
    for (i = 0; i < 16; i++)
        if (input_cb(0, RETRO_DEVICE_JOYPAD, 0, i)) mask |= 1u << i;
    start = (mask >> RETRO_DEVICE_ID_JOYPAD_START) & 1u;

    if (counter == 10) {
        struct retro_message msg = { "mock: frame 10", 60 };
        env_cb(RETRO_ENVIRONMENT_SET_MESSAGE, &msg);
        if (log_cb) log_cb(RETRO_LOG_WARN, "mock core: reached frame %u (color=%s speed=%u)\n",
                           (unsigned)counter, color, step);
    }
    if (start) {
        put_u32(sram, counter);
        sram[4] = 0xA5;
    }

    for (y = 0; y < HEIGHT; y++) {
        for (x = 0; x < WIDTH; x++) {
            uint32_t v = (x * 4u + counter * 2u + seed) & 0xFFu;
            uint32_t w = ((y * 5u + counter * 3u + (seed >> 8)) & 0xFFu) >> 2;
            uint32_t ch[3] = { w, w, w };
            uint32_t px;
            ch[dominant] = v;
            px = (ch[0] << 16) | (ch[1] << 8) | ch[2];
            px ^= (mask * 0x00010101u) & 0x00FFFFFFu;
            frame_pixels[y * WIDTH + x] = px;
        }
    }
    video_cb(frame_pixels, WIDTH, HEIGHT, WIDTH * sizeof(uint32_t));

    for (i = 0; i < AUDIO_FRAMES; i++) {
        uint32_t n = counter * AUDIO_FRAMES + i;
        int16_t s = (int16_t)(((n * 220u) % 4096u) - 2048);
        audio_buf[2 * i] = s;
        audio_buf[2 * i + 1] = (int16_t)(-s);
    }
    audio_batch_cb(audio_buf, AUDIO_FRAMES);

    counter += step;
}

MOCK_API size_t retro_serialize_size(void) { return STATE_SIZE; }

MOCK_API bool retro_serialize(void *data, size_t size) {
    uint8_t *p = (uint8_t *)data;
    if (!data || size < STATE_SIZE) return false;
    memset(p, 0, STATE_SIZE);
    memcpy(p, "MOCK", 4);
    put_u32(p + 4, counter);
    put_u32(p + 8, seed);
    return true;
}

MOCK_API bool retro_unserialize(const void *data, size_t size) {
    const uint8_t *p = (const uint8_t *)data;
    if (!data || size < STATE_SIZE || memcmp(p, "MOCK", 4) != 0) return false;
    counter = get_u32(p + 4);
    seed = get_u32(p + 8);
    return true;
}

MOCK_API void retro_cheat_reset(void) {}
MOCK_API void retro_cheat_set(unsigned index, bool enabled, const char *code) {
    (void)index; (void)enabled; (void)code;
}

MOCK_API unsigned retro_get_region(void) { return RETRO_REGION_NTSC; }

MOCK_API void *retro_get_memory_data(unsigned id) { return id == RETRO_MEMORY_SAVE_RAM ? sram : NULL; }
MOCK_API size_t retro_get_memory_size(unsigned id) { return id == RETRO_MEMORY_SAVE_RAM ? SRAM_SIZE : 0; }
