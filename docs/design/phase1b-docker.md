# Phase 1b — Premier déploiement Docker : contrat et découpage

Critère de fin (roadmap) : image runtime lancée **par l'utilisateur** en mode WSLg et noVNC, jeu (240p Test Suite
ou homebrew) **jouable au clavier**, **son réel via `cpal`** en WSLg. Décisions : ADR 0003 (amendement 1b), 0004, 0008.
Les signatures sont **déjà dans le code** (corps `todo!()`) ; le code fait foi en cas d'écart.

## 1. Périmètre

| Dans 1b | Reste en phase 2 (ou plus tard) |
|---|---|
| `CpalSink` / `AudioOutput` : périphérique par défaut, fréquence par défaut, repli silencieux | Choix du périphérique, volume, reconnexion à chaud |
| Rééchantillonnage linéaire + contrôle dynamique du débit (DRC ±0,5 %) | Rééchantillonneur sinc (`rubato`) si la qualité ne suffit pas |
| Cadence `FramePacer` (horloge murale) | Synchro pilotée par l'audio, vsync, avance rapide, pause |
| Correspondance clavier Mega Drive corrigée, liste des touches loggée au démarrage | Configuration des touches dans l'UI (egui), par système |
| `--no-audio` ; noVNC sans son | Son en noVNC (non visé, ADR 0004) |
| Commandes de test utilisateur (WSLg, noVNC, diagnostic son) | X11/Wayland hôte Linux validés (fournis, non exigés en 1b) |

## 2. Audio (`crates/emu-runtime/src/{resample,audio_out}.rs`)

### Feature `audio-cpal` et environnement de build

- Seul `CpalSink` (et la variante `AudioOutput::Device`) dépend de `cpal`, derrière la feature **`audio-cpal`**
  d'`emu-runtime`, relayée par `emu-ui` et `emu-app` (`emu-app/audio-cpal → emu-ui/audio-cpal → emu-runtime/audio-cpal`).
- Tout le reste (`StereoResampler`, `rate_adjust`, `audio_ring` → `RingSink`/`RingReader`, `OutputSample`,
  `AudioConfig`/`AudioStats`/`AudioError`, `AudioOutput`) compile **sans** `cpal` et sans périphérique.
- Historique : le conteneur de Claude n'avait pas `libasound2-dev` (`pkg-config alsa` échouait, `alsa-sys` ne
  compilait pas) ; le plan prévoyait la feature désactivée par défaut. Depuis le redémarrage du conteneur,
  `libasound2-dev` est installé ⇒ **feature activée par défaut** dans les trois crates (`default = ["audio-cpal"]`).
  Les dépendances internes du workspace sont déclarées `default-features = false` pour que
  `cargo build -p emu-app --no-default-features` produise vraiment un binaire sans `cpal` (repli silencieux,
  `AudioError::NotCompiled`) sur une machine sans en-têtes ALSA.
- L'UI ne nomme jamais `CpalSink` : son code est identique avec ou sans la feature.

### Chaîne

```text
core (i16 stéréo, timing.sample_rate) ─push_samples─▶ StereoResampler (ratio × rate_adjust(fill))
   ─f32 stéréo, fréquence carte─▶ rtrb::Producer ═══ anneau SPSC ═══ rtrb::Consumer ─▶ callback cpal
   (thread d'émulation)                                                      (thread audio de cpal)
```

- **Dépendances** (déjà déclarées dans `[workspace.dependencies]` et `crates/emu-runtime/Cargo.toml`) :
  `cpal = "0.18.2"` (Apache-2.0, optionnelle ; arbre transitif MIT/Apache/Zlib/BSD/Unlicense, vérifié),
  `rtrb = "0.4.1"` (MIT OR Apache-2.0). Pas de crate de rééchantillonnage.
- **Build Linux** : `alsa-sys` exige `libasound2-dev` + `pkg-config` (présents : conteneur de Claude, image dev, étage de build runtime).
- **Ouverture** (`CpalSink::open`) : `default_host()` → `default_output_device()` → `default_output_config()`.
  Formats acceptés `F32`, `I16`, `U16`, `I32` (sinon `UnsupportedFormat`) ; conversion par `cpal::FromSample`.
  Canaux : 1 → (L+R)/2 ; ≥ 2 → L, R puis zéros. `BufferSize::Default`.
