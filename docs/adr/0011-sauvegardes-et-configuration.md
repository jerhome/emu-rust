# ADR 0011 — SRAM, savestates et fichier de configuration

Statut : accepté (2026-11-19). Précise ADR 0006 (emplacements).

## Contexte

Phase 2 : persistance de la SRAM, savestates à emplacements, configuration des touches éditable dans l'UI.
ADR 0006 fixe la racine de données et prévoit « `saves/ states/` par core puis par jeu » et `config.toml` (600).
Il manque : l'identifiant de core, le nommage, le format des savestates, la sûreté des écritures.

## Décision

### Racine et répertoires

- Racine : `--data-dir` > `EMU_DATA_DIR` > dossier standard (`directories`, `ProjectDirs::from("", "", "emu-rust")`,
  `data_dir()`) ; `/data` dans Docker. L'ancien défaut relatif `./data` (phase 1) disparaît.
- Ajout à l'arborescence d'ADR 0006 : `assets/` (`GET_CORE_ASSETS_DIRECTORY`). `config.toml` reste **à la racine**
  (un seul volume Docker).
- **Identifiant de core** (`core_id`) : nom du fichier sans extension ni suffixe `_libretro`
  (`genesis_plus_gx_libretro.so` → `genesis_plus_gx`), caractères hors `[A-Za-z0-9_-]` → `_`. Stable entre versions
  et entre OS ; sert aux dossiers et aux sections de configuration par core.
- **Nom de jeu** : nom du fichier de contenu sans extension (l'archive pour un `.zip`).

### SRAM

- `<data>/saves/<core_id>/<jeu>.srm` = contenu brut de `RETRO_MEMORY_SAVE_RAM` (compatible RetroArch).
- Le core reçoit `<data>/saves/<core_id>/` comme `GET_SAVE_DIRECTORY` (fichiers qu'il gère lui-même).
- Restaurée juste après `retro_load_game` ; écrite si modifiée : toutes les 10 s, à l'ouverture du menu, à la sortie.

### Savestates

- `<data>/states/<core_id>/<jeu>.state0` … `.state9` (10 emplacements, pas d'état automatique en phase 2).
- Conteneur maison : `EMURSTAT` + version (`u32`, = 1) + en-tête texte `clé=valeur` (`core_id`, `core_version`,
  `game`, `created_unix`) + charge utile (sérialisation libretro) + somme FNV-1a 64. `core_id` différent ⇒ refus ;
  version de core différente ⇒ avertissement puis tentative. Non compatible avec les `.state` RetroArch (accepté).

### Écritures

- Toujours **atomiques** (`emu_runtime::write_atomic`) : fichier temporaire dans le même dossier, `sync_all`,
  `rename`, fsync du dossier (Unix). Jamais de fichier tronqué après un arrêt brutal.
- `config.toml` : créé directement en `0600` (Unix) ; un fichier existant plus permissif est remis en `0600` avec un
  avertissement. Pas de secret en phase 2, mais le fichier en contiendra en phase 4 (ADR 0006).

### Configuration (`config.toml`)

- Sérialisation `serde` + `toml` (MIT/Apache-2.0) dans `emu-ui`, qui possède le type `Config` ; `emu-app` résout la
  racine et passe les chemins. Sections : `video`, `audio`, `speed`, `hotkeys`, `input.keyboard.<profil>`
  (`default` ou `core_id`), `core_options.<core_id>`.
- Fichier illisible ou invalide : avertissement, valeurs par défaut, et **le fichier n'est pas réécrit**
  tant que l'utilisateur ne modifie rien (pas d'écrasement silencieux).
- Touches désignées par les noms `winit::keyboard::KeyCode` (feature `serde` de `winit`), boutons par
  `JoypadButton::name` (stables, persistés).

## Conséquences

- Les sauvegardes survivent à une mise à jour du core (chemins par `core_id`, pas par `library_name`).
- La SRAM est interopérable avec RetroArch, pas les savestates.
- Les profils de touches sont par core en phase 2 ; ils deviendront par système avec la bibliothèque (phase 4).
