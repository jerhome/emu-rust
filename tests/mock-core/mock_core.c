/*
 * Deterministic mock libretro core for emu-rust tests (ADR 0009). Test code only, never shipped.
 *
 * The libretro declarations below were written from the public libretro API specification; libretro.h
 * itself is not vendored. Only the subset used here is declared (layouts must match libretro.h).
 *
 * Extended protocol (v2.0 / Atari ST, all OFF unless an environment variable is set, so the base behaviour
 * below and its image are unchanged). Variables are read with getenv() when the core is set up / loaded:
 *   MOCK_KEYBOARD   contains "cb"   -> SET_KEYBOARD_CALLBACK (12) registered; every event is recorded.
 *                   contains "poll" -> RETRO_DEVICE_KEYBOARD polled each run (ids 0..341).
 *                   "1" = both. Events may arrive between runs (same thread as retro_run assumed).
 *   MOCK_MOUSE      set (any value)  -> RETRO_DEVICE_MOUSE port 0 polled each run (X, Y accumulated).
 *   MOCK_DISK       "basic" -> SET_DISK_CONTROL_INTERFACE (13) only; "ext" -> asks GET_DISK_CONTROL_INTERFACE_VERSION
 *                   (57) and, when >= 1, SET_DISK_CONTROL_EXT_INTERFACE (58), else falls back to 13.
 *                   Up to 8 images; index == num means "no disk"; set_image_index / replace_image_index fail
 *                   unless the tray is ejected (like real cores). On load_game the content (or the images
 *                   listed by a .m3u, relative to its directory) becomes the image list when it is empty.
 *                   set_initial_image (58) is remembered and applied at load_game when the index is valid.
 *   MOCK_DISK_DUMP  set -> after every disk change `mock-disk.txt` is rewritten in GET_SAVE_DIRECTORY:
 *                   lines "interface=basic|ext", "ejected=0|1", "index=N", "num=N", "initial=N|-",
 *                   "image<i>=<path>|<label>" (label = file name without extension; empty for a freshly added slot).
 *   MOCK_DISK_NO_INITIAL  set (with MOCK_DISK=ext) -> the ext interface is registered with set_initial_image = NULL
 *                   (like puae): "initial=-" in the dump and band 3 cell 6 stays 0.
 *   MOCK_ENV_PROBE  set -> retro_set_environment also asks 28 GET_PERF_INTERFACE, 45 GET_VFS_INTERFACE,
 *                   46 GET_LED_INTERFACE and 64 SET_FASTFORWARDING_OVERRIDE (as puae does). The answers are logged
 *                   (INFO "mock core: env 28=0|1 45=.. 46=.. 64=..") and, with MOCK_DISK_DUMP, written at retro_init to
 *                   `mock-env.txt` in GET_SAVE_DIRECTORY ("env28=0|1" ...). No visible effect on the image.
 *   MOCK_NO_GAME    set -> SET_SUPPORT_NO_GAME answers true to the core (value true) and retro_load_game(NULL)
 *                   succeeds (seed 0); without it the core announces false and load_game(NULL) fails.
 * When any of MOCK_KEYBOARD / MOCK_MOUSE / MOCK_DISK / MOCK_NO_GAME is set, rows 0..39 of the 64x48 XRGB8888 image
 * are overwritten by a "register display": 5 bands of 8 rows (y = 8*band .. 8*band+7) split into 8 cells of
 * 8 px (x = 8*cell .. 8*cell+7). A cell holds one byte v as the gray pixel 0x00vvvvvv; read it at the cell
 * centre (x = 8*cell+4, y = 8*band+4). Multi-byte values are little-endian over consecutive cells.
 * Rows 40..47 keep the base pattern (frame counter / seed / pad), so the base fingerprint logic still applies there.
 *   band 0 keyboard callback: 0 events received (mod 256); 1-2 newest keycode (u16); 3 flags of the newest event
 *          (bit0 down, bit1 character != 0); 4-5 newest character (low 16 bits of the UTF-32 value);
 *          6-7 newest key_modifiers (u16).
 *   band 1 keyboard counters / polling: 0 number of keys polled down; 1-2 lowest polled keycode down (u16, 0 if none);
 *          3-4 highest polled keycode down (u16, 0 if none); 5 down events received (mod 256);
 *          6 up events received (mod 256); 7 events with a non-zero character (mod 256).
 *   band 2 mouse: 0-1 cumulative dx (int16, two's complement); 2-3 cumulative dy; 4 buttons held now (bit0 left,
 *          bit1 right, bit2 middle, bit3 wheel up, bit4 wheel down); 5 left presses (rising edges, mod 256);
 *          6 right presses; 7 wheel steps (up + down, mod 256).
 *   band 3 disk: 0 current index (255 when >= 255); 1 num images; 2 ejected (0/1); 3 successful set_image_index
 *          calls (mod 256); 4 low byte of FNV-1a of the current image path (0 when none); 5 same for the label;
 *          6 initial image index + 1 set by the frontend (0 = never); 7 flags: bit0 ext interface registered,
 *          bit1 content loaded with load_game(NULL), bit2 basic interface registered, bit3 replace_image_index
 *          called, bit4 add_image_index called.
 *   band 4 last 4 keycodes received by the callback, newest first, each u16 over 2 cells (cells 0-1 newest ... 6-7
 *          oldest); 0 when absent.
 * All state is reset by retro_init; counters keep running across retro_reset. The events are also logged
 * (INFO "mock core: key ..." / "mock core: disk ...") for the log tests.
 * 16-bit pixel variants quantize the gray cells (values are only exact with the default XRGB8888 build).
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
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#define MOCK_API __declspec(dllexport)
#else
#define MOCK_API __attribute__((visibility("default")))
#endif

/* ---- libretro subset ---- */
#define RETRO_API_VERSION 1
#define RETRO_DEVICE_JOYPAD 1
#define RETRO_DEVICE_MOUSE 2
#define RETRO_DEVICE_KEYBOARD 3
#define RETRO_DEVICE_ID_MOUSE_X 0
#define RETRO_DEVICE_ID_MOUSE_Y 1
#define RETRO_DEVICE_ID_MOUSE_LEFT 2
#define RETRO_DEVICE_ID_MOUSE_RIGHT 3
#define RETRO_DEVICE_ID_MOUSE_WHEELUP 4
#define RETRO_DEVICE_ID_MOUSE_WHEELDOWN 5
#define RETRO_DEVICE_ID_MOUSE_MIDDLE 6
#define RETROK_LAST 342
#define RETRO_DEVICE_ID_JOYPAD_START 3
#define RETRO_MEMORY_SAVE_RAM 0
#define RETRO_REGION_NTSC 0
#define RETRO_PIXEL_FORMAT_0RGB1555 0
#define RETRO_PIXEL_FORMAT_XRGB8888 1
#define RETRO_PIXEL_FORMAT_RGB565 2