- **Anneau** : `f32` entrelacés, capacité = 2 × `latency` × fréquence carte × 2 ; consigne de remplissage 50 %
  (= `latency`, 64 ms par défaut). Le producteur n'écrit que des **paires complètes**.
- **Pré-remplissage** : le callback joue du silence tant que l'anneau n'a pas atteint la consigne une première fois
  (drapeau atomique) ; ces silences ne comptent pas comme sous-alimentations.
- **Sous-alimentation** (callback, anneau trop court) : copie ce qui est disponible, complète par du silence,
  `underruns += 1`. Pas de log dans le callback ; le thread d'émulation logge en `debug` toutes les ~5 s si le compteur bouge.
- **Débordement** (producteur, anneau plein) : écrit ce qui tient, **jette le reste** (les plus récents),
  `dropped_samples += n`. Jamais de blocage du thread d'émulation.
- **Erreur de flux** (callback d'erreur cpal, ex. serveur Pulse perdu) : `stream_failed = true` ; le premier
  `push_samples` suivant logge un `warn!` unique puis jette tout (comportement `NullSink`). Pas de reconnexion en 1b.
- **Pas de périphérique** : `AudioOutput::open_or_null` logge `warn!("audio disabled: {err}")` et renvoie `Null`.
  Aucune panique nulle part ; aucun `unwrap` sur un résultat cpal.
- Compteurs partagés : `AtomicU64` / `AtomicBool` en `Relaxed` (statistiques seulement).

### Testabilité sans périphérique

Le pipeline est public et indépendant de `cpal` (`audio_ring.rs`) :
- `audio_ring(input_rate, device_rate, &config) -> (RingSink, RingReader)`.
- `RingSink::push_samples` = remplissage → `set_rate_adjust(rate_adjust(fill, dev))` → `process` → écriture.
- `RingReader::fill::<T: OutputSample>(out, channels)` = ce que fait le callback ; `failure_notifier()` pour le
  callback d'erreur. `CpalSink` ne fait que brancher un flux cpal dessus (le `RingReader` est déplacé dans le callback).

Tests unitaires attendus (`runtime-av`) :
- `StereoResampler` : 32 040 → 48 000 Hz sur 1 s ⇒ 48 000 ± 1 trames ; découpage en morceaux arbitraires = un seul
  appel (égalité exacte) ; signal constant conservé ; sinus 1 kHz : fréquence conservée (passages par zéro ± 1 %) ;
  échantillon impair final ignoré ; `set_input_rate` sans discontinuité ; `ratio()` suit `set_rate_adjust` et le clamp.
- `rate_adjust` : 0 → 1 + d, 0,5 → 1, 1 → 1 − d, hors bornes clampé.
- `OutputSample::from_f32` : bornes et clamp pour `f32`, `i16`, `u16`, `i32`.
- `RingSink` : débordement ⇒ paires complètes seulement + `dropped_samples` ; DRC : anneau vide ⇒ plus de sortie.
- `RingReader::fill` : pré-remplissage silencieux sans compter d'underrun ; underrun ⇒ silence + compteur ; mono et 6 canaux.
- `failure_notifier()` ⇒ `stats().stream_failed` et `push_samples` jette.
- `AudioOutput::null()` accepte des échantillons ; `stats()` par défaut ; `open_or_null` ne panique pas sans carte
  (ici : `default_output_config` renvoie `DeviceNotAvailable` ⇒ `Null`, vérifié).

## 3. Rythme en 1b — décision

La boucle reste pilotée par `FramePacer` (inchangé). Les deux horloges (murale pour l'émulation, carte pour la
lecture) dérivent de quelques 0,01 % ; le **DRC sur le ratio de rééchantillonnage** (±0,5 %, ajusté à chaque
`push_samples` selon le remplissage, consigne 50 %) absorbe la dérive sans toucher à la vidéo. Si l'émulation est
trop lente (rendu logiciel), l'anneau se vide : silences comptés, pas de compensation supplémentaire en 1b.
Après un arrêt long (`FramePacer` resynchronise), rien de spécial : pré-remplissage non rejoué, DRC reconverge.

## 4. Docker et environnement (`platform-build`)

Déjà en cours (non modifié ici) : `libasound2 libasound2-plugins libpulse0 alsa-utils` dans l'image finale,
`libasound2-dev libpulse-dev` à la compilation, `.asoundrc` → `pulse` si `PULSE_SERVER` défini (entrypoint),
`PULSE_SERVER=unix:/mnt/wslg/PulseServer` + montage `/mnt/wslg` (service `emu-wslg`). À ajouter / vérifier :

1. `pkg-config` présent dans l'étage de build runtime et dans l'image dev (requis par `alsa-sys`).
2. Mode **noVNC sans son : accepté** (ADR 0004). L'entrypoint ajoute `--no-audio` en mode `novnc` (évite le bruit ALSA sur stderr).
3. `.asoundrc` écrit dans un `$HOME` inscriptible par l'utilisateur non root (sinon `ALSA_CONFIG_PATH` ou `/etc/asound.conf` à la construction + choix à l'exécution).
4. Données : documenter comment placer core et ROM dans le volume `/data` (ou un montage `EMU_DATA_DIR` en lecture seule pour les ROMs).
5. L'image runtime compile `emu-app` avec ses features par défaut (donc `audio-cpal`) ; rien à passer en plus.
   Si un jour un build doit se passer d'ALSA : `--no-default-features` (binaire silencieux).
