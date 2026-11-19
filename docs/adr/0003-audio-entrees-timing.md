# ADR 0003 — Audio, entrées et timing

Statut : accepté (2026-11-19)

## Audio

- Sortie via `cpal` (ALSA → PulseAudio sous WSLg / Linux, WASAPI, CoreAudio).
- Le core fournit des échantillons à sa fréquence (ex. 32 040 Hz SNES) : rééchantillonnage vers la fréquence
  de la carte avec **contrôle dynamique du débit** (ajustement de ±0,5 % selon le remplissage du tampon) pour éviter
  craquements et dérive.
- Sans périphérique audio (conteneur de dev, noVNC) : sortie nulle ou vers un fichier WAV pour les tests.

## Entrées

- Abstraction `InputBackend` → état de manettes virtuelles libretro (`RETRO_DEVICE_JOYPAD`, clavier, souris).
- **Clavier d'abord** (événements winit), avec une configuration de touches par système, modifiable dans l'UI.
- Manette (DualShock 4 en priorité) en phase 6 : backend SDL3 candidat (gère la DS4 en USB et Bluetooth sans pilote),
  à confirmer par une ADR dédiée. Aucun code ne doit supposer « clavier seulement ».
- Ordinateurs (ST, Amiga, DOS) : clavier complet et souris transmis au core.

## Timing

- Boucle cadencée sur la fréquence du core (`retro_system_av_info.timing.fps`, ex. 59,92 / 50 Hz),
  synchronisée sur l'audio ; vsync en option. Avance rapide et pause gérées dans `emu-runtime`.

## Amendement 2026-11-19 — phase 1b (docs/design/phase1b-docker.md)

- Dépendances : `cpal 0.18.2` (Apache-2.0, backend ALSA par défaut sous Linux, PulseAudio via `libasound2-plugins`),
  `rtrb 0.4.1` (MIT/Apache-2.0, anneau SPSC sans verrou). Sous Linux, `cpal` tire `alsa-sys` : le **build** exige
  `libasound2-dev` + `pkg-config` (image dev, étage de build runtime, conteneur de Claude).
- Rééchantillonnage : interpolation **linéaire maison** (`emu_runtime::StereoResampler`), sans dépendance, testable
  sans périphérique. Un rééchantillonneur sinc (`rubato`) pourra le remplacer en phase 2 si la qualité ne suffit pas.
- Thread audio = celui de `cpal` ; il ne fait que dépiler et convertir (pas d'allocation, de verrou ni de log).
  Le rééchantillonnage se fait côté émulation.
- **Timing en 1b** : la boucle reste cadencée par `FramePacer` (horloge murale). La dérive entre horloge murale et
  horloge de la carte est absorbée par le contrôle dynamique du débit (±0,5 % selon le remplissage de l'anneau,
  consigne 50 %). La synchro « pilotée par l'audio » (attente sur l'anneau, vsync, avance rapide) reste en phase 2.
- Sans périphérique : `AudioOutput::open_or_null` renvoie une sortie nulle avec un `warn!`, jamais de panique.
- `cpal` est isolé derrière la feature `audio-cpal` (par défaut, relayée `emu-app → emu-ui → emu-runtime`) ;
  `--no-default-features` donne un binaire silencieux sans dépendance ALSA. Le pipeline (anneau, rééchantillonnage, DRC)
  ne dépend pas de `cpal`.