#define RETRO_ENVIRONMENT_GET_CAN_DUPE 3
#define RETRO_ENVIRONMENT_SET_MESSAGE 6
#define RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE 13
#define RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK 12
#define RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY 9
#define RETRO_ENVIRONMENT_SET_PIXEL_FORMAT 10
#define RETRO_ENVIRONMENT_GET_VARIABLE 15
#define RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE 17
#define RETRO_ENVIRONMENT_SET_VARIABLES 16
#define RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME 18
#define RETRO_ENVIRONMENT_GET_LOG_INTERFACE 27
#define RETRO_ENVIRONMENT_GET_PERF_INTERFACE 28
#define RETRO_ENVIRONMENT_GET_VFS_INTERFACE 45
#define RETRO_ENVIRONMENT_GET_LED_INTERFACE 46
#define RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE 64
#define RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY 31
#define RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION 52
#define RETRO_ENVIRONMENT_SET_CORE_OPTIONS 53
#define RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 67
#define RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION 57
#define RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE 58
#define RETRO_ENVIRONMENT_SET_VARIABLE 70

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

typedef void (*retro_keyboard_event_t)(bool down, unsigned keycode, uint32_t character, uint16_t key_modifiers);
struct retro_keyboard_callback { retro_keyboard_event_t callback; };

