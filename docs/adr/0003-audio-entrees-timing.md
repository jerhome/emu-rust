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
