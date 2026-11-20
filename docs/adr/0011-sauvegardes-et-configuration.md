# ADR 0011 — SRAM, savestates et fichier de configuration

Statut : accepté (2026-11-19). Précise ADR 0006 (emplacements). **Amendée le 2026-11-21** (section
« Amendement 2.1 » en fin de document) : identité de jeu (`save_name`), dossier de sauvegarde des homonymes,
ligne `game_fp` des savestates.

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

## Amendement 2.1 (2026-11-21) — identité de jeu

Conception et découpage : `docs/design/v2.1-identite-disques.md` (§2.1, §2.2). Contrat posé dans le code par I0
(`emu_runtime::{GameIdentity, IdentityNotice, IdentityRequest, identity_request_for, resolve_game_identity,
StateOrigin}`, `Emulator::{load_content_as, identity, content_name, state_origin}`, `StateHeader::game_fp`).

1. **Nom de jeu → `save_name`.** Le « nom de jeu » ci-dessus devient le **nom de base** ; la clé de la SRAM, des
   savestates, des options par jeu et du dossier `.disks` est le `save_name` attribué par un registre par core,
   `<saves>/<core_id>/.emu-rust-games` (verrou `.emu-rust-games.lock`, écriture atomique, borné à 8 Mio). Le
   premier jeu d'un nom garde le **nom nu** ; un homonyme (même nom de base, autre source principale) reçoit
   `<base> (n)`, `n ≥ 2` libre au registre et sans fichier existant de ce radical. Résolution : même chemin ⇒ même
   identité ; sinon même empreinte et ancien chemin disparu ⇒ même identité (déplacé ou renommé) ; sinon reprise
   d'un dossier `.disks` 2.0 par son manifeste ; sinon attribution. `NoGame` : `no-game`, hors registre.
2. **Aucun fichier existant n'est renommé, déplacé ni supprimé** : un jeu non homonyme garde ses fichiers 1.0 / 2.0
   (nom nu adopté). Homonymes déjà joués en 2.0 : `.srm` et options copiés au second (cible absente seulement),
   états laissés au premier et signalés. Registre illisible : jamais réécrit, comportement 2.0 pour la session.
3. **`GET_SAVE_DIRECTORY`** : `<saves>/<core_id>/` pour un nom nu (inchangé) ; `<saves>/<core_id>/<save_name>/`
   pour un homonyme (les fichiers qu'un core écrit lui-même portent le nom du contenu, que le frontend ne peut pas
   départager).
4. **Savestates** : ligne d'en-tête optionnelle `game_fp=<taille>:<crc32 hex>` (empreinte de la source principale ;
   CRC32 du fichier entier jusqu'à 64 Mio, sinon du premier et du dernier Mio). Format toujours en **version 1** :
   le lecteur 2.0 ignore la clé, un état sans `game_fp` est lu tel quel. Au chargement, empreinte différente ⇒
   `StateOrigin::OtherContent`, absente avec plusieurs homonymes ⇒ `Ambiguous` : l'UI demande confirmation
   (défaut Non), jamais de refus définitif.
5. Conséquences : interopérabilité RetroArch des `.srm` au nom nu conservée ; retour à la 2.0 sans perte pour les
   noms nus (les fichiers `<base> (n).*` sont alors ignorés). Schéma de `library.sqlite` inchangé (v3).
