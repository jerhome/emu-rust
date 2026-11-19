# ADR 0008 — Frontière FFI, singleton de callbacks et propriété du contexte GL

Statut : accepté (2026-11-19)

## Contexte

- L'API libretro passe des pointeurs de fonctions C globaux, sans pointeur de contexte (`void *user_data`) :
  les callbacks vidéo / audio / entrées / environnement doivent retrouver l'état du frontend via un état global.
- `unsafe` n'est autorisé que dans `emu-libretro` et `emu-runtime`. Or créer un contexte OpenGL
  (`glutin`, `glow::Context::from_loader_function`) et appeler `glow` est `unsafe` : si `emu-ui` créait la fenêtre GL,
  il aurait besoin d'`unsafe`.
- `egui_glow` (phase 2) accepte un `Arc<glow::Context>` de façon sûre.

## Décision

### Callbacks libretro (`emu-libretro`)

- **Une seule instance de `Core` par processus** : garde `static` atomique ; un second `Core::load` renvoie
  `LibretroError::AlreadyLoaded`. La garde est relâchée dans `Drop` (après `retro_deinit` et `dlclose`).
- `Core` est **`!Send` et `!Sync`** : toutes les entrées dans le core (`retro_*`) se font sur le thread qui l'a chargé.
  Les callbacks reçus depuis un autre thread sont ignorés (valeur neutre) et loggés une fois.
- L'état d'environnement (format de pixels, répertoires, variables de core, géométrie, demande d'arrêt) appartient
  à `emu-libretro` et vit dans l'état global, valable de `Core::load` à `Drop`.
- Le gestionnaire du frontend (`&mut dyn FrontendHandler`) n'est installé dans l'état global **que pendant
  `Core::run`** (garde RAII qui le retire même en cas de panique). Hors de `run`, les callbacks vidéo / audio /
  entrées sont ignorés.
- Chaque `extern "C"` est enveloppé dans `catch_unwind` ; une panique du gestionnaire est mémorisée et `run`
  renvoie `LibretroError::CallbackPanicked`.
- Les pointeurs fournis par le core ne sortent jamais de l'appel : la vidéo est exposée en `VideoFrame<'_>` empruntée.

### Contexte GL (`emu-runtime`)

- `emu-ui` possède la **boucle d'événements et la fenêtre** (`winit`).
- `emu-runtime` possède **la création du contexte GL et de la surface** (`GlWindow`, via `glutin` + `glutin-winit`)
  et tout appel `glow`. Il expose `Arc<glow::Context>` pour `egui_glow` (phase 2) sans exiger d'`unsafe` côté appelant.
- Versions alignées sur l'écosystème egui (déclarées dans `[workspace.dependencies]`) :
  `winit 0.30`, `glutin 0.32`, `glutin-winit 0.5`, `glow 0.17`.

### Graphe de dépendances

Inchangé. `emu-runtime` ré-exporte les types d'`emu-libretro` utiles à l'UI (`AvInfo`, `JoypadButton`, …) pour
qu'`emu-ui` ne dépende pas d'`emu-libretro`. `emu-app` ne dépend que d'`emu-ui`.

## Conséquences

- Tests : un seul `Core` vivant à la fois ⇒ les tests d'intégration qui chargent un core se sérialisent
  (verrou `static Mutex<()>` dans le fichier de test) ; `cargo test` en threads parallèles reste sûr.
- Pas de core sur un thread dédié en phase 1. Si la phase 2 veut un thread d'émulation, `Core` sera créé **sur**
  ce thread (pas déplacé) ; l'UI communiquera par canaux.
- `GET_LOG_INTERFACE` exige un callback C variadique (`printf`), impossible en Rust stable : la commande renvoie
  `false` en phase 1 (les cores écrivent alors sur `stderr`). Une petite cale C (`cc`) sera décidée par une ADR en phase 2.
