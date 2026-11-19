# Phase 1 — POC : contrat et découpage

Critère de fin (roadmap) : genesis_plus_gx fait tourner une ROM homebrew Mega Drive, validé par des empreintes
d'images headless (tests) et une capture Xvfb. Décisions structurantes : ADR 0001, 0002, 0003, 0007, **0008**.

Les signatures ci-dessous sont **déjà dans le code** (corps `todo!()`) ; le code fait foi en cas d'écart.

## 1. Périmètre

### Dans la phase 1

- `emu-libretro` : chargement `libloading`, vérification `retro_api_version == 1`, `retro_init/deinit`,
  `retro_get_system_info`, `retro_load_game` (en mémoire si `!need_fullpath`, sinon chemin), `retro_get_system_av_info`,
  `retro_run`, `retro_reset`, `retro_unload_game`, `retro_set_controller_port_device` (joypad sur les ports 0 et 1).
- Callbacks vidéo (logiciel, 3 formats de pixels, dupe), audio (sample + batch), `input_poll`, `input_state`.
- Commandes d'environnement gérées (noms de `libretro.h`, vérifier les valeurs et le drapeau `EXPERIMENTAL`
  dans l'en-tête officiel) :

  | Commande | Réponse phase 1 |
  |---|---|
  | `GET_CAN_DUPE` | `true` |
  | `SET_PIXEL_FORMAT` | accepte `0RGB1555`, `XRGB8888`, `RGB565` |
  | `GET_SYSTEM_DIRECTORY`, `GET_SAVE_DIRECTORY`, `GET_CORE_ASSETS_DIRECTORY` | chemins de `CoreConfig` (C-string gardée vivante) |
  | `GET_CORE_OPTIONS_VERSION` | `false` ⇒ le core utilise `SET_VARIABLES` (format historique) |
  | `SET_VARIABLES` | mémorise clés et valeur par défaut (première de `"Desc; a\|b\|c"`), applique les surcharges |
  | `GET_VARIABLE` | valeur courante, `false` si clé inconnue |
  | `GET_VARIABLE_UPDATE` | `false` |
  | `SET_GEOMETRY`, `SET_SYSTEM_AV_INFO` | met à jour `av_info`, `RunEvents::av_info_changed` |
  | `SHUTDOWN` | `RunEvents::shutdown_requested` |
  | `GET_INPUT_BITMASKS` | `true` (id `JOYPAD_ID_MASK` géré par le runtime) |
  | `GET_LANGUAGE` | anglais |
  | `GET_AUDIO_VIDEO_ENABLE` | vidéo + audio activés |
  | `SET_INPUT_DESCRIPTORS`, `SET_CONTROLLER_INFO`, `SET_PERFORMANCE_LEVEL`, `SET_SUPPORT_ACHIEVEMENTS`, `SET_SUPPORT_NO_GAME` | `true`, ignorés |
  | `GET_LOG_INTERFACE` | `false` (callback variadique, ADR 0008) |
  | `SET_HW_RENDER` et tout le reste | `false`, loggé une fois en `debug` |

- `emu-runtime` : `Emulator` headless, conversion → `FrameRgba`, empreinte, PNG, `AudioSink` (nul / collecte / WAV),
  `InputBackend` (nul, scripté), `FramePacer` (horloge murale), `GlWindow` + `FrameRenderer` (texture, échelle entière).
- `emu-ui` : boucle `winit`, fenêtre, clavier → `KeyboardInput`, cadence, sortie après N images, dump PNG.
- `emu-app` : CLI minimale → `emu_ui::run`.

### Reporté en phase 1b / 2

- 1b : sortie audio `cpal` (la phase 1 utilise `NullSink` en fenêtre) ; image Docker jouable.
- 2 : synchro A/V pilotée par l'audio et contrôle dynamique du débit ; options de core v1/v2 et UI ;
  `GET_LOG_INTERFACE` (cale C) ; SRAM, savestates ; extraction zip ; configuration des touches ; egui ;
  core sur thread dédié ; `SET_MEMORY_MAPS`, rotation, messages OSD, `GET_VARIABLE_UPDATE` dynamique.
- 5 : clavier/souris émulés ; 6 : manette ; 7 : `SET_HW_RENDER`.

## 2. API publique

### `emu-libretro` (`crates/emu-libretro/src/{core,types,error}.rs`)