struct retro_disk_control_callback {
    bool (*set_eject_state)(bool ejected);
    bool (*get_eject_state)(void);
    unsigned (*get_image_index)(void);
    bool (*set_image_index)(unsigned index);
    unsigned (*get_num_images)(void);
    bool (*replace_image_index)(unsigned index, const struct retro_game_info *info);
    bool (*add_image_index)(void);
};
struct retro_disk_control_ext_callback {
    bool (*set_eject_state)(bool ejected);
    bool (*get_eject_state)(void);
    unsigned (*get_image_index)(void);
    bool (*set_image_index)(unsigned index);
    unsigned (*get_num_images)(void);
    bool (*replace_image_index)(unsigned index, const struct retro_game_info *info);
    bool (*add_image_index)(void);
    bool (*set_initial_image)(unsigned index, const char *path);
    bool (*get_image_path)(unsigned index, char *path, size_t len);
    bool (*get_image_label)(unsigned index, char *label, size_t len);
};

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

/* -DMOCK_PIXEL_RGB565 / -DMOCK_PIXEL_1555: the same image quantized to 16 bits, rows padded by 8 bytes. */
#if defined(MOCK_PIXEL_RGB565)
#define PIXEL_FORMAT RETRO_PIXEL_FORMAT_RGB565
#define PIXEL_16 1
#elif defined(MOCK_PIXEL_1555)
#define PIXEL_FORMAT RETRO_PIXEL_FORMAT_0RGB1555
#define PIXEL_16 1
#else
#define PIXEL_FORMAT RETRO_PIXEL_FORMAT_XRGB8888
#endif
#ifdef PIXEL_16
#define ROW_PAD_U16 4u
static uint16_t frame_pixels16[(WIDTH + ROW_PAD_U16) * HEIGHT];
#endif

/* -DMOCK_DYNAMIC_OPTIONS: once the counter reaches 20 the core sets mock_color=blue itself (SET_VARIABLE);
 * each time GET_VARIABLE_UPDATE reports a frontend change, a message is emitted. */
#ifdef MOCK_DYNAMIC_OPTIONS
static bool dynamic_done;
#endif

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

/* ---- extended protocol (see header) ---- */
#define MAX_DISKS 8u
#define DISK_PATH_MAX 512u
#define CELL 8u
#define BANDS 5u

static bool kb_cb_on, kb_poll_on, mouse_on, no_game_on, disk_dump, mock_visible;
static int disk_mode; /* 0 off, 1 basic, 2 ext */
static bool disk_ext_registered, disk_basic_registered, loaded_no_game, replace_called, add_called;

static uint8_t kb_events;
static uint8_t kb_down_events, kb_up_events, kb_char_events;
static unsigned kb_ring[4];
static unsigned kb_last_flags, kb_last_mods;
static uint32_t kb_last_char;
static unsigned kb_polled_count, kb_polled_lo, kb_polled_hi;

static int32_t mouse_dx, mouse_dy;
static unsigned mouse_buttons, mouse_left_presses, mouse_right_presses, mouse_wheel_steps;
static bool mouse_prev_left, mouse_prev_right;

static char disk_path[MAX_DISKS][DISK_PATH_MAX];
static char disk_label[MAX_DISKS][DISK_PATH_MAX];
static unsigned disk_num, disk_index, disk_set_calls;
static bool disk_ejected;
static bool initial_set;
static bool no_initial_on, env_probe_on;
static int probe_answer[4]; /* 28, 45, 46, 64 */
static unsigned initial_index;
static char initial_path[DISK_PATH_MAX];

static bool env_flag(const char *name) {
    const char *v = getenv(name);
    return v && v[0];
}

