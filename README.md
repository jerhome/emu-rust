# emu-rust

Frontend [libretro](https://www.libretro.com/) écrit en Rust. L'émulation est faite par des cores libretro existants ;
emu-rust fournit l'image, le son, le clavier et la manette PS4, la bibliothèque de jeux avec jaquettes, le
téléchargement des cores et des BIOS libres. Usage personnel.

**Version stable : 7.0.0** ([releases](../../releases))

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

Clavier par défaut, manette PS4 (DualShock 4) depuis la 2.1. Les jeux se rangent dans `roms/<système>/`.

## Feuille de route

| Version | Contenu | État |
|---|---|---|
| 7.1 | Finition de l'interface : pied de page (version, crédit), icône identique sur les 3 OS, terminal caché sous Windows avec panneau Journal, bouton « Ranger les ROMs » (aperçu et annulation), indication des BIOS d'origine qui améliorent un core | en développement |
| 7.2 | Sega CD 32X (Night Trap 32X…), par picodrive | conçue |
| 7.2.1 | Glisser-déposer de jeux dans la fenêtre depuis le gestionnaire de fichiers | étudiée |
| 8.0 → 8.3 | Systèmes libretro non testés, ajoutés depuis l'application : consoles à cartouche (8.0), CD (8.1), ordinateurs (8.2), arcade (8.3) | étudiée |
| 9.0 | Rendu matériel : OpenGL d'abord, puis DirectX ; Vulkan au second plan | prévue |

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
