# emu-rust

Frontend [libretro](https://www.libretro.com/) en Rust : Super Nintendo et Mega Drive d'abord, puis Atari ST, Amiga,
PC DOS et consoles 32 bits. Usage personnel.

- Feuille de route : [docs/roadmap.md](docs/roadmap.md)
- Décisions d'architecture : [docs/adr/](docs/adr/)

## Lancer avec Docker (depuis la racine du dépôt)

| Hôte | Commande |
|---|---|
| Windows + WSL2 (WSLg) | `docker compose -f docker/compose.yaml up --build emu-wslg` |
| Linux (X11) | `docker compose -f docker/compose.yaml up --build emu-x11` |
| N'importe lequel, dans le navigateur (sans son) | `docker compose -f docker/compose.yaml up --build emu-novnc` puis <http://localhost:6080/vnc.html> |

Les données (cores, BIOS, ROMs, sauvegardes, covers) sont dans le volume Docker `emu-rust_emu-data`.

## Développement

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p emu-app
```

L'application ne télécharge jamais de ROM ni de BIOS commercial : fournissez vos propres fichiers.
