# ADR 0007 — Stratégie de tests

Statut : accepté (2026-11-19)

- **Unitaires** dans chaque crate (`cargo test --workspace`), sans réseau : serveurs HTTP simulés (`wiremock`).
- **Intégration headless** (`tests/`) : un core réel fait tourner une ROM homebrew N images ; on compare l'empreinte
  (hash) d'images de référence et on peut écrire PNG / WAV pour inspection. Aucun écran nécessaire.
- **Interface** : lancement sous `Xvfb` (rendu Mesa llvmpipe), captures d'écran vérifiées par l'agent `qa-systems`.
- **ROMs de test** : uniquement des homebrews à licence libre, téléchargés par `scripts/fetch-test-assets.sh`
  (URL + SHA256 épinglés), jamais commités. Idem pour les cores de test.
- Les tests qui nécessitent réseau, cores ou ROMs sont marqués `#[ignore]` et lancés explicitement
  (`cargo test -- --ignored`) après `scripts/fetch-test-assets.sh`.
- Le jeu réel (latence, son, ressenti) est validé par l'utilisateur sur l'image Docker WSLg.

## Amendement 2026-11-19

- **Core de test** (genesis_plus_gx) : nightly du buildbot, non épinglable. Politique TOFU : le hash est noté au premier
  téléchargement dans `tests/cores/*.sha256` (non versionné, propre à chaque machine) ; `--refresh-cores` met à jour.
  Un changement de nightly peut obliger à régénérer les empreintes d'images après vérification visuelle.
- **Emplacement** : le code des tests d'intégration vit dans `crates/<crate>/tests/` (la racine est un workspace virtuel) ;
  les données de test sont à la racine : `tests/roms`, `tests/cores`, `tests/fingerprints`. Le « `tests/` » ci-dessus s'entend ainsi.
- **Audio** : sortie réelle (`cpal`) en phase 1b. En phase 1, l'audio est vérifié via WAV / compte d'échantillons ;
  la fenêtre jette l'audio.