6. Commandes de test pour l'utilisateur (dans `docker/README.md` ou en tête de `compose.yaml`) :

```sh
docker compose -f docker/compose.yaml --profile wslg build
# diagnostic son seul (doit produire un bip gauche/droite sous WSLg) :
docker compose -f docker/compose.yaml run --rm --entrypoint /usr/local/bin/sound-check emu-wslg
# jeu :
docker compose -f docker/compose.yaml run --rm emu-wslg --core /data/cores/genesis_plus_gx_libretro.so --rom /data/roms/240pSuite-1.21.bin
# noVNC (sans son) puis http://localhost:6080/vnc.html :
docker compose -f docker/compose.yaml --profile novnc up --build emu-novnc
```

   `sound-check` = petit script de l'image : configure `.asoundrc` comme l'entrypoint puis `speaker-test -c2 -t sine -l1`.

## 5. API publique ajoutée

### `emu-runtime`

```rust
pub const MAX_RATE_ADJUST: f64 = 0.02;
pub struct StereoResampler;                 // Debug, Clone
impl StereoResampler {
    pub fn new(input_rate: f64, output_rate: f64) -> Self;   // taux invalides → 44 100 Hz + warn
    pub fn set_input_rate(&mut self, input_rate: f64);
    pub fn set_rate_adjust(&mut self, adjust: f64);          // clamp 1 ± MAX_RATE_ADJUST
    pub fn ratio(&self) -> f64;                              // sortie / entrée, ajustement compris
    pub fn process(&mut self, input: &[i16], out: &mut Vec<f32>); // ajoute à `out`
    pub fn reset(&mut self);
}
pub fn rate_adjust(fill: f64, max_deviation: f64) -> f64;

// audio_ring.rs — sans cpal
pub struct AudioConfig { pub latency: Duration /* 64 ms */, pub max_rate_deviation: f64 /* 0.005 */ } // Default
#[non_exhaustive] pub struct AudioStats { pub underruns: u64, pub dropped_samples: u64, pub stream_failed: bool }
#[non_exhaustive] pub enum AudioError { NoDevice, Config(String), UnsupportedFormat(String), Stream(String), NotCompiled }
pub trait OutputSample: Copy + Send + 'static { const SILENCE: Self; fn from_f32(v: f32) -> Self; } // f32 i16 u16 i32
pub fn audio_ring(input_rate: f64, device_rate: u32, config: &AudioConfig) -> (RingSink, RingReader);
pub struct RingSink;     // device_rate(), set_input_rate(f64), fill() -> f64, stats(); impl AudioSink
pub struct RingReader;   // Send ; fill<T: OutputSample>(&mut self, &mut [T], channels: usize);
                         // failure_notifier(&self) -> impl Fn() + Send + Sync + 'static

// audio_out.rs
#[cfg(feature = "audio-cpal")]
pub struct CpalSink;                        // !Send (cpal::Stream)
impl CpalSink {
    pub fn open(input_rate: f64, config: &AudioConfig) -> Result<Self, AudioError>;
    pub fn device_rate(&self) -> u32;
    pub fn set_input_rate(&mut self, input_rate: f64);
    pub fn stats(&self) -> AudioStats;
}
impl AudioSink for CpalSink;
#[non_exhaustive] pub enum AudioOutput { #[cfg(feature = "audio-cpal")] Device(CpalSink), Null(NullSink) }
impl AudioOutput {
    pub fn open_or_null(input_rate: f64, config: &AudioConfig) -> Self; // sans feature : warn NotCompiled + Null
    pub fn null() -> Self;
    pub fn is_device(&self) -> bool;
    pub fn set_input_rate(&mut self, input_rate: f64);
    pub fn stats(&self) -> AudioStats;
}
impl AudioSink for AudioOutput;
```

