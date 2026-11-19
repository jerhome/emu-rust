# ADR 0012 — Release et distribution des binaires

Statut : accepté (2026-11-19)

## Contexte

- L'utilisateur veut une release **v0.1.0** testable sur 4 cibles : Docker, Linux, macOS Intel, Windows. Le build
  Windows, prévu en phase 6, est avancé ; la manette PS4 reste en phase 6.
- Le dépôt `jerhome/emu-rust` est public ; la CI (Linux + `macos-15-intel`) existe et est verte.
- Contraintes : pas d'OpenSSL, aucun core / BIOS / ROM dans les artefacts (ADR 0001, 0005), OpenGL ≤ 4.1 sur macOS
  (ADR 0002), cale C de log compilée par `cc` (ADR 0009).

## Décision

1. **Déclencheur** : `.github/workflows/release.yml` sur tag `v*` ; le tag doit égaler `[workspace.package] version`
   (sinon échec). Une seule source de version : `Cargo.toml` du workspace ; historique dans `CHANGELOG.md`.
2. **Artefacts** (nommés `emu-rust-<version>-<os>-x86_64.<ext>`), chacun avec `README.txt` (lancement, données,
   URLs des cores), `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY-LICENSES.html` (`cargo-about`, version épinglée) :
   - Linux : `tar.gz`, construit sur **`ubuntu-22.04`** (glibc 2.35, compatible avec plus de distributions que 24.04).
   - macOS Intel : bundle `emu-rust.app` (Info.plist, `LSMinimumSystemVersion` 10.13), signature **ad hoc**
     (`codesign -s -`), non notarisé, zippé avec `ditto`. Pas de dylib embarquée.
   - Windows : toolchain **MSVC** (`x86_64-pc-windows-msvc`, runner `windows-2025`), `zip`. Sous-système **console**
     (pas de `windows_subsystem = "windows"`) : l'application est pilotée en ligne de commande et logge sur stderr ;
     réévalué quand un lanceur graphique existera (phase 3/4, avec fichier de log).
   - `SHA256SUMS.txt` sur l'ensemble.
3. **Docker** : l'image runtime existante (multi-étapes) est publiée sur `ghcr.io/jerhome/emu-rust:<version>`,
   `:<majeur>.<mineur>` et `:latest`, `linux/amd64` seulement, avec `GITHUB_TOKEN` (`packages: write` limité au job).
   `docker/compose.yaml` accepte `EMU_IMAGE` pour utiliser l'image publiée sans build.
4. **Release GitHub** : créée par `gh release create` (pas d'action tierce) dans un job final `contents: write`,
   notes = section du `CHANGELOG.md`, marquée **pré-release** tant que la version est `0.x`.
5. **Sécurité de la chaîne** : permissions par défaut `contents: read`, élevées par job ; actions épinglées par SHA
   (commentaire de version) ; `persist-credentials: false` ; outils installés à version fixe (`cargo install --locked`).
6. **CI** : `ci.yml` gagne un job Windows (fmt, clippy `-D warnings`, test, build release) ; les tests du core factice
   restent `cfg(unix)` jusqu'à ce que l'utilitaire sache produire une `.dll`.

## Conséquences

- Avertissement Gatekeeper sur macOS (contournement documenté) et SmartScreen sur Windows (« Informations
  complémentaires → Exécuter quand même ») : acceptable pour un usage personnel.
- Le build Windows quitte la phase 6 ; celle-ci garde la manette et la CI « matrice complète ».
- Le premier push sur GHCR crée un paquet **privé** : l'utilisateur doit le passer en public une fois (ou faire
  `docker login ghcr.io`).
- Après août 2027 (fin de `macos-15-intel`), le build macOS Intel passera en cross-compilation depuis arm64.
