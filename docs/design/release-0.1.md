# Release 0.1.0 — périmètre, workflow, découpage

Décision : **ADR 0012**. Cible : tag `v0.1.0` poussé quand tout est vert ; l'utilisateur teste les 4 builds.

## 1. Périmètre

| Dans la 0.1 | Hors 0.1 |
|---|---|
| Phase 2 complète (U2 egui inclus) : callbacks, formats, sync A/V, SRAM, savestates, menu Échap, touches, options du core | Gestionnaire de cores (phase 3) : l'utilisateur télécharge le core lui-même |
| Builds Linux x86_64, macOS Intel (`.app`), **Windows x86_64** (avancé de la phase 6), image Docker sur GHCR | Manette PS4 (phase 6), bibliothèque / covers (phase 4) |
| Lancement **en ligne de commande** : `emu-rust --core <core> --rom <rom>` | Lanceur graphique, sélecteur de fichiers |
| `emu-rust --version` | Signature / notarisation macOS, signature Authenticode |

Aucun core, BIOS ni ROM dans les artefacts (ADR 0001, 0005) : `README.txt` donne les URLs.

### Core et ROM de test

- ROM : **240p Test Suite** (Mega Drive) 1.21, GPL-2.0 —
  `https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download`
  (zip SHA256 `2054524a7f8695d8df7f4cf1a773af6b26d4c7fe7f74d8f115fab2641a5a9dc5` ; le `.zip` se charge directement).
- Core `genesis_plus_gx` (buildbot officiel, URLs vérifiées le 2026-11-19 ; dézipper) :

| OS | URL |
|---|---|
| Linux | `https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip` |
| macOS Intel | `https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/genesis_plus_gx_libretro.dylib.zip` |
| Windows | `https://buildbot.libretro.com/nightly/windows/x86_64/latest/genesis_plus_gx_libretro.dll.zip` |

  SNES : même chemin avec `snes9x_libretro.{so,dylib,dll}.zip`.
- Dossier de données par défaut (`directories`) : Linux `~/.local/share/emu-rust`, macOS
  `~/Library/Application Support/emu-rust`, Windows `%APPDATA%\emu-rust\data`, Docker `/data`. `--data-dir` le remplace.

### Checklist de test (chaque plateforme)

1. `emu-rust --version` affiche `0.1.0`.
2. **Lancement** avec core + `240pSuite-GenesisMD-1.21.zip` : la fenêtre s'ouvre, pas d'erreur dans le terminal.
3. **Image** : menu de la 240p nette, mise à l'échelle entière ; « Color Bars » sans couleurs inversées ;
   redimensionner la fenêtre, F11 plein écran (HiDPI : image pas au quart de la fenêtre).