static void read_protocol_env(void) {
    const char *kb = getenv("MOCK_KEYBOARD");
    const char *disk = getenv("MOCK_DISK");
    kb_cb_on = kb && (!strcmp(kb, "1") || strstr(kb, "cb"));
    kb_poll_on = kb && (!strcmp(kb, "1") || strstr(kb, "poll"));
    mouse_on = env_flag("MOCK_MOUSE");
    no_game_on = env_flag("MOCK_NO_GAME");
    disk_dump = env_flag("MOCK_DISK_DUMP");
    no_initial_on = env_flag("MOCK_DISK_NO_INITIAL");
    env_probe_on = env_flag("MOCK_ENV_PROBE");
    disk_mode = !disk ? 0 : !strcmp(disk, "ext") ? 2 : disk[0] ? 1 : 0;
    mock_visible = kb_cb_on || kb_poll_on || mouse_on || no_game_on || disk_mode != 0;
}

static void reset_protocol_state(void) {
    kb_events = kb_down_events = kb_up_events = kb_char_events = 0;
    memset(kb_ring, 0, sizeof kb_ring);
    kb_last_flags = kb_last_mods = 0;
    kb_last_char = 0;
    mouse_dx = mouse_dy = 0;
    mouse_buttons = mouse_left_presses = mouse_right_presses = mouse_wheel_steps = 0;
    mouse_prev_left = mouse_prev_right = false;
    memset(disk_path, 0, sizeof disk_path);
    memset(disk_label, 0, sizeof disk_label);
    disk_num = disk_index = disk_set_calls = 0;
    disk_ejected = false;
    initial_set = false;
    initial_index = 0;
    initial_path[0] = 0;
    loaded_no_game = replace_called = add_called = false;
}

static void mock_keyboard_event(bool down, unsigned keycode, uint32_t character, uint16_t key_modifiers) {
    kb_events++;
    if (down) kb_down_events++; else kb_up_events++;
    if (character) kb_char_events++;
    kb_ring[3] = kb_ring[2]; kb_ring[2] = kb_ring[1]; kb_ring[1] = kb_ring[0]; kb_ring[0] = keycode & 0xFFFFu;
    kb_last_flags = (down ? 1u : 0u) | (character ? 2u : 0u);
    kb_last_char = character;
    kb_last_mods = key_modifiers;
    if (log_cb) log_cb(RETRO_LOG_INFO, "mock core: key %s code=%u char=%u mods=%u\n", down ? "down" : "up", keycode,
                       (unsigned)character, (unsigned)key_modifiers);
}

/* File name without directory and extension, bounded copy. */
static void label_from_path(const char *path, char *out, size_t cap) {
    const char *base = path, *p, *dot;
    size_t n;
    for (p = path; *p; p++)
        if (*p == '/' || *p == '\\') base = p + 1;
    dot = strrchr(base, '.');
    n = dot && dot != base ? (size_t)(dot - base) : strlen(base);
    if (n >= cap) n = cap - 1;
    memcpy(out, base, n);
    out[n] = 0;
}

static void set_disk_slot(unsigned i, const char *path) {
    snprintf(disk_path[i], DISK_PATH_MAX, "%s", path ? path : "");
    label_from_path(disk_path[i], disk_label[i], DISK_PATH_MAX);
}

static void disk_changed(void) {
    const char *save_dir = NULL;
    char path[1024];
    FILE *f;
    unsigned i;
    if (log_cb) log_cb(RETRO_LOG_INFO, "mock core: disk ejected=%d index=%u num=%u\n", disk_ejected, disk_index, disk_num);
    if (!disk_dump || !env_cb || !env_cb(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, &save_dir) || !save_dir) return;
    snprintf(path, sizeof path, "%s/mock-disk.txt", save_dir);
    f = fopen(path, "wb");
    if (!f) return;
    fprintf(f, "interface=%s\nejected=%d\nindex=%u\nnum=%u\n", disk_ext_registered ? "ext" : "basic", disk_ejected,
            disk_index, disk_num);
    if (initial_set) fprintf(f, "initial=%u\n", initial_index); else fputs("initial=-\n", f);
    for (i = 0; i < disk_num; i++) fprintf(f, "image%u=%s|%s\n", i, disk_path[i], disk_label[i]);
    fclose(f);
}

