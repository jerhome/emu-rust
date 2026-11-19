# ADR 0009 — Code C : cale de log libretro et core factice de test

Statut : accepté (2026-11-19)

## Contexte

- `RETRO_ENVIRONMENT_GET_LOG_INTERFACE` attend un `retro_log_printf_t` **variadique** (`printf`) : impossible à
  définir en Rust stable (`c_variadic` est instable). En phase 1 la commande renvoie `false` (ADR 0008) et les cores
  écrivent sur `stderr`, hors de `tracing` (pas de niveaux, pas de filtre).
- Les tests de la phase 2 (SRAM, savestates, options v1/v2, messages, chargement en mémoire, log) ont besoin d'un
  core **déterministe et contrôlable**, disponible sans téléchargement. Un core factice en Rust (`cdylib`) exigerait
  de l'`unsafe` hors d'`emu-libretro` / `emu-runtime` (interdit).

## Décision

1. **Cale de log en C** dans `emu-libretro` : `src/log_shim.c`, compilée par `build.rs` avec le crate `cc`
   (build-dependency, MIT/Apache-2.0). Une seule fonction variadique `emu_libretro_log_printf` : `vsnprintf` dans un
   tampon de pile de 1024 octets (troncature signalée), puis appel d'un `extern "C"` Rust
   (`emu_libretro_log_sink(level, msg, truncated)`, `catch_unwind`) qui émet un événement `tracing`
   (cible `emu_libretro::core`, niveau mappé, fin de ligne retirée). Utilisable depuis n'importe quel thread et hors
   de `Core::run` : ne touche jamais au gestionnaire du frontend.
2. **Core factice en C** pour les tests : `tests/mock-core/mock_core.c` (périmètre `qa-systems`), compilé à la
   demande par un utilitaire partagé `tests/support/mock_core.rs` (inclus via `#[path]` par les tests des crates)
   en appelant le compilateur C du système (`$CC`, sinon `cc`) : `-shared -fPIC -O1`, sortie dans
   `target/test-output/mock-core/` (écriture dans un fichier temporaire puis `rename`, sûr entre binaires de test
   parallèles). Linux seulement pour l'instant (`#[cfg(target_os = "linux")]`). Ces tests ne sont **pas** `#[ignore]` :
   aucune ressource externe.
3. Le code C est soumis à la même exigence que l'`unsafe` Rust : revue par `reviewer`, commentaires sur les
   invariants (taille de tampon, terminaison NUL, pas de format venant de la cale elle-même).

## Conséquences

- Un compilateur C devient nécessaire au **build** d'`emu-libretro` (présent : conteneur de Claude, images Docker
  via l'image Rust, MSVC / MinGW sous Windows, clang sous macOS).
- Les messages des cores passent par `tracing` (filtrables via `RUST_LOG`) ; plus rien sur `stderr` pour les cores
  qui utilisent l'interface.
- Les fonctionnalités de la phase 2 sont testées à chaque `cargo test --workspace`, sans réseau ; le vrai core
  (genesis_plus_gx) reste en `#[ignore]` (ADR 0007).
