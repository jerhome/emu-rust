# emu-rust — Instructions projet

Frontend **libretro** écrit en Rust : il charge des cores libretro existants (`.so`/`.dylib`/`.dll`) et fournit
image, son, entrées, bibliothèque de jeux, téléchargements (cores, BIOS libres, covers). Usage **personnel**.
Feuille de route : `docs/roadmap.md`. Décisions d'architecture : `docs/adr/` (elles font autorité).
Vocabulaire de nos échanges : `docs/glossaire.md`.

Conventions communes de mes projets Rust : `../agents-home/docs/rust/CONVENTIONS.md` (Home Repo `../agents-home`,
lis son `AGENTS.md`). En cas de conflit, ce fichier et les ADR du repo priment.

## Contraintes non négociables

- **Pas de cores maison** : l'émulation est faite par les cores libretro (ADR 0001).
- **Aucun téléchargement de ROM ou de BIOS commercial.** Seulement : cores libretro, BIOS libres (EmuTOS ; AROS est intégré au core puae),
  covers/métadonnées, ROMs du domaine public ou homebrew, SoundFont libre GeneralUser GS (sur action, 4.0). L'utilisateur fournit le reste ; l'app vérifie les hashes (ADR 0005).
- **`unsafe`** interdit sauf dans `emu-libretro` et `emu-runtime` ; chaque bloc `unsafe` a un commentaire `// SAFETY:`.
- **Cibles** : image Docker (WSLg / X11 / Wayland / noVNC), puis Linux, Windows, macOS **Intel** (`x86_64-apple-darwin`,
  OpenGL ≤ 4.1). Code spécifique OS via `#[cfg(...)]`.
- **Entrées** : clavier d'abord ; manette PS4 (DualShock 4) plus tard. Ne jamais coder en dur une hypothèse « clavier seulement ».
- **Pas d'OpenSSL** : `rustls` partout.
- **Secrets** (identifiants ScreenScraper, clés IGDB) : jamais en dur, jamais loggés, jamais commités ; fichier de config en `600`.
- **Commentaires** en anglais, uniquement quand ils apportent quelque chose (invariant, raison d'un choix, piège).
- Économie de tokens : lire uniquement les fichiers nécessaires.

## Environnement de développement

Claude tourne dans un conteneur **sans écran, sans son, sans `/dev/input`**. Il a un démon Docker
dédié (service `dind`, `DOCKER_HOST` déjà réglé), séparé de celui de l'hôte.
- Tests : headless (empreintes d'images, WAV) ; captures d'interface via `Xvfb` (installé).
- Docker : les images se construisent et se testent en headless ici (dind) ; l'essai réel (WSLg, son, clavier)
  est fait par **l'utilisateur** sur son hôte (Windows + WSL2). Disque limité : `docker image prune` après usage.
- Pas de build macOS possible ici ; Windows plus tard par cross-compilation.
- **Avant toute commande cargo** : `. ~/.local/udev/env.sh; export CARGO_INCREMENTAL=0` (sinon `libudev-sys` échoue :
  `libudev.pc` introuvable), et `CARGO_TARGET_DIR=<racine du worktree>/target` : on travaille dans le worktree
  indiqué par la consigne, jamais dans un autre.
- **Disque** : `df -h /workspace` avant un build. Jamais de nouveau dossier `target` (ni d'autre `CARGO_TARGET_DIR`).
  Sous 15 Go libres : ne pas lancer le build, le signaler dans le rapport.

## Architecture (workspace Cargo)

```text
crates/
├── emu-libretro   # FFI libretro : chargement dynamique, callbacks d'environnement, savestates
├── emu-runtime    # boucle d'émulation, vidéo GL, audio, entrées, timing
├── emu-library    # scan ROMs, hash CRC32/SHA1, identification (DAT / libretro-database), SQLite
├── emu-assets     # téléchargements : cores (buildbot), BIOS libres, covers (3 sources)
├── emu-ui         # interface egui
└── emu-app        # binaire `emu-rust` : assemblage, config, CLI
```

Dépendances autorisées : `emu-app → emu-ui → (emu-runtime, emu-library, emu-assets)` ; `emu-runtime → emu-libretro`.
`emu-libretro`, `emu-library` et `emu-assets` ne dépendent d'aucun autre crate du workspace.

## Commandes

- Vérification complète : `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- Lancer : `cargo run -p emu-app`

## Répartition des agents (`.claude/agents/`)

| Agent | Périmètre |
|---|---|
| `architecte` | ADR, interfaces publiques entre crates, découpage des tâches. Pas de code applicatif. |
| `libretro-ffi` | `crates/emu-libretro` |
| `runtime-av` | `crates/emu-runtime` |
| `library-assets` | `crates/emu-library`, `crates/emu-assets` |
| `ui` | `crates/emu-ui`, `crates/emu-app` |
| `platform-build` | `docker/`, `scripts/`, CI, packaging |
| `qa-systems` | `tests/` (données), `crates/*/tests/` (intégration), configs par système, vérification complète, captures Xvfb |
| `reviewer` | lecture seule : `unsafe`, sécurité des téléchargements, licences |
| `triage` | lecture seule : diagnostic des échecs et routage vers le bon agent |

Ordre type d'une fonctionnalité : `architecte` (interface + découpage) → agents de crate en parallèle (un crate chacun)
→ `qa-systems` (vérification complète) → `reviewer` sur tout changement `unsafe` ou réseau.
Un agent ne modifie jamais un crate hors de son périmètre : il décrit le besoin dans son résumé final.
Les agents communiquent via des résumés courts : fichiers modifiés, API publique ajoutée, points ouverts.

## Définition de « terminé »

`fmt`, `clippy -D warnings` et `test` passent sur tout le workspace ; aucun test n'appelle un vrai service réseau
(mocks `wiremock`) hors tests `#[ignore]` explicites ; `reviewer` a validé tout `unsafe` ou code réseau nouveau.

### Boucle d'échec (orchestrée par la session principale)

Un sous-agent ne peut pas en lancer un autre : c'est la session principale qui enchaîne les étapes.

1. `qa-systems` rend `VERDICT : À CORRIGER` (ou une commande échoue) → lancer `triage` avec la sortie d'erreur.
2. `triage` rend un rapport « → agent / → environnement » et un `ORDRE`.
3. Lancer les agents désignés dans cet ordre (en parallèle seulement si `triage` dit « indépendants »),
   en leur donnant uniquement leurs lignes du rapport.
4. Relancer `qa-systems`. Maximum **3 cycles** sur le même échec, puis s'arrêter et demander à l'utilisateur.
5. Les points « environnement / utilisateur » ne sont confiés à aucun agent : les remonter tels quels.