static bool disk_set_eject_state(bool ejected) { disk_ejected = ejected; disk_changed(); return true; }
static bool disk_get_eject_state(void) { return disk_ejected; }
static unsigned disk_get_image_index(void) { return disk_index; }
static bool disk_set_image_index(unsigned index) {
    if (!disk_ejected || index > disk_num) return false;
    disk_index = index;
    disk_set_calls++;
    disk_changed();
    return true;
}
static unsigned disk_get_num_images(void) { return disk_num; }
static bool disk_replace_image_index(unsigned index, const struct retro_game_info *info) {
    unsigned i;
    if (!disk_ejected || index >= disk_num) return false;
    replace_called = true;
    if (!info) { /* removal: later images shift down */
        for (i = index; i + 1 < disk_num; i++) {
            memcpy(disk_path[i], disk_path[i + 1], DISK_PATH_MAX);
            memcpy(disk_label[i], disk_label[i + 1], DISK_PATH_MAX);
        }
        disk_num--;
        disk_path[disk_num][0] = 0;
        disk_label[disk_num][0] = 0;
        if (disk_index > disk_num) disk_index = disk_num;
    } else {
        set_disk_slot(index, info->path);
    }
    disk_changed();
    return true;
}
static bool disk_add_image_index(void) {
    if (disk_num >= MAX_DISKS) return false;
    add_called = true;
    disk_path[disk_num][0] = 0;
    disk_label[disk_num][0] = 0;
    disk_num++;
    disk_changed();
    return true;
}
static bool disk_set_initial_image(unsigned index, const char *path) {
    initial_set = true;
    initial_index = index;
    snprintf(initial_path, sizeof initial_path, "%s", path ? path : "");
    if (log_cb) log_cb(RETRO_LOG_INFO, "mock core: initial image %u %s\n", index, initial_path);
    return true;
}
static bool disk_get_image_path(unsigned index, char *path, size_t len) {
    if (index >= disk_num || !disk_path[index][0] || !path || !len) return false;
    snprintf(path, len, "%s", disk_path[index]);
    return true;
}
static bool disk_get_image_label(unsigned index, char *label, size_t len) {
    if (index >= disk_num || !disk_label[index][0] || !label || !len) return false;
    snprintf(label, len, "%s", disk_label[index]);
    return true;
}

static void register_disk_interface(void) {
    static struct retro_disk_control_callback basic = {
        disk_set_eject_state, disk_get_eject_state, disk_get_image_index, disk_set_image_index,
        disk_get_num_images, disk_replace_image_index, disk_add_image_index,
    };
    static struct retro_disk_control_ext_callback ext = {
        disk_set_eject_state, disk_get_eject_state, disk_get_image_index, disk_set_image_index,
        disk_get_num_images, disk_replace_image_index, disk_add_image_index,
        disk_set_initial_image, disk_get_image_path, disk_get_image_label,
    };
    ext.set_initial_image = no_initial_on ? NULL : disk_set_initial_image;
    if (disk_mode == 2) {
        unsigned version = 0;
        if (!env_cb(RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION, &version)) version = 0;
        if (version >= 1 && env_cb(RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE, &ext)) {
            disk_ext_registered = true;
            return;
        }
    }
    if (env_cb(RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE, &basic)) disk_basic_registered = true;
}

/* Fills the disk list from the loaded content: a .m3u lists images (relative to its directory), anything else
 * is a single image. `text` is the playlist body (may be NULL when unreadable). */