### `emu-ui` / `emu-app`

- `LaunchOptions` gagne `pub audio: bool` (`cli.rs` met `true` ; `ui` ajoute l'option `--no-audio` → `false`).
- `app.rs` : champ `audio: AudioOutput` (remplace `NullSink`), ouvert dans `run` après `Emulator::load` :
  `if options.audio { AudioOutput::open_or_null(av.timing.sample_rate, &AudioConfig::default()) } else { AudioOutput::null() }` ;
  sur `av_info_changed` : `audio.set_input_rate(...)` ; à la sortie, `info!` des `stats()`. Au démarrage, `info!` de la
  liste des touches (`KeyMap::bindings`) pour le joueur.

## 6. Découpage

| Tâche | Agent | Fichiers | Dépend de |
|---|---|---|---|
| A1a Pipeline sans cpal | `runtime-av` | `crates/emu-runtime/src/{resample,audio_ring}.rs` + tests unitaires §2 | — |
| A1b Branchement cpal | `runtime-av` | `crates/emu-runtime/src/audio_out.rs` (`CpalSink`, `AudioOutput`) ; vérifier aussi `cargo clippy -p emu-app --no-default-features` | A1a (même agent, à la suite) |
| P1 Docker | `platform-build` | `pkg-config` dans les Dockerfiles, entrypoint `--no-audio` en noVNC, script `sound-check`, `.asoundrc` inscriptible, commandes §4.6, doc de remplissage de `/data` (travail déjà en cours sur `docker/`) | — |
| U1 UI | `ui` | `crates/emu-ui/src/{app,lib}.rs`, `crates/emu-app/src/cli.rs` (`--no-audio`, usage, log des touches) ; correction `keyboard.rs` (déjà en cours) | API figée ; exécution réelle après A1b |
| Q1 Vérif. | `qa-systems` | `crates/emu-runtime/tests/audio_pipeline.rs` (API publique : rééchantillonneur, `rate_adjust`, `audio_ring` de bout en bout, `open_or_null` sans carte ⇒ `Null`) ; vérif. complète avec et sans `--no-default-features` ; build de l'image runtime dans dind + noVNC headless (capture Xvfb) | A1a, A1b, U1, P1 |
| R1 Revue | `reviewer` | callback temps réel (pas d'alloc/verrou/log), aucun `unwrap` sur cpal, licences `cpal`/`rtrb`/arbre, scripts Docker | Q1 |
| E1 Essai réel | **utilisateur** | commandes §4.6 sous WSLg (son + clavier) et noVNC | Q1, R1 |

Parallélisme : **vague 1** = A1a→A1b (`runtime-av`), P1, U1 en parallèle (signatures figées, un crate par agent ;
tout compile déjà, corps `todo!()`). **Vague 2** = Q1 puis R1. **Vague 3** = E1 (utilisateur).
Tout changement de signature publique repasse par l'`architecte`.