```rust
pub struct CoreConfig { pub system_dir: PathBuf, pub save_dir: PathBuf, pub variables: Vec<(String, String)> }

pub trait FrontendHandler {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>); // None = dupe
    fn audio_samples(&mut self, samples: &[i16]);               // stéréo entrelacé
    fn input_poll(&mut self);
    fn input_state(&mut self, query: InputQuery) -> i16;
}

#[non_exhaustive] #[derive(Default)]
pub struct RunEvents { pub av_info_changed: bool, pub shutdown_requested: bool }

pub struct Core { /* !Send + !Sync */ }
impl Core {
    pub fn load(path: &Path, config: CoreConfig) -> Result<Self, LibretroError>;
    pub fn system_info(&self) -> &SystemInfo;
    pub fn load_game(&mut self, rom: &Path) -> Result<AvInfo, LibretroError>;
    pub fn av_info(&self) -> Option<&AvInfo>;
    pub fn pixel_format(&self) -> PixelFormat;
    pub fn run(&mut self, handler: &mut dyn FrontendHandler) -> Result<RunEvents, LibretroError>;
    pub fn reset(&mut self);
    pub fn unload_game(&mut self);
}
impl Drop for Core; // unload_game, retro_deinit, dlclose, libère la garde

pub enum PixelFormat { Xrgb1555 /* défaut */, Xrgb8888, Rgb565 }   // + bytes_per_pixel()
pub struct VideoFrame<'a> { pub data: &'a [u8], pub width: u32, pub height: u32, pub pitch: usize, pub format: PixelFormat }
pub struct SystemInfo { pub library_name: String, pub library_version: String,
                        pub valid_extensions: Vec<String>, pub need_fullpath: bool, pub block_extract: bool }
pub struct GameGeometry { pub base_width: u32, pub base_height: u32, pub max_width: u32, pub max_height: u32,
                          pub aspect_ratio: f32 }  // + display_aspect()
pub struct SystemTiming { pub fps: f64, pub sample_rate: f64 }
pub struct AvInfo { pub geometry: GameGeometry, pub timing: SystemTiming }
pub enum InputDevice { None, Joypad, Mouse, Keyboard, LightGun, Analog, Pointer }  // + from_raw(u32)
pub struct InputQuery { pub port: u32, pub device: InputDevice, pub index: u32, pub id: u32 }
pub const JOYPAD_ID_MASK: u32 = 256;
#[repr(u32)] pub enum JoypadButton { B = 0, Y, Select, Start, Up, Down, Left, Right, A, X, L, R, L2, R2, L3, R3 }
#[non_exhaustive] pub enum LibretroError { AlreadyLoaded, Load{..}, MissingSymbol(&'static str), ApiVersion{..},
    Io{..}, LoadGameFailed(PathBuf), NoGame, GameAlreadyLoaded, CallbackPanicked }
```

Singleton de callbacks (ADR 0008) : garde atomique process-wide ; état d'environnement global de `load` à `Drop` ;
gestionnaire installé seulement pendant `run` (garde RAII) ; `catch_unwind` dans chaque `extern "C"` ;
callbacks d'un autre thread ou hors `run` ignorés. Le module `sys` (bindings bruts) reste privé.

### `emu-runtime` (`crates/emu-runtime/src/*.rs`)

```rust
pub use emu_libretro::{AvInfo, GameGeometry, JoypadButton, SystemInfo, SystemTiming};

pub struct EmulatorOptions { pub system_dir: PathBuf, pub save_dir: PathBuf, pub core_variables: Vec<(String, String)> }
#[non_exhaustive] #[derive(Default)]
pub struct FrameStatus { pub video_updated: bool, pub av_info_changed: bool, pub shutdown_requested: bool }
pub struct Emulator { /* !Send */ }
impl Emulator {
    pub fn load(core: &Path, rom: &Path, options: EmulatorOptions) -> Result<Self, RuntimeError>;
    pub fn system_info(&self) -> &SystemInfo;
    pub fn av_info(&self) -> &AvInfo;
    pub fn run_frame(&mut self, input: &mut dyn InputBackend, audio: &mut dyn AudioSink)
        -> Result<FrameStatus, RuntimeError>;
    pub fn frame(&self) -> &FrameRgba;
    pub fn frame_count(&self) -> u64;
    pub fn reset(&mut self);
}

pub struct FrameRgba { /* RGBA8 serré, alpha 255 */ }
impl FrameRgba { pub fn width(&self) -> u32; pub fn height(&self) -> u32; pub fn pixels(&self) -> &[u8];
                 pub fn fingerprint(&self) -> u64;  // FNV-1a 64 : width LE, height LE, pixels
                 pub fn write_png(&self, path: &Path) -> Result<(), RuntimeError>; }
pub fn convert_frame(src: &VideoFrame<'_>, dst: &mut FrameRgba); // 5 bits: (v<<3)|(v>>2) ; 6 bits: (v<<2)|(v>>4)

pub trait AudioSink { fn push_samples(&mut self, interleaved_stereo: &[i16]); }
pub struct NullSink;  pub struct CollectSink { pub samples: Vec<i16> }
pub fn write_wav(path: &Path, sample_rate: u32, interleaved_stereo: &[i16]) -> io::Result<()>;

pub const MAX_PORTS: usize = 2;
pub struct PadState { .. }  // set(button, pressed), is_pressed(button), bits() -> u16
#[non_exhaustive] #[derive(Default)] pub struct InputFrame { pub pads: [PadState; MAX_PORTS] }
pub trait InputBackend { fn poll(&mut self, frame_index: u64, state: &mut InputFrame); }
pub struct NullInput;
pub struct ScriptedInput;  // new(), press(port, button, frames: Range<u64>) -> Self

pub struct FramePacer;     // new(fps, now), set_fps, frame_due(now) -> bool, next_deadline() -> Instant

pub struct GlWindow;       // new(&ActiveEventLoop, WindowAttributes) -> Result; window(); gl() -> &Arc<glow::Context>;
                           // resize(PhysicalSize<u32>); swap_buffers() -> Result
pub enum Scaling { Integer /* défaut */, Fit }
pub struct FrameRenderer;  // new(&GlWindow) -> Result; upload(&FrameRgba); draw(viewport, aspect_ratio, Scaling)
#[non_exhaustive] pub enum RuntimeError { Libretro(LibretroError), Io{..}, Png(String), Gl(String) }
```

