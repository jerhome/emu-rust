# Phase 2 — Frontend solide : contrat et découpage

Critère de fin (roadmap) : callbacks libretro complets (pour les cores 16 bits / ordinateurs sans HW render),
formats de pixels, sync A/V, SRAM, savestates, configuration des touches — le tout testé headless, menu egui capturé
sous Xvfb, puis essayé par l'utilisateur sous WSLg. Décisions : **ADR 0009** (C : log + core factice),
**ADR 0010** (sync A/V, thread unique), **ADR 0011** (sauvegardes, config). Les signatures `emu-libretro` et
`emu-runtime` sont **dans le code** (corps `todo!()`) ; le code fait foi en cas d'écart. Celles d'`emu-ui` /
`emu-app` (un seul agent) sont données ici.

## 1. Périmètre

| Dans la phase 2 | Plus tard |
|---|---|
| Options de core legacy / v1 / v2 (+ `_INTL` en anglais), `SET_CORE_OPTIONS_DISPLAY`, `SET_VARIABLE`, `GET_VARIABLE_UPDATE` dynamique, callback de mise à jour d'affichage ; écran « Options du core », persistance par core | Localisation des options |
| `GET_LOG_INTERFACE` via cale C (ADR 0009) | — |
| SRAM (`.srm`), savestates 10 emplacements, conteneur `EMURSTAT`, écritures atomiques (ADR 0011) | État automatique à la sortie, rembobinage, miniatures |
| Chargement depuis `.zip` (en mémoire ou extraction temporaire), `GameSource` | 7z, multi-disques (phase 5), `GET_GAME_INFO_EXT` |
| `Pacer` : sync audio + repli horloge, vsync, pause, avance rapide 4× (ADR 0010) ; compteurs xruns dans l'OSD | Ralenti, rubato (si qualité insuffisante) |
| Thread d'émulation : **non** (ADR 0010) | Réévaluable phase 7 |
| egui minimal : menu en jeu, écran des touches (persisté), options du core, OSD (messages + stats) | Bibliothèque (phase 4), gestionnaire de cores (phase 3) |
| `config.toml` (600) à la racine de données ; racine `--data-dir` > `EMU_DATA_DIR` > `directories` ; `assets_dir` | Identifiants (phase 4) |
| Formats de pixels : les 3 déjà gérés, tests de conversion étendus (pitch, géométrie variable) | `SET_ROTATION` (arcade), shaders |
| Manette : rien de codé, mais aucune hypothèse « clavier seul » (profil = liste de liaisons) | PS4 / SDL3 : phase 6 |
| — | `SET_HW_RENDER` : phase 7 ; clavier/souris émulés, disk control : phase 5 |

### Commandes d'environnement (vérifier valeurs et drapeau `EXPERIMENTAL` dans `libretro.h`)

