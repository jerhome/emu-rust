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
| 0.1.0 (pré-release) | Phase 2 + 4 builds (Docker, Linux, macOS Intel 12.7.6 Monterey, Windows) ; Super Nintendo et Mega Drive. Publiée le 2026-11-20. **Clôturée sans essai** (décision du 2026-11-20 : seules les versions ≥ 1.0 sont essayées) | Pré-release conservée pour l'historique |
| 0.1.x (stable) | **Clôturée** (2026-11-20) : pas d'essais des versions < 1.0, donc pas de 0.1.x | — |
| 0.2 … 0.x | **Clôturée** : la 1.0 a été développée directement | — |
| 1.0 ✅ (1.0.0 stable publiée le 2026-11-20, après essais Windows et macOS) | **Petites consoles** (choix de l'utilisateur, 2026-11-19) : **Game Boy, NES, Game Gear, Master System** (Master System et Game Gear déjà pris en charge par genesis_plus_gx ; cores Game Boy et NES à choisir par l'architecte), **bibliothèque et jaquettes** (phase 4), **gestionnaire de cores complet** dans l'UI (phase 3 : liste, mise à jour, BIOS) | Pré-release `v1.0.0-rc.1` → **premiers essais de l'utilisateur** (guide : `docs/guide-essai-1.0.md`) → corrections → 1.0.0 stable |
| 2.0 (**conception en cours**, 2026-11-20 : ADR 0015, `docs/design/v2.0-atari-st.md`) | **Atari ST** (hatari + EmuTOS téléchargé) + toute l'infrastructure « ordinateur » : clavier complet et clavier virtuel, souris capturée, disquettes multiples et changement à chaud, BIOS libres/fournis avec hash, options par jeu | Même cycle |
| 3.0 | **Amiga** (puae ; AROS + Kickstart fourni par l'utilisateur), réutilise l'infrastructure de la 2.0 | Même cycle |
| 4.0 | **PC-DOS** (dosbox_pure) | Même cycle |

Ordre ST puis Amiga décidé le 2026-11-19 : le ST est plus simple et testable sans ROM propriétaire (EmuTOS), il valide l'infrastructure ordinateur avant l'Amiga.

## Phases

| # | Phase | Critère de fin |
|---|---|---|
| 0 | Cadrage | Squelette, ADR, agents, Dockerfiles. ✅ |
| 1 | POC | genesis_plus_gx fait tourner une ROM homebrew : empreintes d'images headless + capture Xvfb. ✅ |
| 1b | Premier déploiement Docker | Image en mode WSLg et noVNC ; sortie audio réelle via `cpal`. Code et image ✅. **Clôturée** (2026-11-20) : l'essai Docker se fait avec la 1.0 |
| 2 | Frontend solide | Callbacks libretro complets, formats de pixels, sync A/V, SRAM, savestates, configuration des touches. ✅ (essai réel par l'utilisateur avec la 1.0) |
| 0.1 | Release 0.1 | Tag `v0.1.0` : phase 2 + 4 builds (image GHCR, Linux `tar.gz`, macOS Intel `.app`, Windows MSVC `zip`) publiés par `release.yml`, CI Windows ajoutée ; publiée ✅ ; **clôturée sans essai** (voir « Versions ») (ADR 0012, `docs/design/release-0.1.md`) |
| 2b | Builds natifs Linux et macOS Intel | Livrés techniquement par la release 0.1 ; phase close après l'essai utilisateur **de la 1.0**. CI GitHub Actions (fmt, clippy, tests avec le core factice) sur Ubuntu et `macos-15-intel` ; binaire Linux ; bundle `.app` macOS (OpenGL ≤ 4.1, CoreAudio, cores `.dylib`) essayé par l'utilisateur sur son Mac Intel. Binaire Linux essayé directement dans WSL2 (WSLg : Wayland/X11 + PulseAudio) ; profil Docker `x11` non testable (pas d'hôte Linux) |
| 3 | Gestionnaire de cores | Téléchargement et mise à jour des cores depuis l'UI, vérification des BIOS, dossiers système. ✅ (1.0.0-rc.1, ADR 0013) |
| 4 | Bibliothèque et covers | Scan, identification, SQLite, covers (libretro-thumbnails → ScreenScraper → IGDB). ✅ (1.0.0-rc.1, ADR 0014) |
| 5 | Ordinateurs | ST, Amiga, DOS : clavier/souris émulés, disquettes multiples, EmuTOS/AROS automatiques. Découpée en versions 2.0 (ST), 3.0 (Amiga), 4.0 (DOS) |
| 6 | Windows et manette | Manette PS4 (DualShock 4), CI en matrice complète (core factice `.dll`). Le build Windows est avancé dans la release 0.1. **Conception prête** (2026-11-20, version après 1.0, ordre vs 2.0 à décider) : `docs/adr/0016-manette.md` (gilrs, repli SDL3), `docs/design/phase6-manette.md` |
| 7 | 32 bits | PS1, Saturn, 3DO, GBA, 32X ; rendu matériel GL (puis Vulkan) |
| 8 | Sources de ROMs | Téléchargement de ROMs du domaine public / homebrew |
