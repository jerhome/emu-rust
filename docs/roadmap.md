# Feuille de route

## Décisions de cadrage (2026-11-19)

1. Frontend libretro en Rust, usage personnel (cores non commerciaux comme snes9x et genesis_plus_gx acceptables).
2. Docker est une cible de déploiement comme les autres, lancée par l'utilisateur sur son hôte : WSLg en priorité,
   X11/Wayland pour un hôte Linux, noVNC pour les tests et le dépannage.
3. Développement et tests dans le conteneur de Claude, sans affichage (headless + captures Xvfb).
4. Clavier d'abord ; manette PS4 avec les builds natifs.
5. Repo GitHub public `jerhome/emu-rust` ; ordre des plateformes (2026-11-19) : Docker (WSLg / X11 / noVNC), puis
   Linux natif et macOS Intel (phase 2b, juste après la phase 2), puis Windows (phase 6).
   Mise à jour : release 0.1 (Docker, Linux, macOS Intel **et Windows**) juste après la phase 2 (ADR 0012).
6. Covers : libretro-thumbnails, ScreenScraper, IGDB. ROMs : domaine public et homebrew uniquement ; BIOS libres EmuTOS et AROS.

## Systèmes et cores

| Système | Core(s) | BIOS |
|---|---|---|
| Super Nintendo | snes9x, bsnes, mesen-s | — |
| Mega Drive / 32X | genesis_plus_gx, picodrive | — (Mega-CD : fourni par l'utilisateur) |
| Atari ST | hatari | EmuTOS (libre, téléchargé) |
| Amiga | puae | AROS (libre, téléchargé) ; Kickstart d'origine fourni par l'utilisateur |
| PC DOS | dosbox_pure | — |
| 32 bits (phase 7) | swanstation / beetle-psx, beetle-saturn, opera, mgba | fourni par l'utilisateur |

## Versions

| Version | Contenu | Processus |
|---|---|---|
| 0.1.0 (pré-release) | Phase 2 + 4 builds (Docker, Linux, macOS Intel 12.7.6 Monterey, Windows) | Tag `v0.1.0` → pré-release GitHub → essais de l'utilisateur sur ses plateformes → corrections |
| 0.1.x (stable) | 0.1 corrigée après essais | Nouvelle version de correctif publiée comme release stable (non pré-release) |
| 1.0 | Les deux ordinateurs 16 bits : **Atari ST** (hatari + EmuTOS) et **Amiga** (puae + AROS), donc phase 5 au moins pour ST/Amiga (DOS non requis), avec ce qu'il faut des phases 3 et 4 pour les rendre utilisables | Même cycle : pré-release, essais, stable |

## Phases

| # | Phase | Critère de fin |
|---|---|---|
| 0 | Cadrage | Squelette, ADR, agents, Dockerfiles. ✅ |
| 1 | POC | genesis_plus_gx fait tourner une ROM homebrew : empreintes d'images headless + capture Xvfb. ✅ |
| 1b | Premier déploiement Docker | Image en mode WSLg et noVNC, lancée par l'utilisateur, jouable au clavier ; sortie audio réelle via `cpal`. Code et image ✅ ; essai WSLg par l'utilisateur en attente |
| 2 | Frontend solide | Callbacks libretro complets, formats de pixels, sync A/V, SRAM, savestates, configuration des touches. ✅ (essai réel par l'utilisateur avec la 0.1) |
| 0.1 | Release 0.1 | Tag `v0.1.0` : phase 2 + 4 builds (image GHCR, Linux `tar.gz`, macOS Intel `.app`, Windows MSVC `zip`) publiés par `release.yml`, CI Windows ajoutée ; checklist essayée par l'utilisateur sur chaque plateforme (ADR 0012, `docs/design/release-0.1.md`) |
| 2b | Builds natifs Linux et macOS Intel | Livrés techniquement par la release 0.1 ; phase close après l'essai utilisateur. CI GitHub Actions (fmt, clippy, tests avec le core factice) sur Ubuntu et `macos-15-intel` ; binaire Linux ; bundle `.app` macOS (OpenGL ≤ 4.1, CoreAudio, cores `.dylib`) essayé par l'utilisateur sur son Mac Intel. Binaire Linux essayé directement dans WSL2 (WSLg : Wayland/X11 + PulseAudio) ; profil Docker `x11` non testable (pas d'hôte Linux) |
| 3 | Gestionnaire de cores | Téléchargement et mise à jour des cores depuis l'UI, vérification des BIOS, dossiers système |
| 4 | Bibliothèque et covers | Scan, identification, SQLite, covers (libretro-thumbnails → ScreenScraper → IGDB) |
| 5 | Ordinateurs | ST, Amiga, DOS : clavier/souris émulés, disquettes multiples, EmuTOS/AROS automatiques |
| 6 | Windows et manette | Manette PS4 (DualShock 4), CI en matrice complète (core factice `.dll`). Le build Windows est avancé dans la release 0.1 |
| 7 | 32 bits | PS1, Saturn, 3DO, GBA, 32X ; rendu matériel GL (puis Vulkan) |
| 8 | Sources de ROMs | Téléchargement de ROMs du domaine public / homebrew |
