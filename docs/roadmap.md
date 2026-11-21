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
| Amiga | puae | AROS (libre, intégré au core puae) ; Kickstart d'origine fourni par l'utilisateur |
| PC DOS | dosbox_pure | — (MT-32 facultatif, fourni par l'utilisateur ; SoundFont libre GeneralUser GS téléchargée sur action) |
| Amstrad CPC (5.0) | cap32 | ROM système intégrées au core (permission d'Amstrad) |
| Commodore 64 (5.0) | vice_x64 (vice_x64sc en alternative) | ROM système intégrées à VICE, **acceptées par l'utilisateur** (2026-11-21) ; JiffyDOS facultatif, fourni par l'utilisateur |
| GBA, 32X, Mega CD (6.0) | mgba, picodrive, genesis_plus_gx | GBA : facultatif ; 32X : aucun ; Mega CD : **requis**, fourni par l'utilisateur |
| PlayStation, 3DO (7.0, rendu logiciel) | PlayStation : pcsx_rearmed (défaut), swanstation, mednafen_psx ; 3DO : opera (ADR 0021) | **recommandés**, fournis par l'utilisateur, jamais téléchargés (`scph5500/5501/5502.bin` par région, `panafz10.bin`) ; sans eux, BIOS de remplacement intégré au core (HLE, OpenBIOS, Opera Boot) avec avertissement |
| Saturn, Dreamcast | plus tard (avec le rendu matériel) | — |

## Versions

| Version | Contenu | Processus |
|---|---|---|
| 0.1.0 (pré-release) | Phase 2 + 4 builds (Docker, Linux, macOS Intel 12.7.6 Monterey, Windows) ; Super Nintendo et Mega Drive. Publiée le 2026-11-20. **Clôturée sans essai** (décision du 2026-11-20 : seules les versions ≥ 1.0 sont essayées) | Pré-release conservée pour l'historique |
| 0.1.x (stable) | **Clôturée** (2026-11-20) : pas d'essais des versions < 1.0, donc pas de 0.1.x | — |
| 0.2 … 0.x | **Clôturée** : la 1.0 a été développée directement | — |
| 1.0 ✅ (1.0.0 stable publiée le 2026-11-20, après essais Windows et macOS) | **Petites consoles** (choix de l'utilisateur, 2026-11-19) : **Game Boy, NES, Game Gear, Master System** (Master System et Game Gear déjà pris en charge par genesis_plus_gx ; cores Game Boy et NES à choisir par l'architecte), **bibliothèque et jaquettes** (phase 4), **gestionnaire de cores complet** dans l'UI (phase 3 : liste, mise à jour, BIOS) | Pré-release `v1.0.0-rc.1` → **premiers essais de l'utilisateur** (guide : `docs/guide-essai-1.0.md`) → corrections → 1.0.0 stable |
| 2.0 (**2.0.0-rc.2**, pré-release le 2026-11-21 ; rc.1 essayée : Mouse Trap OK sous Windows), 2026-11-20 : ADR 0015, `docs/design/v2.0-atari-st.md`) | **Atari ST** (hatari + EmuTOS téléchargé) + toute l'infrastructure « ordinateur » : clavier complet et clavier virtuel, souris capturée, disquettes multiples et changement à chaud, BIOS libres/fournis avec hash, options par jeu | Même cycle |
| 2.1 (**2.1.0-rc.2**, pré-release le 2026-11-21 ; rc.1 le même jour) | **Mega CD express** (priorité utilisateur du 2026-11-21 : Dune ; `docs/design/v2.1-megacd-express.md`, complété en 6.0) + **Manette PS4** (ADR 0016 : gilrs, branchement UI, réglages, CI avec libudev ; décision du 2026-11-21) + disquettes ajoutées en cours de jeu mémorisées + petites corrections issues des essais et audits de la 2.0 | Même cycle |
| 3.0 (**3.0.0-rc.1**, pré-release le 2026-11-21 ;  : conception re-validée contre le code le 2026-11-21, **ADR 0017 acceptée**, contrat posé dans le code (X0, branche `wip-3.0`) ; `docs/design/v3.0-amiga.md` §10, étude `docs/essais-puae.md` ; ordre de travail entre versions : `docs/design/integration-largeur.md`) | **Amiga** (puae ; AROS intégré au core + Kickstart fourni par l'utilisateur), réutilise l'infrastructure de la 2.0 et **pose les règles communes des versions suivantes** (design §3.5 : ordre d'évaluation des médias, extensions partagées, `game_saves`, défauts calculés, contrôle de firmware unique, schéma v4). Touches Amiga = Alt gauche / Alt droit. **Demande utilisateur (2026-11-20)** : bouton sur l'écran principal (bibliothèque) qui ouvre le dossier des ROMs dans le gestionnaire de fichiers du système (Explorateur, Finder, gestionnaire Linux ; sous Docker : afficher le chemin du volume à la place) ; bouton « Importer un BIOS… » (sélecteur natif, hash vérifié, copie sous le nom attendu par le core) | Même cycle |
| 4.0 (**4.0.0-rc.1**, pré-release le 2026-11-21 ; conception corrigée le 2026-11-21 : ADR 0018 proposé, `docs/design/v4.0-dos.md`, étude `docs/essais-dosbox.md`) | **PC-DOS** (dosbox_pure) ; SoundFont GeneralUser GS autorisée (2026-11-21) ; règles CD de la bibliothèque ; axes analogiques de la manette (dépend de la 2.1) | Même cycle |
| 5.0 (**5.0.0-rc.1**, pré-release le 2026-11-21 ; conception corrigée le 2026-11-21 : ADR 0019 (proposé), `docs/design/v5.0-cpc-c64.md`, étude `docs/essais-cpc-c64.md`) | **Amstrad CPC** (cap32) et **Commodore 64** (vice_x64 par défaut, vice_x64sc en alternative : ≈ 5× plus lent) : une seule version (décision du 2026-11-20), une conception commune ; CPC livré en premier en pré-release, puis C64 (repli : C64 en 5.1 s'il bloque). Réutilise l'infrastructure ordinateur 2.0-4.0 (clavier complet, touche hôte, médias multiples, options par jeu, import de BIOS) ; ROMs système intégrées aux cores (règle « Builtin », ADR 0005 amendée ; **ROM C64 acceptées par l'utilisateur le 2026-11-21**) ; mode d'entrée par défaut **Joystick** pour ces deux systèmes (`default_input`) | Même cycle |
| 6.0 (**6.0.0-rc.1**, pré-release le 2026-11-21 ; conception : ADR 0020 (accepté), `docs/design/v6.0-gba-32x-megacd.md`, étude `docs/essais-gba-32x-megacd.md`) | **Game Boy Advance** (mgba), **32X** (picodrive) et **Mega CD** (genesis_plus_gx, BIOS fourni par l'utilisateur), rendu logiciel (décision du 2026-11-20 ; issue de la phase 7). PlayStation et 3DO : 7.0 ; Saturn, Dreamcast et rendu matériel : plus tard | Même cycle |
| 7.0 (**conception prête** le 2026-11-21, feu vert de l'utilisateur le même jour : ADR 0021 (accepté), `docs/design/v7.0-ps1-3do.md` ; branche `wip-7.0`) | **PlayStation** (pcsx_rearmed par défaut ; swanstation et beetle_psx sélectionnables) et **3DO** (opera), rendu logiciel ; réutilise les CD de la 4.0 / 6.0 (`disc_serial`, `DiscRegion`, Redump), le contrôle de firmware par région et l'import de BIOS de la 3.0 ; ajoute les sondes de disque PlayStation / 3DO, le mode de manette DualShock (analogique), la carte mémoire par jeu reprise d'un core à l'autre ; BIOS de l'utilisateur préférés mais non bloquants. **Saturn, Dreamcast et rendu matériel : plus tard** ; le rendu matériel ciblera **OpenGL, DirectX et Vulkan** (Vulkan validé par les tests automatiques lavapipe, essai réel par un camarade) | Même cycle |

### Versions de schéma de la bibliothèque (`library.sqlite`, `SCHEMA_VERSION`)

Une migration par version au plus, toujours sans perte (favoris, historique, jaquettes conservés). La 2.1 n'en a pas.

| Schéma | Version | Contenu |
|---|---|---|
| v1, v2 | 1.0 | tables `dat_info`, `dat_entry`, `game`, `cover` ; `game.pinned` |
| v3 | 2.0 (livré) | `game_media` (jeux à plusieurs disquettes) ; `dat_info(system PRIMARY KEY)` : **un seul DAT par système** |
| **v4** | 3.0 | `dat_info` / `dat_entry` à clé `(system, source)` : plusieurs DAT par système (Amiga : TOSEC + WHDLoad ; CPC en 5.0) |
| **v5** | 4.0 | `game.is_dir` (jeux-dossiers DOS) ; métadonnées DOS éventuelles dans la même migration |
| — | 5.0 | aucune migration (nouveaux systèmes seulement ; utilise la clé `(system, source)` de la v4) |
| **v6** | 6.0 | `game.region` (région du disque : BIOS Mega CD, puis PlayStation en 7.0) |
| — | 7.0 | aucune migration (v6 suffit : `game.region`, `dat_entry.serial` / `region`) |

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
| 5 | Ordinateurs | ST, Amiga, DOS : clavier/souris émulés, disquettes multiples, EmuTOS téléchargé, AROS intégré au core puae. Découpée en versions 2.0 (ST), 3.0 (Amiga), 4.0 (DOS) |
| 6 | Windows et manette | Manette PS4 (DualShock 4), CI en matrice complète (core factice `.dll`). Le build Windows est avancé dans la release 0.1. **Manette livrée en version 2.1** (décision du 2026-11-21 ; T0 et T1 faits, T2 à T7 = 2.1) : `docs/adr/0016-manette.md` (acceptée, amendée le 2026-11-21 : `analog = true`, PS + R3 / L3 ; gilrs, repli SDL3), `docs/design/phase6-manette.md`. En dépendent : axes analogiques de la 4.0 (DG1), mapping GBA de la 6.0 |
| 7 | 32 bits | Réduite (2026-11-20) : **GBA, 32X et Mega CD en version 6.0**, **PS1 et 3DO en version 7.0** (Saturn, Dreamcast, rendu matériel : plus tard). **Rendu matériel, quand il sera abordé (consigne utilisateur du 2026-11-20)** : essayer **OpenGL** et **DirectX** (Windows : contexte libretro D3D11) , et **Vulkan aussi**, mais l'utilisateur ne pourra pas l'essayer (sa machine, Intel Iris Xe, le supporte mal) : les **tests automatiques Vulkan (lavapipe) suffisent à valider les étapes** ; essai réel facultatif par un camarade de l'utilisateur (à lui rappeler) (**conception 6.0 prête** : ADR 0020, `docs/design/v6.0-gba-32x-megacd.md`) ; **conception 7.0 prête** : ADR 0021, `docs/design/v7.0-ps1-3do.md` |
| 8 | Sources de ROMs | Téléchargement de ROMs du domaine public / homebrew |
