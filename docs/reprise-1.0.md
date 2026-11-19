# Reprise de la 1.0 (travail arrêté le 2026-11-20)

La 1.0 a été mise en pause à la demande de l'utilisateur pour finir la 0.1. Tout le travail est sur la branche
`wip-1.0` ; `main` ne contient que la 0.1 (+ tests et documentation). Contenu prévu de la 1.0 : `docs/roadmap.md`
(section « Versions »), conception : `docs/design/v1.0-cores.md`, `docs/design/v1.0-bibliotheque.md`, ADR 0013 et 0014.

État du workspace sur `wip-1.0` au moment de l'arrêt : `cargo fmt --check`, `cargo clippy --workspace --all-targets
-D warnings` et `cargo test --workspace` passent.

## Fait

| Tâche | Crate | Revue | Notes |
|---|---|---|---|
| L1 bibliothèque : scan, hash, DAT, SQLite | emu-library | revue + corrections validées | schéma v2 (`pinned`), lots hors transaction, budget DAT |
| A1 HTTP (rustls), jobs, extraction des cores | emu-assets | VALIDÉE | redirections ≤ 3, même hôte, tailles bornées |
| A2 gestionnaire de cores | emu-assets | VALIDÉE (2 revues) | verrou inter-processus `<cores>/.lock`, SHA-256, `.prev`, BIOS vérifiés (md5, CC-BY-SA) |
| L2 jaquettes, DAT, images, secrets | emu-assets | **non revue** | libretro-thumbnails → ScreenScraper → IGDB, cache borné, `Secret` |
| U3/U5 config bibliothèque, identifiants, CLI cores | emu-ui, emu-app | non revue en dédié | `--list-cores`, `--install-core`, etc. |
| U6 écran Cores, `plan_launch`/`prepare_launch` | emu-ui | non revue | taille de fenêtre selon l'aspect |
| U4 bibliothèque (accueil, réglages, lancement, retour) | emu-ui, emu-app | **non revue** | `emu-rust` sans argument ouvre la bibliothèque |
| P2 Docker / packaging 1.0 | docker, packaging | — | `/data/{cores,covers,database}`, attribution CC-BY-SA |
| Profils clavier SMS / GG / GB / NES | emu-ui | — | choix par extension |
| Tests SMS/GG (genesis_plus_gx), GB/NES (gambatte, mesen…) | tests | — | 17 + 79 tests `#[ignore]` |
| Fixtures `tests/data/` | tests | — | DAT, ROM factices, zip piégés, JSON jaquettes |
| Tests d'intégration du gestionnaire de cores | emu-assets/tests | — | archives, HTTP défaillant, bout en bout |

Cores retenus : genesis_plus_gx (MD/SMS/GG), snes9x (SNES), **gambatte** (GB/GBC), **mesen** (NES, OK depuis le
correctif de l'ordre d'initialisation libretro, intégré à la 0.1). Nestopia écarté (bug du core), SameBoy refusé
(débit audio 2 MHz > `MAX_RATE`).

## Reste à faire

1. **Brancher les jaquettes dans l'UI** (U4 ↔ L2) : implémenter `CoverProvider` avec `CoverChain`/`CoverCache`/
   `decode_cover` (thread de travail, `set_cover`, textures limitées aux cartes visibles), état « hors ligne »,
   « Rechercher à nouveau » (`clear_missing_covers`).
2. **DAT** : activer « Mettre à jour la base (DAT) » (`download_dat`) et le proposer au premier scan.
3. **Écran Cores** : passer `remove`, `rollback`, `set_pinned` en `Job` (ils peuvent attendre le verrou de dossier) ;
   proposer « Réinstaller » quand `rollback` renvoie `Tampered` ; relancer le jeu après installation depuis le dialogue.
4. **Revues** à faire : L2 (réseau, secrets, images), U4/U6/U3-U5 (UI), puis revue finale V1.
5. **Vérification complète Q2** + captures Xvfb (une commande `--scan` sans fenêtre rendrait la préparation déterministe).
6. Petits points : clavier Gauche/Droite dans les Réglages ; fenêtre non redimensionnée au jeu en mode bibliothèque ;
   commentaires « task U4 » obsolètes dans `cores_ui` ; test du retour arrière sans fichier courant (`manager.rs`) ;
   remonter les versions de dépendances dans `[workspace.dependencies]` ; vérifier `cargo about` (moxcms, pxfm).
7. **Packaging** : `.github/workflows/release.yml` l.257 (zip Windows) doit copier `packaging/THIRD-PARTY-DATA.txt`
   (édition refusée à un agent par le contrôle de permissions : décision de l'utilisateur).
8. Essai utilisateur sur ses 4 plateformes, puis pré-release 1.0.

Décisions par défaut et points ouverts : `docs/questions-ouvertes.md` (lignes « 1.0 »).
