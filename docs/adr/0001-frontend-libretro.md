# ADR 0001 — Frontend libretro, pas de cores maison

Statut : accepté (2026-11-19)

## Contexte

Écrire un émulateur fidèle prend des années par console. L'écosystème libretro fournit des cores matures pour
tous les systèmes visés (SNES, Mega Drive, ST, Amiga, DOS, puis 32 bits), compilés pour Linux, Windows et macOS.

## Décision

- emu-rust est un **frontend** : il charge les cores dynamiquement (`libloading`) et implémente l'API libretro
  (`libretro.h`) : `retro_init`, `retro_load_game`, `retro_run`, callbacks vidéo / audio / entrées / environnement.
- Les bindings sont écrits à la main dans `emu-libretro` à partir de `libretro.h` (pas de dépendance à un crate tiers
  peu maintenu). Le reste du code ne voit qu'une API Rust sûre (`Core`, `Frame`, `AudioBatch`, `InputState`).
- Une instance de core par processus à la fois (l'API libretro repose sur des callbacks globaux).
- Les cores sont téléchargés depuis le buildbot libretro, jamais commités ni empaquetés.

## Conséquences

- Licences : les cores sont chargés dynamiquement ; usage personnel, donc les cores non commerciaux sont acceptables.
- Si des cores Rust maison voient le jour, ils implémenteront la même API interne que `emu-libretro` expose.