| Commande | Réponse phase 2 |
|---|---|
| `GET_LOG_INTERFACE` | `true`, `emu_libretro_log_printf` (cale C) |
| `GET_CORE_OPTIONS_VERSION` | `2` |
| `SET_CORE_OPTIONS`, `_INTL`, `_V2`, `_V2_INTL` | mémorise (anglais : champ `us`), applique surcharges de `CoreConfig::variables` ; `true` (V2 : valeur de retour selon l'en-tête) |
| `SET_CORE_OPTIONS_DISPLAY`, `SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK` | visibilité / callback appelé par `Core::options` |
| `SET_VARIABLE` | le core change une valeur ; `options_changed` |
| `GET_VARIABLE_UPDATE` | `true` une fois après `set_option` |
| `SET_MESSAGE`, `SET_MESSAGE_EXT`, `GET_MESSAGE_INTERFACE_VERSION` (=1) | `CoreMessage` (cibles OSD) ou `tracing` (cible log) |
| `SET_SERIALIZATION_QUIRKS` | mémorise ; `true` |
| `GET_FASTFORWARDING` | état fourni par `set_fast_forwarding` |
| `GET_CORE_ASSETS_DIRECTORY` | `CoreConfig::assets_dir` |
| `GET_LIBRETRO_PATH` | chemin du core |
| `GET_INPUT_MAX_USERS` | 2 ; `GET_INPUT_DEVICE_CAPABILITIES` : joypad (+ analog plus tard) |
| `GET_OVERSCAN` | `false` (pas de rognage) |
| `SET_MEMORY_MAPS`, `SET_SUBSYSTEM_INFO`, `SET_CONTENT_INFO_OVERRIDE` | `true`, ignorés |
| `GET_USERNAME`, `GET_PERF_INTERFACE`, `GET_VFS_INTERFACE`, `GET_LED_INTERFACE`, `GET_RUMBLE_INTERFACE`, `SET_FRAME_TIME_CALLBACK`, `SET_AUDIO_CALLBACK`, `SET_AUDIO_BUFFER_STATUS_CALLBACK`, `SET_KEYBOARD_CALLBACK`, `SET_DISK_CONTROL_*`, `SET_HW_RENDER`, `GET_PREFERRED_HW_RENDER`, reste | `false`, loggé une fois en `debug` |

Phase 1 inchangé pour le reste (`GET_CAN_DUPE`, `SET_PIXEL_FORMAT`, répertoires, `SET_GEOMETRY`, …).

## 2. API publique

### 2.1 `emu-libretro` (`types.rs`, `core.rs`, `error.rs`)

```rust
pub struct CoreConfig { system_dir, save_dir, assets_dir /* nouveau */, variables }
pub struct RunEvents { av_info_changed, shutdown_requested, options_changed /* nouveau */ }  // non_exhaustive
pub enum GameSource { Path(PathBuf), Memory { path: PathBuf, data: Vec<u8> } }
pub enum MemoryKind { SaveRam, Rtc, SystemRam, VideoRam }
pub struct SerializationQuirks(pub u64);   // consts INCOMPLETE … PLATFORM_DEPENDENT, contains()
pub enum LogLevel { Debug, Info, Warn, Error }
pub struct CoreMessage { text: String, duration: Duration, level: LogLevel }
pub struct CoreOption { key, label, info: Option<String>, category: Option<String>,
                        values: Vec<CoreOptionValue>, default, current, visible: bool }
pub struct CoreOptionValue { value, label }      pub struct CoreOptionCategory { key, label, info }
impl JoypadButton { pub const fn name(self) -> &'static str; pub fn from_name(&str) -> Option<Self>; }
impl Core {
    pub fn load_game_from(&mut self, game: GameSource) -> Result<AvInfo, LibretroError>;
    pub fn serialize_size(&self) -> usize;
    pub fn serialize(&mut self, out: &mut Vec<u8>) -> Result<(), LibretroError>;
    pub fn unserialize(&mut self, data: &[u8]) -> Result<(), LibretroError>;
    pub fn serialization_quirks(&self) -> SerializationQuirks;
    pub fn memory(&self, kind: MemoryKind) -> Option<&[u8]>;
    pub fn memory_mut(&mut self, kind: MemoryKind) -> Option<&mut [u8]>;
    pub fn options(&mut self) -> Vec<CoreOption>;
    pub fn option_categories(&self) -> Vec<CoreOptionCategory>;
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), LibretroError>;
    pub fn set_fast_forwarding(&mut self, enabled: bool);
    pub fn take_messages(&mut self) -> Vec<CoreMessage>;
}
// LibretroError + NeedsFullPath, SerializeUnsupported, SerializeFailed, UnserializeFailed,
//                 UnknownOption(String), InvalidOptionValue { key, value }
```

Invariants : `memory*` réinterroge le core à chaque appel (pointeur jamais gardé) ; tranche vide ou pointeur nul ⇒
`None`. `serialize` respecte `MUST_INITIALIZE` (erreur avant la première image si le drapeau est posé). Toute entrée
dans le core reste sur le thread propriétaire (ADR 0008). La cale de log ne touche pas au gestionnaire.

### 2.2 `emu-runtime` (`pacing.rs`, `saves.rs`, `emulator.rs`, `audio_*.rs`, `gl.rs`)

```rust
// pacing.rs (ADR 0010)
pub const MAX_BURST: u32 = 3;  pub const DEFAULT_FAST_FORWARD_RATIO: f64 = 4.0;
pub enum SyncMode { Audio /* défaut */, Clock }
pub enum Speed { Normal, FastForward { ratio: f64 }, Paused }
pub struct AudioLevel { pub buffered: Duration, pub target: Duration }
pub enum PaceDecision { Run, WaitUntil(Instant), Idle }
impl Pacer { new(fps, SyncMode, now); set_fps(f64); set_mode(SyncMode, now); set_speed(Speed, now);
             speed() -> Speed; poll(now, Option<AudioLevel>) -> PaceDecision }
// saves.rs (ADR 0011)
pub const STATE_SLOTS: u8 = 10;  STATE_MAGIC = *b"EMURSTAT";  STATE_FORMAT_VERSION = 1;
pub enum FileMode { Default, Private /* 0600 */ }
pub fn write_atomic(path: &Path, data: &[u8], mode: FileMode) -> io::Result<()>;
pub fn core_id_from_path(&Path) -> String;  pub fn game_name_from_path(&Path) -> String;
pub fn sram_path(save_dir, core_id, game) -> PathBuf;  pub fn state_path(state_dir, core_id, game, slot) -> PathBuf;
pub struct StateHeader { core_id, core_version, game, created_unix: u64 }
pub enum StateFileError { BadMagic, UnsupportedVersion(u32), Truncated, Checksum, BadHeader, WrongCore { expected, found } }
pub fn encode_state(&StateHeader, payload: &[u8]) -> Vec<u8>;
pub fn decode_state(&[u8]) -> Result<(StateHeader, &[u8]), StateFileError>;
// emulator.rs
pub const MAX_CONTENT_SIZE: u64 = 512 MiB;
pub struct EmulatorOptions { system_dir, save_dir, state_dir /* nouveau */, assets_dir /* nouveau */, core_variables }
pub struct FrameStatus { video_updated, av_info_changed, shutdown_requested, options_changed /* nouveau */ }
impl Emulator {
    core_id(&self) -> &str; game_name(&self) -> &str; sram_path(&self) -> PathBuf;
    state_path(&self, slot) -> Result<PathBuf>; flush_sram(&mut self) -> Result<bool>;
    memory(&self, MemoryKind) -> Option<&[u8]>;
    serialize_state(&mut self) -> Result<Vec<u8>>; unserialize_state(&mut self, &[u8]) -> Result<()>;
    save_state(&mut self, slot) -> Result<PathBuf>; load_state(&mut self, slot) -> Result<()>;
    state_info(&self, slot) -> Option<(StateHeader, SystemTime)>;
    options(&mut self); option_categories(&self); set_option(&mut self, key, value);
    set_fast_forwarding(&mut self, bool); take_messages(&mut self) -> Vec<CoreMessage>;
}
// RuntimeError + Archive { path, message }, NoContentInArchive(PathBuf), ContentTooLarge { path, size },
//                InvalidSlot(u8), StateFile(StateFileError)
// audio
impl RingSink    { pub fn set_paused(&mut self, bool); pub fn level(&self) -> Option<AudioLevel>; }
impl AudioOutput { pub fn set_paused(&mut self, bool); pub fn level(&self) -> Option<AudioLevel>; } // Null ⇒ None
// gl.rs
impl GlWindow { pub fn set_vsync(&self, bool) -> Result<()>; pub fn capture(&mut self) -> Result<FrameRgba>; }
```

Contrat `Emulator::load` (phase 2), détaillé dans le commentaire du code : zip (passé tel quel si le core liste `zip`
ou `block_extract`, sinon première entrée acceptée — **nom de base uniquement**, jamais un chemin d'entrée —, en
mémoire, ou dans un dossier temporaire privé si `need_fullpath`, supprimé au `Drop`) ; `MAX_CONTENT_SIZE` vérifié sur
la taille déclarée **et** sur les octets lus ; création de `saves/<core_id>/`, `states/<core_id>/` ; SRAM restaurée
après `retro_load_game` ; `Drop` ⇒ `flush_sram` (erreur loggée). Dépendances : `zip` (MIT,
`default-features = false, features = ["deflate-flate2-zlib-rs"]`), `tempfile` (MIT/Apache).

Contrat GL : `draw_frame` repositionne tout l'état GL qu'il utilise ; egui dessine entre `draw_frame` et
`swap_buffers` ; `capture` lit le tampon arrière avant `swap_buffers`.

### 2.3 `emu-ui` / `emu-app` (agent `ui` ; signatures cibles, à poser par l'agent)

```rust
// emu-ui/src/config.rs — serde, #[serde(default)] partout, noms en snake_case
pub struct DataDirs { pub root, pub config_file, pub system, pub saves, pub states, pub assets: PathBuf }
impl DataDirs { pub fn under(root: &Path) -> Self; pub fn create_all(&self) -> io::Result<()>; }
pub struct Config { pub video: VideoConfig, pub audio: AudioSettings, pub speed: SpeedConfig,
                    pub hotkeys: Hotkeys, pub input: InputConfig,
                    pub core_options: BTreeMap<String /*core_id*/, BTreeMap<String, String>> }
pub struct VideoConfig { pub vsync: bool /*true*/, pub scaling: ScalingChoice /*integer|fit*/, pub scale: u32 /*3*/ }
pub struct AudioSettings { pub enabled: bool, pub latency_ms: u32 /*64*/, pub sync: SyncChoice /*audio|clock*/ }
pub struct SpeedConfig { pub fast_forward_ratio: f64 /*4.0*/ }
pub struct Hotkeys { pub menu, pause, reset, save_state, load_state, prev_slot, next_slot,
                     fast_forward, fullscreen, stats: KeyCode }
//   défauts : Escape, KeyP, KeyH, F2, F4, F6, F7, Space (maintenue), F11, F3
pub struct InputConfig { pub keyboard: BTreeMap<String /*"default" | core_id*/, KeyMap> }
impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError>;          // absent ⇒ Ok(défaut)
    pub fn save(&self, path: &Path) -> Result<(), ConfigError>;     // write_atomic(.., FileMode::Private)
    pub fn keymap_for(&self, core_id: &str) -> KeyMap;              // profil du core, sinon "default", sinon intégré
    pub fn set_keymap(&mut self, profile: &str, map: KeyMap);
}
pub enum ConfigError { Io { path, source }, Parse(String), Serialize(String) }
// KeyMap : (dé)sérialisé en `bindings = [{ key = "KeyZ", port = 0, button = "y" }]` ; clé ou bouton inconnu ⇒
// liaison ignorée + warn (jamais d'échec du fichier entier). Méthodes ajoutées : bind(key, port, button)
// (une touche = une liaison), unbind(key), keys_for(port, button) -> Vec<KeyCode>.
pub struct LaunchOptions { pub core, pub rom: PathBuf, pub dirs: DataDirs, pub config: Config,
    pub scale: Option<u32>, pub audio: bool, pub exit_after_frames: Option<u64>,
    pub dump_last_frame: Option<PathBuf>, pub screen: Option<Screen>, pub dump_window: Option<PathBuf> }
pub enum Screen { Menu, Keys, CoreOptions }
pub fn run(options: LaunchOptions) -> Result<(), UiError>;   // UiError + Config(ConfigError)
```

CLI : `emu-rust --core C --rom R [--data-dir D] [--system-dir D] [--save-dir D] [--scale N] [--no-audio]
[--frames N] [--dump-frame F.png] [--screen menu|keys|options] [--dump-window F.png]`. `--scale`/`--no-audio` ne
sont pas persistés. `--screen` : après `--frames N` images (défaut 30), ouvre l'écran, rend 3 images egui, écrit
`--dump-window` (`GlWindow::capture`) puis quitte — capture déterministe sous Xvfb, sans temporisation.

Boucle (`about_to_wait`) : `loop { match pacer.poll(now, audio.level()) { Run => step(), WaitUntil(t) => …, Idle => Wait } }`
(≤ `MAX_BURST`). Rendu : `draw_frame` → egui → `swap_buffers`. Échap ouvre/ferme le menu (fini « Échap = quitter » ;
quitter = menu ou fermeture de fenêtre). Menu ouvert ⇒ pause, `release_all`, `flush_sram`. Raccourcis prioritaires
sur le jeu ; l'écran des touches refuse une touche de raccourci. Menu : Reprendre, Réinitialiser, Sauvegarder /
Charger (sélecteur 0–9 avec date), Options du core (par catégorie, `ComboBox`, application immédiate, persistance dans
`core_options.<core_id>` et réinjection via `core_variables` au lancement), Touches (J1/J2 × 16 boutons, profil
« ce core » / « défaut », Retour arrière = effacer), Vidéo/Audio (vsync, échelle, synchro), Quitter. OSD : messages
du core et du frontend (« État 3 sauvegardé », erreurs) ; F3 : fps, audio en file (ms), xruns, échantillons jetés.
Config écrite seulement si modifiée. SRAM : `flush_sram` toutes les 10 s. Dépendances : `egui`, `egui-winit`,
`egui_glow` 0.37 (MIT/Apache), `serde`, `toml` 1.x, `directories` 6 (dans `emu-app`), feature `serde` de `winit`.
Ordre de destruction : painter egui, `GlWindow`, `Emulator`.

## 3. Tests headless

| Sujet | Où | Comment |
|---|---|---|
| Core factice (ADR 0009) | `tests/mock-core/mock_core.c`, `tests/support/mock_core.rs` | 64×48 XRGB8888 déterministe (compteur, graine = FNV du contenu, option `mock_color`, entrées) ; 735 trames audio/image ; SRAM 2 Kio (Start ⇒ compteur écrit en SRAM) ; sérialisation 64 o ; options v2 (1 catégorie, 2 options) avec repli legacy ; `SET_MESSAGE` à l'image 10 ; log `printf` avec arguments ; écrit `mock-was-here.txt` dans `GET_SAVE_DIRECTORY` ; variante `-DMOCK_NEED_FULLPATH` |
| Savestate aller-retour | mock (non ignoré) + genesis (`#[ignore]`) | N images → `serialize_state` → M images (empreintes + hash audio A) → `unserialize_state` → M images ⇒ mêmes empreintes et audio. Idem via `save_state(3)`/`load_state(3)` et dans une **seconde session** (fichier). Fichier tronqué / octet modifié / autre `core_id` ⇒ erreur, jeu intact |
| SRAM | mock | Start ⇒ SRAM modifiée ; drop ⇒ `saves/mock/<jeu>.srm` (octets attendus, pas de `.tmp` restant) ; nouvelle session ⇒ `memory(SaveRam)` restaurée ; `flush_sram` sans changement ⇒ `Ok(false)` |
| Options | mock + genesis | v2 listées avec catégories ; `set_option` ⇒ empreinte change à l'image suivante ; valeur invalide ⇒ erreur ; legacy (core factice compilé `-DMOCK_LEGACY_OPTIONS`) |
| Zip | mock (zip construit dans le test) + genesis (`240pSuite-GenesisMD-1.21.zip` épinglé, conservé par `fetch-test-assets.sh`) | mêmes empreintes que le `.bin` ; entrée `../evil.bin` ⇒ aucun fichier hors du dossier temporaire ; entrée > `MAX_CONTENT_SIZE` déclarée ⇒ `ContentTooLarge` ; aucune entrée acceptée ⇒ `NoContentInArchive` ; variante fullpath ⇒ dossier temporaire supprimé au drop |
| Log | `emu-libretro` | format avec arguments, troncature à 1023 octets, appel hors `run` et depuis un autre thread sans panique |
| Formats / unitaires | crates | `name`/`from_name`, quirks, parsing v1/v2 (structs C construits en test), `Pacer` (instants synthétiques : sous la consigne ⇒ `Run`, au-dessus ⇒ attente calculée, rafale ≤ 3, pause ⇒ `Idle`, avance rapide ≈ fps × ratio sur 1 s simulée, changement de vitesse sans rafale), audio en pause (silence, pas de xrun, pré-remplissage réarmé), `write_atomic` (contenu, 0600, pas de résidu), `encode/decode_state` (chaque préfixe tronqué ⇒ `Err`, jamais de panique), `core_id_from_path` (`.so`/`.dll`/`.dylib`) |
| Config | `emu-ui` | TOML aller-retour (`Config` → texte → `Config` égal) ; fichier partiel ⇒ défauts ; touche inconnue ⇒ liaison ignorée ; fichier invalide ⇒ défauts et fichier intact ; droits 0600 après `save` ; logique des menus/raccourcis en fonctions pures testables sans fenêtre |
| Rendu egui | `scripts/xvfb-capture.sh` (Xvfb, llvmpipe) | `--screen menu|keys|options --dump-window` avec le core factice puis genesis ; PNG non uniformes, taille = fenêtre ; inspection visuelle par `qa-systems` |

Aucun test n'utilise le réseau ; seuls les tests sur genesis_plus_gx sont `#[ignore]`.

## 4. Découpage

| Tâche | Agent (modèle) | Fichiers | Dépend de |
|---|---|---|---|
| **F1** FFI complet | `libretro-ffi` (**opus** : `unsafe`, structures C v1/v2) | `crates/emu-libretro/**` : `sys.rs` (structs, constantes), `callbacks.rs` (commandes §1), `core.rs` (méthodes §2.1), `build.rs` + `src/log_shim.c` (`cc`), tests unitaires | — |
| **R1** Pacer, audio, persistance, GL | `runtime-av` (**opus** : temps réel, atomicité) | `crates/emu-runtime/src/{pacing,saves,audio_ring,audio_out,gl}.rs` + tests unitaires | — |
| **Q1** Core factice | `qa-systems` (sonnet) | `tests/mock-core/mock_core.c`, `tests/support/mock_core.rs`, `scripts/fetch-test-assets.sh` (garder le zip 240p). **Aucun test appelant une API `todo!()`** | — |
| **R2** Emulator phase 2 | `runtime-av` (**opus** : zip non fiable) | `crates/emu-runtime/src/emulator.rs` (+ module privé zip), `Cargo.toml` (`zip`, `tempfile`) | F1, R1, Q1 (pour essayer) |
| **U1** Config, CLI, boucle | `ui` (sonnet) | `crates/emu-ui/src/{config,keyboard,app,lib}.rs`, `crates/emu-app/**` : `DataDirs`, `Config` TOML, `--data-dir`, `Pacer`, vsync, pause/avance rapide, raccourcis, SRAM 10 s | R1 (API figée ; exécution réelle après R1) |
| **P1** Docker et scripts | `platform-build` (sonnet) | `docker/README.md` (raccourcis, `/data/{saves,states,assets,config.toml}`), vérif. compilateur C dans les étages de build, `scripts/xvfb-capture.sh` (`--dump-window`/`--screen` relayés, attente du PNG au lieu du délai) | — |
| **U2** egui | `ui` (sonnet ; opus si U1 a révélé des soucis GL) | `crates/emu-ui/src/{ui/*,app}.rs` : intégration egui, menu, écran des touches, options du core, OSD/stats, `--screen`/`--dump-window` | U1, R1 (`capture`) |
| **Q2** Tests d'intégration | `qa-systems` (sonnet) | `crates/emu-libretro/tests/*`, `crates/emu-runtime/tests/*` (§3, mock + genesis `#[ignore]`) | F1, R2 |
| **Q3** Vérification complète | `qa-systems` (sonnet) | fmt/clippy/test, `--ignored`, `--no-default-features`, captures Xvfb des 3 écrans, image runtime dans dind | U2, Q2, P1 |
| **V1** Revue | `reviewer` (**opus**) | `unsafe` + C (F1), `capture`/vsync (R1), zip + temporaire + `write_atomic` + droits 0600 (R1/R2/U1), licences (`cc`, `zip`, `tempfile`, `egui*`, `toml`, `directories`) | Q3 |
| **E1** Essai réel | **utilisateur** | WSLg : son sans craquement en sync audio, vsync on/off, menu, touches persistées après relance, savestates, avance rapide | V1 |

Vagues (≤ 3 agents, crates disjoints) :

1. **F1** (`emu-libretro`) ‖ **R1** (`emu-runtime`) ‖ **Q1** (`tests/`, `scripts/fetch-test-assets.sh`).
2. **R2** (`emu-runtime`) ‖ **U1** (`emu-ui`, `emu-app`) ‖ **P1** (`docker/`, `scripts/xvfb-capture.sh`).
3. **U2** (`emu-ui`) ‖ **Q2** (`crates/*/tests/`).
4. **Q3** → **V1** → **E1**.

Chaque agent garde le workspace vert (fmt/clippy/test) en fin de tâche. Un besoin hors périmètre (ex. U2 veut
une méthode de `GlWindow`) est décrit dans le résumé final et repasse par l'`architecte`.