4. **Son** : menu « Sound Test » audible, sans craquements ; Espace maintenu = avance rapide muette.
5. **Clavier** : flèches, Entrée = Start, Z / X / C = A / B / C.
6. **Menu Échap** : pause (son coupé), Reprendre, Touches (changer une touche, relancer : conservée), Quitter.
7. **Savestate** : F2 sauvegarde, changer d'écran, F4 recharge ; F6/F7 change d'emplacement ; message OSD.
8. **SRAM** (facultatif : la 240p n'en a pas ; couvert par les tests du core factice) : jeu à pile fourni par
   l'utilisateur, sauvegarder en jeu, quitter, relancer → `saves/<core>/<jeu>.srm` présent et relu.
9. Relancer : `config.toml` présent (Unix : mode `600`).

Particularités :

| Plateforme | Avant de tester |
|---|---|
| Docker | `EMU_IMAGE=ghcr.io/jerhome/emu-rust:0.1.0 docker compose -f docker/compose.yaml up --no-build emu-wslg` (noVNC : `emu-novnc`, sans son, <http://localhost:6080/vnc.html>) ; core + ROM copiés dans le volume (`docker/README.md`) |
| Linux (WSL2) | `tar xzf`, dépendances : `sudo apt install libasound2t64 libasound2-plugins libxkbcommon-x11-0 libgl1 libegl1 libwayland-client0 libxcursor1 libxrandr2 libxi6` (22.04 : `libasound2`) ; son WSLg : `~/.asoundrc` = `pcm.!default pulse` / `ctl.!default pulse` |
| macOS Intel | dézipper, `xattr -dr com.apple.quarantine emu-rust.app` (ou clic droit → Ouvrir, une fois) ; même chose pour la `.dylib` téléchargée au navigateur (ou `curl -LO`) ; lancer `./emu-rust.app/Contents/MacOS/emu-rust --core … --rom …` (double-clic sans arguments : ne fait rien en 0.1) |
| Windows | dézipper, PowerShell : `.\emu-rust.exe --core .\genesis_plus_gx_libretro.dll --rom .\240pSuite-GenesisMD-1.21.zip` ; SmartScreen : « Informations complémentaires → Exécuter quand même » |

## 2. Workflows

### `release.yml` (tag `v*` ; `workflow_dispatch` avec `dry_run` = construit et téléverse les artefacts, sans GHCR ni release)

| Job | Runner | Permissions | Contenu |
|---|---|---|---|
| `check-version` | ubuntu-24.04 | `contents: read` | tag sans `v` == `version` du workspace (`cargo metadata`), sinon échec |
| `linux` | **ubuntu-22.04** | read | deps apt de `ci.yml` ; `cargo build --release --locked -p emu-app` ; `cargo about generate` ; `tar.gz` (binaire, `README.txt`, licences) |
| `macos` | macos-15-intel | read | `MACOSX_DEPLOYMENT_TARGET=10.13` ; build `x86_64-apple-darwin` ; `scripts/package-macos.sh` : `emu-rust.app/Contents/{Info.plist, MacOS/emu-rust, Resources/}` (+ licences et `README.txt` à côté du `.app` dans le zip), `codesign --force -s -`, `ditto -c -k --keepParent` |
| `windows` | windows-2025 | read | MSVC, build `x86_64-pc-windows-msvc` ; zip PowerShell `Compress-Archive` (exe, licences, `README.txt`) |
| `docker` | ubuntu-24.04 | read + **`packages: write`** | buildx, login `ghcr.io` avec `GITHUB_TOKEN`, `metadata-action` (tags `0.1.0`, `0.1`, `latest` ; label `org.opencontainers.image.source`), `build-push-action` `linux/amd64`, cache GHA |
| `release` | ubuntu-24.04 | **`contents: write`** | `needs` tous ; télécharge les artefacts, `SHA256SUMS.txt`, notes = section `## [0.1.0]` du `CHANGELOG.md`, `gh release create --prerelease --verify-tag` |

Règles : permissions par défaut `contents: read` ; actions épinglées par SHA (mêmes SHA que `ci.yml` quand l'action
y figure) ; `persist-credentials: false` ; `--locked` partout ; `cargo-about` installé à version fixe avec
`--locked` ; `concurrency` par tag sans annulation.

Fichiers d'empaquetage (`platform-build`) : `packaging/README.txt` (texte du §1 : lancement, URLs, Gatekeeper,
SmartScreen), `packaging/macos/Info.plist` (`CFBundleExecutable` `emu-rust`, `CFBundleIdentifier`
`io.github.jerhome.emu-rust`, `CFBundleShortVersionString` / `CFBundleVersion` substitués, `CFBundlePackageType`
`APPL`, `LSMinimumSystemVersion` `10.13`, `NSHighResolutionCapable` `true`), `about.toml` + `packaging/about.hbs`,
`scripts/package-{linux,macos}.sh`.

### `ci.yml` : job `windows`

`windows-2025`, MSVC stable : fmt, `clippy --workspace --all-targets -- -D warnings`, `test --workspace`,
`build --workspace --no-default-features`, `build --release -p emu-app`, artefact `emu-rust-windows-x86_64`.

### Points de vigilance par plateforme

| Sujet | Constat / attendu | Agent |
|---|---|---|
| Cale C sous MSVC | `cc` → `cl.exe` ; `vsnprintf` conforme C99 depuis VS 2015 ; le symbole `#[unsafe(no_mangle)] emu_libretro_log_sink` est résolu par `link.exe` (pas de problème d'ordre comme avec `ld`) ; à prouver par un test de la cale **non** gaté `unix` | `libretro-ffi` |
| Chemins passés au core (Windows) | UTF-8 via `to_string_lossy` (déjà) ; `warn!` si perte (non-UTF-16 valide) | `libretro-ffi` |
| Noms de fichiers Windows | `game_name_from_path` ne filtre que `/ \` et contrôles ; `entry_file_name` filtre les caractères interdits mais pas les **noms réservés** (`CON`, `PRN`, `AUX`, `NUL`, `COM0-9`, `LPT0-9`, y compris `COM¹²³`, insensible à la casse, aussi avec extension : `con.bin`) → un seul assainisseur partagé (suffixe `_`), caractères `<>:"|?*`, points / espaces finaux ; tests unitaires sur tous les OS. **Avant la release.** | `runtime-av` |
| HiDPI (Retina, Windows 150 %) | viewport et `capture` en pixels physiques ; `ScaleFactorChanged` géré | `runtime-av` (+ `ui` côté événements) |
| Contexte GL | macOS : 3.3 core demandé → CGL rend 4.1 core (profil « 3.2+ core »), GLSL `330 core` valide, VAO déjà créé ; Windows : WGL via `glutin-winit`. Logger version / profil / renderer obtenus en `info` (diagnostic des essais) | `runtime-av` |
| Audio | WASAPI / CoreAudio via cpal : formats f32/i16/u16/i32 déjà gérés ; vérifier un débit natif 44,1 / 48 kHz et l'absence de périphérique (« pas de son », pas d'échec) | `runtime-av` |
| Avertissements `cfg(unix)` | code `#[cfg(unix)]` dans `saves.rs`, `content.rs`, `config.rs` : aucun import / variable inutilisé sous Windows (clippy `-D warnings`) | chaque agent de crate |
| Messages d'échec de chargement du core | macOS : rappeler `xattr` ; Windows : architecture (DLL 32 bits) ; texte en français côté `emu-ui` | `ui` |

## 3. Version

- `[workspace.package] version = "0.1.0"` (+ `Cargo.lock`), **après U2** (évite le conflit sur `Cargo.lock`).
- `CHANGELOG.md` (format *Keep a Changelog*) : `## [0.1.0] - AAAA-MM-JJ` — Ajouté (phases 1, 1b, 2), plateformes,
  limites connues (CLI seulement, pas de manette, non signé). Le workflow en extrait les notes.

## 4. Découpage

| Tâche | Agent (modèle) | Fichiers | Dépend de | Quand |
|---|---|---|---|---|
| **P1** Workflows et empaquetage | `platform-build` (**opus** : permissions, chaîne d'approvisionnement) | `.github/workflows/{release,ci}.yml`, `packaging/`, `about.toml`, `scripts/package-*.sh`, `docker/compose.yaml` (`image: ${EMU_IMAGE:-emu-rust:dev}`), `docker/README.md` (section GHCR) | — | **maintenant** |
| **L1** Windows / macOS (FFI) | `libretro-ffi` (sonnet) | `crates/emu-libretro` : test de la cale de log tous OS, `warn!` chemin non UTF-8, propreté `cfg` | — | **maintenant** |
| **R3** Windows / macOS (runtime) | `runtime-av` (sonnet) | `crates/emu-runtime/src/{saves,content,gl,audio_out}.rs` : assainisseur partagé + tests, HiDPI, log GL, audio sans périphérique | — | **maintenant** |
| **U3** Finitions 0.1 | `ui` (sonnet) | `crates/emu-app/src/cli.rs` (`--version` via `CARGO_PKG_VERSION`), messages d'échec du core par OS, `ScaleFactorChanged`, `cfg` propres | **U2**, R3 | après U2 |
| **P2** Version et notes | `platform-build` (sonnet) | `Cargo.toml` (`version`), `Cargo.lock`, `CHANGELOG.md`, `README.md` (section « Release 0.1 » → `packaging/README.txt`) | U2 | après U2 (‖ U3) |
| **Q4** Vérification complète | `qa-systems` (sonnet) | fmt/clippy/test, `--ignored`, image runtime dans dind ; CI verte sur les 3 OS **dont Windows** ; `release.yml` en `dry_run` (contenu des 3 archives, `Info.plist`, `SHA256SUMS`) | P1, L1, R3, U3, P2 | ensuite |
| **RV** Revue | `reviewer` (opus) | `release.yml`, `ci.yml`, scripts, assainisseur, toute modif. de la cale C | Q4 | ensuite |
| **T** Tag | session principale | `git tag v0.1.0 && git push origin v0.1.0` | RV | fin |

Parallélisme : **P1 ‖ L1 ‖ R3 ‖ U2** tout de suite (périmètres disjoints) ; puis **U3 ‖ P2** ; puis Q4 → RV → T.
Sans compilateur Windows ici, le job Windows de `ci.yml` (P1, à pousser sur une branche) est la porte d'entrée :
L1 / R3 / U3 corrigent ce qu'il remonte. Après T : l'utilisateur passe le paquet GHCR en public (une fois) et suit
la checklist du §1.
