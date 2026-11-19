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
