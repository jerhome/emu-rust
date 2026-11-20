# emu-rust

Frontend [libretro](https://www.libretro.com/) en Rust : Mega Drive, Super Nintendo, Game Boy, NES, Master System et
Game Gear (1.0), puis Atari ST, Amiga, PC DOS. Usage personnel.

- Feuille de route : [docs/roadmap.md](docs/roadmap.md)
- Décisions d'architecture : [docs/adr/](docs/adr/)

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