Contrat d'entrée : avant chaque image, le runtime **remet `InputFrame` à zéro** puis appelle chaque backend actif ;
un backend ne fait que *positionner* ce qu'il tient enfoncé (combinaison par OU : clavier + manette en phase 6).
`input_state` : `Joypad` + id < 16 → bouton ; `Joypad` + `JOYPAD_ID_MASK` → `bits()` ; autres → 0.

### `emu-ui` (`crates/emu-ui/src/{lib,keyboard}.rs`)

```rust
pub struct LaunchOptions { pub core: PathBuf, pub rom: PathBuf, pub system_dir: PathBuf, pub save_dir: PathBuf,
                           pub scale: u32, pub exit_after_frames: Option<u64>, pub dump_last_frame: Option<PathBuf> }
pub fn run(options: LaunchOptions) -> Result<(), UiError>;   // thread principal
pub struct KeyMap;        // new(Vec<(KeyCode, usize, JoypadButton)>), genesis_default(), bindings()
pub struct KeyboardInput; // new(KeyMap), handle_key(KeyCode, bool) -> bool, release_all(); impl InputBackend
#[non_exhaustive] pub enum UiError { Runtime(RuntimeError), EventLoop(String) }
```

Boucle : `ApplicationHandler` winit 0.30 ; `resumed` crée `GlWindow` + `FrameRenderer` ; `about_to_wait` :
si `FramePacer::frame_due` → `run_frame(&mut keyboard, &mut NullSink)`, `upload`, `request_redraw` ;
`ControlFlow::WaitUntil(next_deadline)`. Échap / fermeture / `shutdown_requested` / `exit_after_frames` → sortie
(dump PNG si demandé). Ordre de destruction : `FrameRenderer`, `GlWindow`, `Emulator`.

### `emu-app`

`emu-rust --core <core.so> --rom <fichier> [--scale N=3] [--frames N] [--dump-frame out.png]
[--system-dir DIR] [--save-dir DIR]` (défauts : `./data/system`, `./data/saves`, créés si absents).
Parsing à la main ou `clap` (choix de l'agent `ui`) ; code de sortie ≠ 0 et message d'erreur lisible en cas d'échec.

## 3. Ressources de test

### ROM : 240p Test Suite (Mega Drive), Artemio Urbina — **GPL-2.0**

- Source : <https://github.com/ArtemioUrbina/240pTestSuite> (fichier `LICENSE` = GPL v2 ; homebrew de test vidéo,
  écrans statiques déterministes, idéal pour des empreintes).
- Binaire : `https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download`
  - SHA256 du zip : `2054524a7f8695d8df7f4cf1a773af6b26d4c7fe7f74d8f115fab2641a5a9dc5`
  - contenu : `240pSuite-1.21.bin` (262 144 octets), SHA256 `1776009ab324c18e4f046b4781fed11b897281f407ec04f2ac43649bcebae28f`
- Destination : `tests/roms/240pSuite-1.21.bin` (déjà ignoré par git).

### Core : genesis_plus_gx

- `https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip` (HTTPS, buildbot officiel).
- Le nightly change : pas de SHA256 épinglable. Politique : **TOFU** — au premier téléchargement le script écrit
  `tests/cores/genesis_plus_gx_libretro.so.sha256` ; les exécutions suivantes ne retéléchargent pas et vérifient ce
  hash ; `--refresh-cores` force la mise à jour. `tests/cores/` est ignoré par git.

### Script `scripts/fetch-test-assets.sh`

Idempotent, `curl --proto '=https' --fail`, zips décompressés dans un dossier temporaire neuf, vérification SHA256
(ROM épinglée, core TOFU), aucun ROM/BIOS commercial. Dépendances : `curl`, `unzip`, `sha256sum`.