static void disk_populate(const char *path, const char *text) {
    size_t plen = path ? strlen(path) : 0;
    if (disk_num != 0) return;
    if (plen > 4 && !strcmp(path + plen - 4, ".m3u") && text) {
        char dir[DISK_PATH_MAX];
        const char *p = text, *slash = NULL, *q;
        size_t dlen;
        for (q = path; *q; q++)
            if (*q == '/' || *q == '\\') slash = q;
        dlen = slash ? (size_t)(slash - path) + 1 : 0;
        if (dlen >= sizeof dir) dlen = sizeof dir - 1;
        memcpy(dir, path, dlen);
        dir[dlen] = 0;
        while (*p && disk_num < MAX_DISKS) {
            char line[DISK_PATH_MAX];
            size_t n = 0;
            while (*p && *p != '\n' && *p != '\r') { if (n + 1 < sizeof line) line[n++] = *p; p++; }
            while (*p == '\n' || *p == '\r') p++;
            line[n] = 0;
            if (!line[0] || line[0] == '#') continue;
            if (line[0] == '/' || (line[0] && line[1] == ':')) set_disk_slot(disk_num, line);
            else {
                char full[2 * DISK_PATH_MAX];
                snprintf(full, sizeof full, "%s%s", dir, line);
                set_disk_slot(disk_num, full);
            }
            disk_num++;
        }
    } else if (path) {
        set_disk_slot(0, path);
        disk_num = 1;
    }
    disk_index = 0;
    if (disk_ext_registered && initial_set && initial_index < disk_num) disk_index = initial_index;
    disk_ejected = false;
    disk_changed();
}

static uint8_t fnv_low(const char *s) {
    return s[0] ? (uint8_t)fnv1a(2166136261u, (const uint8_t *)s, strlen(s)) : 0;
}

static void poll_protocol_inputs(void) {
    unsigned i;
    if (kb_poll_on) {
        kb_polled_count = 0;
        kb_polled_lo = kb_polled_hi = 0;
        for (i = 0; i < RETROK_LAST; i++) {
            if (!input_cb(0, RETRO_DEVICE_KEYBOARD, 0, i)) continue;
            if (kb_polled_count == 0) kb_polled_lo = i;
            kb_polled_hi = i;
            kb_polled_count++;
        }
    }
    if (mouse_on) {
        bool left = input_cb(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_LEFT) != 0;
        bool right = input_cb(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_RIGHT) != 0;
        bool middle = input_cb(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_MIDDLE) != 0;
        bool up = input_cb(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_WHEELUP) != 0;
        bool down = input_cb(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_WHEELDOWN) != 0;
        mouse_dx += input_cb(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_X);
        mouse_dy += input_cb(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_Y);
        if (left && !mouse_prev_left) mouse_left_presses++;
        if (right && !mouse_prev_right) mouse_right_presses++;
        if (up) mouse_wheel_steps++;
        if (down) mouse_wheel_steps++;
        mouse_prev_left = left;
        mouse_prev_right = right;
        mouse_buttons = (left ? 1u : 0u) | (right ? 2u : 0u) | (middle ? 4u : 0u) | (up ? 8u : 0u) | (down ? 16u : 0u);
    }
}

static void put_cell(uint32_t *img, unsigned band, unsigned cell, unsigned value) {
    unsigned x, y;
    uint32_t v = value & 0xFFu;
    uint32_t px = (v << 16) | (v << 8) | v;
    for (y = 0; y < CELL; y++)
        for (x = 0; x < CELL; x++) img[(band * CELL + y) * WIDTH + cell * CELL + x] = px;
}
static void put_cell16(uint32_t *img, unsigned band, unsigned cell, unsigned value) {
    put_cell(img, band, cell, value);
    put_cell(img, band, cell + 1, value >> 8);
}

