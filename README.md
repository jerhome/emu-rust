# emu-rust

Frontend [libretro](https://www.libretro.com/) écrit en Rust. L'émulation est faite par des cores libretro existants ;
emu-rust fournit l'image, le son, le clavier et les manettes (PS4, USB type SNES), la bibliothèque de jeux avec
jaquettes (thème sombre, manuel intégré, assistant de premier lancement), le Journal, le téléchargement des cores et
des BIOS libres. Usage personnel.

**Version stable : 7.6.0** ([releases](../../releases))

## Systèmes pris en charge

| Système | Core par défaut | Depuis |
|---|---|---|
| Mega Drive, Super Nintendo, NES, Game Boy / Color, Master System, Game Gear | genesis_plus_gx, snes9x, mesen, gambatte, genesis_plus_gx | 1.0 |
| Atari ST (EmuTOS libre téléchargé) | hatari | 2.0 |
| Mega CD (BIOS fourni par l'utilisateur) | genesis_plus_gx | 2.1, complété en 6.0 |
| Amiga (AROS intégré, Kickstart de l'utilisateur facultatif) | puae | 3.0 |
| PC MS-DOS (SoundFont libre GeneralUser GS sur action) | dosbox_pure | 4.0 |
| Amstrad CPC, Commodore 64 | cap32, vice_x64 | 5.0 |
| Game Boy Advance, 32X | mgba, picodrive | 6.0 |
| PlayStation, 3DO (démarrent sans BIOS ; BIOS de l'utilisateur préférés) | pcsx_rearmed, opera | 7.0 |
| Sega CD 32X (CD Mega CD + 32X, détection au scan ou marque manuelle ; aucun BIOS Sega 32X requis) | picodrive | 7.4 |

## Manettes

Le clavier marche partout. Manettes :

| Manette | Prise en charge | Essayée |
|---|---|---|
| PlayStation 4 (DualShock 4), USB | reconnue d'office (base SDL) depuis la 2.1 ; **PS** = touche de raccourci | Windows, macOS |
| Super Famicom USB (081f:e401) et pads USB type SNES | boutons devinés, puis apprentissage dans l'application (« appuyez sur A… »), mémorisé par manette ; **Select** = touche de raccourci | macOS (7.2) ; sous Windows, gauche / droite inopérants avant l'apprentissage (à corriger) |
| Autres manettes de la base SDL GameControllerDB (Xbox, 8BitDo…) | reconnues d'office | non essayées |

Détails des touches : [docs/manuel.md](docs/manuel.md).

## Utilisation

Les jeux se rangent dans `roms/<système>/`. Sous Windows, `emu-rust.exe` s'ouvre sans console ; `emu-rust-cli.exe` sert à la
ligne de commande (depuis PowerShell : `.\emu-rust-cli.exe --help`).

## Feuille de route

| Version | Contenu | État |
|---|---|---|
| 7.1 → 7.5 | Finition (pied de page, icône, Journal, messages copiables, terminal caché, `emu-rust-cli.exe`), manette USB type SNES (7.2), refonte visuelle (7.3), Sega CD 32X (7.4), fusion des travaux Mac et PC (7.5) | livrées (7.5.0) |
| 7.6 | Rangement des ROMs : « Ranger les ROMs… » (aperçu, choix du système, annulation, reprise après arrêt brutal ; sauvegardes, favoris et jaquettes suivent les jeux) | livrée (7.6.0) |
| 8.x | 8.0 systèmes libretro non testés ajoutés depuis l'application (cartouches, dont Atari Lynx) ; 8.1 archives .7z ; 8.2 interface en anglais ; 8.3 core jgenesis au choix ; 8.4 à 8.6 systèmes personnalisés CD, ordinateurs, arcade | 8.0 conçue, prochaine |
| 9.0 | Rendu matériel : OpenGL d'abord, puis DirectX ; Vulkan au second plan | prévue, après la 8.6 |

Détails : [docs/roadmap.md](docs/roadmap.md). Décisions d'architecture : [docs/adr/](docs/adr/).

## Lancer avec Docker (depuis la racine du dépôt)

| Hôte | Commande |
|---|---|
| Windows + WSL2 (WSLg) | `docker compose -f docker/compose.yaml up --build emu-wslg` |
| Linux (X11) | `docker compose -f docker/compose.yaml up --build emu-x11` |
| N'importe lequel, dans le navigateur (sans son) | `docker compose -f docker/compose.yaml up --build emu-novnc` puis <http://localhost:6080/vnc.html> |

Les données (cores, BIOS, ROMs, sauvegardes, covers) sont dans le volume Docker `emu-rust_emu-data`.
Détails (placer core et ROM, test du son) : [docker/README.md](docker/README.md).

## Installation / release

Les paquets sont publiés sur les [releases GitHub](../../releases) : archive Linux `tar.gz`, `.app` macOS Intel,
`zip` Windows et image Docker. Vérifiez les empreintes avec `SHA256SUMS`.

- Les cores libretro ne sont pas inclus : l'application les installe (écran Cores ou `--install-core recommended`).
  Les URL manuelles, la checklist de test et les ROM de test (domaine public / homebrew) figurent dans les notes de la release.
- Installation et premier lancement : [packaging/README.txt](packaging/README.txt).
- Docker : [docker/README.md](docker/README.md).

## Développement

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p emu-app
```

L'application ne télécharge jamais de ROM ni de BIOS commercial : fournissez vos propres fichiers.

## Licence

Au choix : [MIT](LICENSE-MIT) ou [Apache-2.0](LICENSE-APACHE). Les cores libretro ne sont pas distribués avec ce dépôt :
chacun garde sa propre licence.

Données embarquées : `crates/emu-assets/data/firmware.toml` (empreintes md5 de BIOS) est dérivé de
[libretro-database](https://github.com/libretro/libretro-database) (`System.dat`, contributeurs de libretro-database),
sous licence [CC-BY-SA-4.0](https://creativecommons.org/licenses/by-sa/4.0/) ; ce fichier reste sous la même licence.
Attribution complète : `packaging/THIRD-PARTY-DATA.txt`.