### Tests headless (`crates/emu-runtime/tests/headless_genesis.rs`)

- Tous `#[ignore]` (ADR 0007). Ressources absentes ⇒ **échec** avec le message « lancer scripts/fetch-test-assets.sh »
  (pas de skip silencieux en mode `--ignored`).
- Sérialisés par un `static LOCK: Mutex<()>` (un seul `Core` par processus, ADR 0008).
- Assertions :
  1. `system_info().library_name` contient « Genesis Plus GX » ; `av_info` : 320×224 (ou 256×224), fps ≈ 59,9.
  2. Après 600 images sans entrée : image non uniforme ; empreintes aux images 120, 300, 600 = références.
  3. Déterminisme : deux sessions successives donnent les mêmes empreintes.
  4. Entrée : `ScriptedInput` (Down sur 400..410, A/Start ensuite) ⇒ empreinte à 600 ≠ sans entrée.
  5. Audio : nombre d'échantillons sur 600 images ≈ `sample_rate / fps × 600 × 2` (± 5 %).
- Références : `tests/fingerprints/genesis_plus_gx-240p.txt` (`frame=<n> fingerprint=<hex16>`), générées avec
  `EMU_UPDATE_FINGERPRINTS=1` après inspection des PNG écrits dans `target/test-output/`. Si un nouveau nightly
  du core les fait varier, on régénère après inspection visuelle (le hash du core est noté en tête du fichier).

### Capture Xvfb (`scripts/xvfb-capture.sh`)

`Xvfb :99 -screen 0 1280x800x24` ; `LIBGL_ALWAYS_SOFTWARE=1 DISPLAY=:99 emu-rust --core … --rom … --frames 600
--dump-frame target/test-output/last.png &` ; capture `import -window root target/test-output/xvfb.png` vers
l'image ~500 (le binaire s'arrête seul à 600) ; vérifie que les deux PNG existent et ne sont pas noirs.

## 4. Découpage

| Tâche | Agent | Fichiers | Dépend de |
|---|---|---|---|
| T1 FFI | `libretro-ffi` | `crates/emu-libretro/**` (module `sys` privé, callbacks, `Core`) ; tests unitaires (`from_raw`, `from_id`, `display_aspect`, parsing `"Desc; a\|b"`) + test `#[ignore]` chargeant le vrai core | — |
| T2 Runtime headless | `runtime-av` | `crates/emu-runtime/src/{frame,audio,input,timing,emulator,error}.rs` + tests unitaires (conversions 3 formats, empreinte, WAV, `ScriptedInput`, `FramePacer`) | API T1 (déjà figée) ; intégration réelle après T1 |
| T3 Runtime GL | `runtime-av` | `crates/emu-runtime/src/gl.rs` | — (même agent que T2, à la suite) |
| T4 UI + app | `ui` | `crates/emu-ui/**`, `crates/emu-app/**` | API T2/T3 (figée) ; exécution réelle après T1–T3 |
| T5 Infra | `platform-build` | `.gitignore` (`tests/cores/`, `target/test-output` déjà sous `/target`), `scripts/xvfb-capture.sh`, vérif. paquets Docker dev/runtime (`libxkbcommon-x11-0`, `libegl1`, `libgl1-mesa-dri`) | — |
| T6 Tests | `qa-systems` | `scripts/fetch-test-assets.sh`, `crates/emu-runtime/tests/headless_genesis.rs`, `tests/fingerprints/` | squelette : aucune ; exécution : T1+T2 |
| T7 Vérif. complète | `qa-systems` | génération des références, capture Xvfb, verdict | T1–T6 |
| T8 Revue | `reviewer` | `unsafe` de T1/T3, script de téléchargement (T6) | T7 |

Parallélisme : **vague 1** = T1, T2+T3, T4, T5, T6 en parallèle (les signatures compilent déjà, chacun code contre
le contrat ; un crate par agent). **Vague 2** = T7 puis T8. Tout changement de signature publique passe par
l'`architecte` (le décrire dans le résumé final plutôt que modifier un autre crate).

## 5. Points ouverts — tranchés (2026-11-19)

1. Core de test : nightly du buildbot, TOFU (`tests/cores/*.sha256`, non versionné, propre à chaque machine),
   `--refresh-cores` pour mettre à jour. Un nouveau nightly peut obliger à régénérer les empreintes après vérification visuelle.
2. Code des tests d'intégration dans `crates/<crate>/tests/` (racine = workspace virtuel) ; données à la racine :
   `tests/roms`, `tests/cores`, `tests/fingerprints`.
3. Audio réel (`cpal`) en phase 1b ; en phase 1, vérifié via WAV / comptage d'échantillons, la fenêtre jette l'audio.