static void draw_registers(uint32_t *img) {
    unsigned i;
    /* band 0 */
    put_cell(img, 0, 0, kb_events);
    put_cell16(img, 0, 1, kb_ring[0]);
    put_cell(img, 0, 3, kb_last_flags);
    put_cell16(img, 0, 4, kb_last_char & 0xFFFFu);
    put_cell16(img, 0, 6, kb_last_mods);
    /* band 1 */
    put_cell(img, 1, 0, kb_polled_count);
    put_cell16(img, 1, 1, kb_polled_lo);
    put_cell16(img, 1, 3, kb_polled_hi);
    put_cell(img, 1, 5, kb_down_events);
    put_cell(img, 1, 6, kb_up_events);
    put_cell(img, 1, 7, kb_char_events);
    /* band 2 */
    put_cell16(img, 2, 0, (unsigned)(mouse_dx & 0xFFFF));
    put_cell16(img, 2, 2, (unsigned)(mouse_dy & 0xFFFF));
    put_cell(img, 2, 4, mouse_buttons);
    put_cell(img, 2, 5, mouse_left_presses);
    put_cell(img, 2, 6, mouse_right_presses);
    put_cell(img, 2, 7, mouse_wheel_steps);
    /* band 3 */
    put_cell(img, 3, 0, disk_index >= 255 ? 255 : disk_index);
    put_cell(img, 3, 1, disk_num);
    put_cell(img, 3, 2, disk_ejected ? 1 : 0);
    put_cell(img, 3, 3, disk_set_calls);
    put_cell(img, 3, 4, disk_index < disk_num ? fnv_low(disk_path[disk_index]) : 0);
    put_cell(img, 3, 5, disk_index < disk_num ? fnv_low(disk_label[disk_index]) : 0);
    put_cell(img, 3, 6, initial_set ? initial_index + 1 : 0);
    put_cell(img, 3, 7, (disk_ext_registered ? 1u : 0u) | (loaded_no_game ? 2u : 0u) | (disk_basic_registered ? 4u : 0u) |
                            (replace_called ? 8u : 0u) | (add_called ? 16u : 0u));
    /* band 4 */
    for (i = 0; i < 4; i++) put_cell16(img, 4, 2 * i, kb_ring[i]);
}

static void probe_environment(void) {
    /* Zeroed scratch large enough for the perf / VFS / LED structs: a frontend that answers true would fill it. */
    static union { unsigned char raw[1024]; void *align; } scratch;
    static bool fast = false;
    memset(scratch.raw, 0, sizeof scratch.raw);
    probe_answer[0] = env_cb(RETRO_ENVIRONMENT_GET_PERF_INTERFACE, scratch.raw);
    probe_answer[1] = env_cb(RETRO_ENVIRONMENT_GET_VFS_INTERFACE, scratch.raw);
    probe_answer[2] = env_cb(RETRO_ENVIRONMENT_GET_LED_INTERFACE, scratch.raw);
    probe_answer[3] = env_cb(RETRO_ENVIRONMENT_SET_FASTFORWARDING_OVERRIDE, &fast);
    if (log_cb) log_cb(RETRO_LOG_INFO, "mock core: env 28=%d 45=%d 46=%d 64=%d\n", probe_answer[0], probe_answer[1],
                       probe_answer[2], probe_answer[3]);
}

static void dump_probe(void) {
    const char *save_dir = NULL;
    char path[DISK_PATH_MAX + 32];
    FILE *f;
    if (!env_probe_on || !disk_dump || !env_cb || !env_cb(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, &save_dir) || !save_dir) return;
    snprintf(path, sizeof path, "%s/mock-env.txt", save_dir);
    f = fopen(path, "w");
    if (!f) return;
    fprintf(f, "env28=%d\nenv45=%d\nenv46=%d\nenv64=%d\n", probe_answer[0], probe_answer[1], probe_answer[2], probe_answer[3]);
    fclose(f);
}

/* ---- libretro entry points ---- */
MOCK_API unsigned retro_api_version(void) { return RETRO_API_VERSION; }

MOCK_API void retro_set_environment(retro_environment_t cb) {
    struct retro_log_callback log;
    bool no_game;
    env_cb = cb;
    read_protocol_env();
    no_game = no_game_on;
    env_cb(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, &no_game);
    log.log = NULL;
    if (env_cb(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, &log)) log_cb = log.log;
    if (kb_cb_on) {
        struct retro_keyboard_callback kb = { mock_keyboard_event };
        env_cb(RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK, &kb);
    }
    if (disk_mode) register_disk_interface();
    if (env_probe_on) probe_environment();
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
    reset_protocol_state();
    dump_probe();
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
    int fmt = PIXEL_FORMAT;
    const char *save_dir = NULL;
    uint32_t h = 2166136261u;
    size_t total = 0;

    if (!game && !no_game_on) return false;
    if (!env_cb(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &fmt)) return false;

    if (!game) {
        loaded_no_game = true; /* no content: seed 0 */
        h = 0;
    } else if (game->data) {
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
#ifdef MOCK_DYNAMIC_OPTIONS
    dynamic_done = false;
#endif

    if (log_cb) log_cb(RETRO_LOG_INFO, "mock core: loaded %s (%u bytes, seed %08x)\n",
                       !game ? "<no game>" : game->path ? game->path : "<memory>", (unsigned)total, (unsigned)seed);

    if (env_cb(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, &save_dir) && save_dir) {
        char path[1024];
        FILE *f;
        /* Fixed format, bounded by snprintf; a truncated path simply fails to open. */
        snprintf(path, sizeof path, "%s/mock-was-here.txt", save_dir);
        f = fopen(path, "wb");
        if (f) { fputs("mock\n", f); fclose(f); }
    }
    if (disk_mode && game) {
        /* Playlist body: from memory, or read from the path (need_fullpath); bounded, NUL-terminated. */
        static char body[4096];
        size_t n = 0;
        body[0] = 0;
        if (game->data) {
            n = game->size < sizeof body - 1 ? game->size : sizeof body - 1;
            memcpy(body, game->data, n);
        } else if (game->path) {
            FILE *f = fopen(game->path, "rb");
            if (f) { n = fread(body, 1, sizeof body - 1, f); fclose(f); }
        }
        body[n] = 0;
        disk_populate(game->path, body);
    }
    return true;
}

MOCK_API bool retro_load_game_special(unsigned type, const struct retro_game_info *info, size_t num) {
    (void)type; (void)info; (void)num;
    return false;
}

MOCK_API void retro_unload_game(void) {
    game_loaded = false;
    /* Like real cores: the disk list belongs to the loaded content. */
    memset(disk_path, 0, sizeof disk_path);
    memset(disk_label, 0, sizeof disk_label);
    disk_num = disk_index = 0;
    disk_ejected = false;
}

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
    poll_protocol_inputs();
    start = (mask >> RETRO_DEVICE_ID_JOYPAD_START) & 1u;

    if (counter == 10) {
        struct retro_message msg = { "mock: frame 10", 60 };
        env_cb(RETRO_ENVIRONMENT_SET_MESSAGE, &msg);
        if (log_cb) log_cb(RETRO_LOG_WARN, "mock core: reached frame %u (color=%s speed=%u)\n",
                           (unsigned)counter, color, step);
    }
#ifdef MOCK_DYNAMIC_OPTIONS
    {
        bool updated = false;
        if (env_cb(RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE, &updated) && updated) {
            struct retro_message m = { "mock: variables updated", 60 };
            env_cb(RETRO_ENVIRONMENT_SET_MESSAGE, &m);
        }
        if (!dynamic_done && counter >= 20) {
            struct retro_variable v = { "mock_color", "blue" };
            dynamic_done = true;
            env_cb(RETRO_ENVIRONMENT_SET_VARIABLE, &v);
            color = get_option("mock_color", "red");
            dominant = !strcmp(color, "green") ? 1u : !strcmp(color, "blue") ? 2u : 0u;
        }
    }
#endif
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
    if (mock_visible) draw_registers(frame_pixels);
#ifdef PIXEL_16
    for (y = 0; y < HEIGHT; y++) {
        for (x = 0; x < WIDTH; x++) {
            uint32_t px = frame_pixels[y * WIDTH + x];
            uint32_t r = (px >> 16) & 0xFFu, g = (px >> 8) & 0xFFu, b = px & 0xFFu;
#ifdef MOCK_PIXEL_RGB565
            frame_pixels16[y * (WIDTH + ROW_PAD_U16) + x] = (uint16_t)(((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3));
#else
            frame_pixels16[y * (WIDTH + ROW_PAD_U16) + x] = (uint16_t)(((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3));
#endif
        }
    }
    video_cb(frame_pixels16, WIDTH, HEIGHT, (WIDTH + ROW_PAD_U16) * sizeof(uint16_t));
#else
    video_cb(frame_pixels, WIDTH, HEIGHT, WIDTH * sizeof(uint32_t));
#endif

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
