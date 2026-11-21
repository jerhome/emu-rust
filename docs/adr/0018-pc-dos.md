# ADR 0018 — PC-DOS (version 4.0) : core dosbox_pure, contenus, modifications séparées, MIDI, clavier PC

Statut : **accepté** (2026-11-21, après re-validation contre le code réel de la 3.0 et de la 2.1 : amendement en fin
de document et `docs/design/v4.0-dos.md` §10) — décisions utilisateur du 2026-11-21 intégrées (SoundFont GeneralUser GS **autorisée** ; mode
d'entrée par défaut **Clavier** ; revue croisée : corrections I1, I4, I5, I6, I7, I8, I10, I13, M3). Réutilise ADR 0015
(infrastructure « ordinateur » de la 2.0), ADR 0017 (3.0 : « Importer un BIOS… », bouton du dossier des ROMs,
`DatSource::Libretro`) et les **règles communes du design 3.0** (`docs/design/v3.0-amiga.md` §3.2 `MediaRules`,
§3.5 : ordre d'évaluation des médias, `folder_extensions`, `game_saves` / `reset_core_game_files`, défauts calculés,
contrôle de firmware, notices) ; précise ADR 0005 (ressources libres téléchargées : amendement du 2026-11-21), 0013
(cores, BIOS), 0014 (DAT, jaquettes), 0016 (manette : axes analogiques, amendement `analog = true`). Étude préalable :
`docs/essais-dosbox.md` (essais headless des trois cores DOSBox avec l'appli 1.0.0). Conception détaillée :
`docs/design/v4.0-dos.md`. Graphe des crates inchangé.

## Contexte

La 4.0 ajoute le PC sous MS-DOS. Contrairement au ST et à l'Amiga, un jeu DOS n'est pas une image de disquette
mais une **arborescence de fichiers** (zip, dossier, ou image CD / disquette / disque dur), lancée par un **exécutable
à choisir**, et qui **écrit** dans ses propres fichiers (sauvegardes, `SETUP`, configuration du son). Le PC exige aussi
un clavier 102 touches complet (F1-F12, flèches, pavé numérique) alors que l'utilisateur joue sur un Razer 60 % AZERTY
(flèches, rangée F et pavé derrière Fn), et beaucoup de ses jeux sont des point-and-click à la souris
(`docs/questions-ouvertes.md`, 2.0 : mode *Clavier* par défaut). Cibles : Windows, macOS Intel 12.7.6, Linux, Docker.

Faits vérifiés le 2026-11-20 :

- **Buildbot** (`.index-extended` de https://buildbot.libretro.com/nightly/{linux/x86_64,windows/x86_64,apple/osx/x86_64}/latest/) :
  `dosbox_pure`, `dosbox_core`, `dosbox_svn` et `dosbox` présents sur les trois (nightlies du 2026-11-19 / 20).
  Le binaire de `dosbox_pure` est la version **`1.0-preview8`** (`retro_get_system_info`, `block_extract = 1`) ;
  son `.info` du buildbot (`assets/frontend/info.zip`) annonce encore `display_version = "0.9.9"`. Essayé avec
  l'appli 1.0.0 (`docs/essais-dosbox.md` §3) : rendu logiciel 640×400 puis 320×200 selon le mode, 70,09 images/s,
  48 kHz, zip à un exécutable lancé directement, menu de démarrage sinon, `bat.pure.zip` écrit dans
  `<saves>/dosbox_pure/`, FreeDOS démarré depuis une disquette.
- **`dosbox_pure`** : dépôt déplacé sur **Codeberg** (https://codeberg.org/schelling/dosbox-pure, `README.md` du
  dépôt GitHub d'origine https://github.com/schellingb/dosbox-pure) ; miroir libretro construit par le buildbot :
  https://github.com/libretro/dosbox-pure (poussé le 2026-11-05). Licence **GPL-2.0-or-later** (README, `LICENSE`).
  Le README impose une **« Strict No LLM / No AI Policy »** pour les tickets et contributions en amont.
  `.info` : `supported_extensions = zip|dosz|exe|com|bat|iso|chd|cue|ins|img|ima|vhd|jrc|m3u|m3u8|conf|/`,
  `needs_fullpath`, `supports_no_game`, savestates sérialisés, `disk_control`, `needs_kbd_mouse_focus`.
  Lecture du source (`dosbox_pure_libretro.cpp`, `dosbox_pure_run.h`, `dosbox_pure_pad.h`, `core_options.h`,
  `src/dos/drives.cpp`, `src/dos/drive_fat.cpp`) :
  - **zip / dosz** lus directement (montés en C:, images disque contenues montées en A: / D:) ; le zip n'est jamais
    écrit : les **modifications** vont dans `<GET_SAVE_DIRECTORY>/<nom du contenu>.pure.zip` (écrit 2 à 60 s après
    une écriture et au déchargement). Même chose pour un ISO / CHD / CUE chargé seul.
  - **dossier** ou **exe / com / bat** chargé directement : le dossier est monté en C: et **modifié en place** (« the
    saves directory is not used »). Une image **img / ima / vhd / jrc** chargée directement est ouverte en `rb+` et
    **modifiée en place** (`FindAndOpenDosFile`, `fatDrive`).
  - **Menu de démarrage** intégré : liste des exécutables du contenu (y compris dans les images montées), montage des
    images, démarrage d'une disquette (« booter »), installation d'un OS. **Démarrage auto** mémorisé dans
    `C:\AUTOBOOT.DBP`, donc dans le `.pure.zip` du jeu (flèche droite sur l'entrée). Un seul exécutable ⇒ lancé
    directement. Menu forcé si **Shift est enfoncé pendant `retro_reset`** (le core interroge
    `RETRO_DEVICE_KEYBOARD` / `RETROK_LSHIFT` dans `retro_reset`). Le menu propose aussi « Go to Command Line ».
  - Chemin de contenu `jeu.zip#PROG.EXE` : sans `AUTOBOOT.DBP`, le core lance `C:\PROG.EXE` ; le fragment ne peut pas
    contenir de `/` ni de `\` (`DBP_ExtractPathInfo` coupe au dernier séparateur) : **exécutable à la racine seulement**.
  - **MIDI** : SoundFonts `.sf2`, paires `*_CONTROL.ROM` + `*_PCM.ROM` (MT-32 / CM-32L, via munt) et `ROM1.BIN`…
    (SC-55) cherchés **récursivement dans le dossier système**, proposés comme valeurs de l'option
    `dosbox_pure_midi`. Ce parcours passe **uniquement** par `GET_VFS_INTERFACE` **version 3** (`opendir`,
    `readdir`…) : sans VFS, l'option n'offre aucune valeur. Cache `system/DOSBoxPureMidiCache.txt` écrit seulement si
    le parcours dépasse 2 s. Un `DOSBOX.SF2` / `MT32_*.ROM` dans le C: du jeu sert de surcharge par jeu.
  - Entrées : `SET_KEYBOARD_CALLBACK` + interrogation `RETRO_DEVICE_KEYBOARD` ; souris `RETRO_DEVICE_MOUSE`
    (relative, mode « virtual ») ou `RETRO_DEVICE_POINTER` (mode « direct ») ; manette : préréglages par port
    (port 0 « Generic Keyboard » = flèches, Échap, Entrée, Espace, Shift, Ctrl, Alt, 1-4, F1-F2 ; port 1 = pavé
    numérique ; joysticks DOS Gravis / 2 boutons / ThrustMaster lus sur `RETRO_DEVICE_ANALOG`) et
    correspondances automatiques par jeu (base Keyb2Joypad intégrée).
  - Options (`core_options.h`, v2) : `dosbox_pure_cycles` (auto, max, 315 … 1 000 000), `_memory_size` (16 Mo par
    défaut), `_machine` (svga, vga, ega, cga, tandy, hercules, pcjr), `_cpu_type`, `_cpu_core`,
    `_sblaster_type` (sb16 par défaut), `_gus`, `_midi` (défaut `frontend` = `GET_MIDI_INTERFACE`, que nous ne
    fournissons pas : musique MIDI muette), `_keyboard_layout` (us par défaut ; `fr`, `gr` = allemand,
    `uk`…), `_mouse_input`, `_mouse_speed_factor`, `_voodoo`, `_voodoo_perf`, `_force60fps`, `_savestate`,
    `_menu_time`, `_conf`, `_strict_mode`.
  - Voodoo : `SET_HW_RENDER` refusé ⇒ rendu logiciel (`voodoo_ogl_initfailed`). Thread d'émulation interne ; les
    messages du thread interne sont mis en file et livrés pendant `retro_run`, mais `log_cb` peut être appelé depuis
    ce thread.
- **`dosbox_core`** (https://github.com/realnc/dosbox-core, GPL-2.0, poussé le 2025-09-27) : ni zip ni savestates,
  dossiers modifiés en place, BASS optionnel ; **`dosbox_svn`** exige SDL 1.2 sur l'hôte (`.info`).
- **Base de données** : `libretro-database/dat/DOS.dat` (clrmamepro, version 2026.10.25, source
  https://github.com/robloach/libretro-database-dos, sans licence propre ; dépôt libretro-database sous
  CC-BY-SA-4.0) : 24 968 entrées, 15 230 titres, **une entrée = un fichier** (`.exe` 19 194, `.ima` 3 186, `.com`
  1 361, `.bat` 981, `.img` 246) avec **CRC32 + taille seulement** (ni md5 ni sha1), plus `genre`, `releaseyear`,
  `developer`. Aucun DAT TOSEC PC dans `libretro-database/metadat/tosec/`. Vignettes :
  https://github.com/libretro-thumbnails/DOS (`Named_Boxarts`, `Named_Snaps`, `Named_Titles`, `Named_Logos`).
- **MT-32** : `libretro-database/dat/System.dat`, section « DOS » : `MT32_CONTROL.ROM` (65 536 o, md5
  `5626206284b22c2734f3e9efefcd2675`), `MT32_PCM.ROM` (524 288 o, `89e42e386e82e0cacb4a2704a03706ca`),
  `CM32L_CONTROL.ROM` (65 536 o, `bfff32b6144c1d706109accb6e6b1113`), `CM32L_PCM.ROM` (1 048 576 o,
  `08cdcfa0ed93e9cb16afa76e6ac5f0a4`) : ROM Roland **commerciales**.
- **SoundFont libre** : GeneralUser GS 2.0.3 (https://github.com/mrbumpy409/GeneralUser-GS, fichier
  `GeneralUser-GS.sf2`, 32 319 396 o, SHA-256 `9575028c7a1f589f5770fccc8cff2734566af40cd26ed836944e9a5152688cfe`
  au commit `684543d5e5efaef08d02be50dcda8d552478fa60`) ; licence « GeneralUser GS License v2.0 » (usage sans
  restriction, redistribution et modification permises ; pas une licence OSI).

## Décision

### 1. Core

- Système `DOS` (nom de base libretro) dans `cores.toml` : **`dosbox_pure` seul**. `dosbox_core` et `dosbox_svn`
  ne sont pas proposés (pas de zip ni de savestates, écrivent dans les dossiers ; SDL 1.2 pour svn) ; ils restent
  utilisables par `--core <chemin>`.
- Propriétés par core (`cores.toml`, extension des tableaux 2.0 / 3.0) : `media = "working_copy"`,
  `media_direct = ["zip", "dosz", "iso", "cue", "chd", "ins"]` (ordre d'évaluation du design 3.0 §3.5.1 : une
  extension directe passe **avant** l'inspection des zips, donc un zip DOS n'est jamais extrait),
  **`folders = "snapshot"`** (§2), **`vfs = true`** (§5), **`analog = true`** (§7), **`featured_options` par
  système** (§4), **`layout_option`** (§6), `game_saves` (mécanisme 3.0), `options = { dosbox_pure_midi = "disabled" }`
  (état explicite, §5).
- Aucune contribution en amont rédigée par un LLM (politique du projet dosbox-pure) : un bogue du core est décrit à
  l'utilisateur, qui décide.

### 2. Contenus et rôle du frontend : les fichiers de l'utilisateur ne sont jamais modifiés

| Contenu | Transmis au core | Où vont les modifications |
|---|---|---|
| `.zip`, `.dosz` | tel quel (`media_direct`) | `<saves>/dosbox_pure/<nom>.pure.zip` (core) |
| `.iso`, `.cue` (+ `.bin`), `.chd`, `.ins` | tel quel | idem |
| **dossier** (jeu décompressé) | **instantané zip** `<data>/cache/dosbox_pure/<jeu>.zip` (§2.1) | idem |
| `.exe`, `.com`, `.bat` (CLI, ou choisi dans un dossier) | instantané du **dossier parent** + `#PROG.EXE` | idem |
| `.img`, `.ima`, `.vhd`, `.jrc` | **copie de travail** (`saves/dosbox_pure/<jeu>.disks/`, 2.0) | dans la copie |
| `.m3u`, `.m3u8` | `.m3u` de travail (2.0) : images directes en chemin absolu, images inscriptibles copiées | idem |
| `.conf` | **refusé** (le core monterait son dossier en C: et y écrirait) | — |

#### 2.1 Instantané d'un dossier

- Zip **stocké** (sans compression : accès aléatoire rapide), écrit dans un temporaire puis renommé ; manifeste
  `<jeu>.zip.manifest` (SHA-256 de la liste triée chemin relatif / taille / date). Réutilisé tant que le dossier
  n'a pas changé, reconstruit sinon : la progression est dans le `.pure.zip`, indépendant de l'instantané (le cache
  peut être vidé sans perte).
- Limites : 4 Gio, 65 535 fichiers, profondeur 16 ; liens symboliques non suivis ; noms non représentables
  (`..`, absolus, caractères de contrôle) ⇒ refus explicite. Construit dans un thread de travail avec progression et
  annulation **avant** `Emulator::load_content` (jamais sur le thread UI pour un gros dossier).
- Nom de l'instantané = nom du jeu (ADR 0011) assaini, sans `#` : le core nomme le `.pure.zip` d'après lui.
- Écartés : transmettre le dossier (écritures chez l'utilisateur) ; copie de travail du dossier (deux mécanismes de
  modification, et une copie reconstruite effacerait la progression).

#### 2.2 Exécutable : menu de démarrage du core, mémorisation par jeu

- Le **menu de démarrage de dosbox_pure** est le sélecteur : il voit aussi les exécutables des images CD / disquette,
  gère les jeux « booter » et l'installation d'un OS. Au premier lancement d'un jeu à plusieurs exécutables,
  l'OSD rappelle : « Choisissez le programme · → : le lancer automatiquement la prochaine fois ». Le choix est mémorisé
  **par le core** dans `AUTOBOOT.DBP` du `.pure.zip` du jeu (donc par jeu, hors des fichiers de l'utilisateur).
- Menu de la touche hôte : **« Changer de programme de démarrage »** = `retro_reset` avec Shift gauche signalé
  enfoncé (API générique `Emulator::reset_with_keys`). Elle **exige une API FFI nouvelle**, `Core::reset_with(handler)` :
  aujourd'hui `Core::reset` appelle `retro_reset` sans gestionnaire installé, et les callbacks d'entrée répondent une
  valeur neutre hors de `Core::run` (`with_handler`, `crates/emu-libretro/src/callbacks.rs`) — le core lirait donc
  toujours « Shift relâché ».
- Lancement d'un exécutable précis (`--rom JEU/START.EXE`, ou choix « Lancer un autre programme… » de la fiche du
  jeu, à la racine du dossier) : `instantané.zip#START.EXE`, mémorisé dans les options du jeu (`[dos] start`).
  Un exécutable hors de la racine ⇒ menu du core (limite du fragment).
- **« Réinitialiser les modifications du jeu »** (menu, confirmation, partie arrêtée) : supprime
  `<saves>/dosbox_pure/<nom>.pure.zip` (sauvegardes du jeu, `SETUP`, choix d'exécutable, correspondances manette),
  et la copie de travail le cas échéant — par `game_saves` + `reset_core_game_files`, **posés en 3.0** (design 3.0
  §3.5.3) : la 4.0 n'ajoute que les motifs de dosbox_pure.

### 3. Sauvegardes et savestates

- Le `.pure.zip` est un fichier du core dans `GET_SAVE_DIRECTORY` (ADR 0011) : rien à persister côté frontend ;
  `retro_unload_game` est toujours appelé avant de quitter ou de revenir à la bibliothèque (vidage final).
- Savestates : ceux du core (conteneur `EMURSTAT` inchangé). Avertissement OSD unique (comme la 2.0) : un état ne
  contient pas les fichiers modifiés et dépend des réglages vidéo / CPU / mémoire du moment.

### 4. Options par jeu

- Mécanisme 2.0 (`<data>/game-options/dosbox_pure/<jeu>.toml`) avec l'ordre de la 3.0 : core < `cores.toml` <
  défauts calculés < config < jeu (design 3.0 §3.5.4), plus une section **« Réglages DOS »** en tête de « Options de
  ce jeu », tirée de `featured_options` : vitesse (cycles), mémoire, type de CPU, carte graphique, carte son, MIDI,
  disposition du clavier, vitesse de la souris. Les autres options restent dans la liste complète. Options marquées
  « restart required » ⇒ « Redémarrer » (2.0).
- Défauts du core conservés (cycles `auto`, 16 Mo, SVGA, SB16, Voodoo logiciel). `featured_options` est une table
  générique de `cores.toml` **par (core, système)** dès la 4.0 (`[cores.<id>.featured_options] "<système>" = […]`) :
  un même core sert plusieurs systèmes aux réglages différents (genesis_plus_gx : Mega Drive, Master System, Game
  Gear, Mega CD ; picodrive : 32X, Mega CD).

### 5. MIDI et dossier système

- `emu-libretro` fournit **`GET_VFS_INTERFACE` v3** (fichiers et dossiers, implémentation `std::fs`) **seulement aux
  cores marqués `vfs = true`** ; les autres reçoivent `false` (choix 3.0 inchangé pour puae).
- Défaut : pas de MIDI (`dosbox_pure_midi = disabled` ; `GET_MIDI_INTERFACE`, sortie MIDI vers l'hôte, n'est pas
  fourni), musique AdLib / Sound Blaster.
- **SoundFont libre facultatif (autorisée par l'utilisateur le 2026-11-21, ADR 0005 amendée)** : GeneralUser GS
  2.0.3 dans le catalogue `free_bios.toml` (généralisé : `kind = "soundfont"`, fichier brut sans archive, taille et
  SHA-256 épinglés), installé en `system/GeneralUser-GS.sf2` **sur action de l'utilisateur** (écran Cores, ligne
  « SoundFont » du core, ou proposition dans « Réglages DOS » quand on choisit MIDI). **Limite de taille propre aux
  SoundFont** : le code livré borne une archive de BIOS à 16 Mio (`MAX_BIOS_ARCHIVE_BYTES`) et une image extraite à
  **4 Mio** (`MAX_BIOS_IMAGE_BYTES`, `crates/emu-assets/src/bios.rs`), et refuse une variante sans nom d'entrée ; le
  fichier fait 32 Mo. La 4.0 ajoute donc une limite **par type de ressource** (`soundfont` : 64 Mio, archive et
  fichier installé ; `bios` : valeurs actuelles inchangées) et accepte `entry` vide pour le seul format `raw`. Après
  installation : proposer « Utiliser pour tous les jeux DOS »
  (`[core_options.dosbox_pure] dosbox_pure_midi = "GeneralUser-GS.sf2"`). Non empaqueté, non redistribué ; licence et
  source citées dans le README de release et l'écran Cores (design 3.0 §3.5.7).
- **MT-32 / CM-32L** : ROM commerciales fournies par l'utilisateur via **« Importer un BIOS… »** (3.0), reconnues
  par md5 (`firmware.toml`, section « DOS » tirée de `System.dat`), installées sous `MT32_CONTROL.ROM` /
  `MT32_PCM.ROM` / `CM32L_CONTROL.ROM` / `CM32L_PCM.ROM`. Autres versions (fichier inconnu) : import non vérifié de la
  3.0 avec ces quatre noms proposés. Jamais téléchargées.
- Après toute installation / importation dans `system/` : supprimer `system/DOSBoxPureMidiCache.txt` s'il existe
  (sinon la nouvelle valeur n'apparaît pas).

### 6. Clavier PC sur clavier 60 %

- Correspondance positionnelle de la 2.0 (touche physique ⇒ `RETROK_*` US), **table globale inchangée** : elle
  envoie `RETROK_LESS` pour `IntlBackslash` (`<` / `>` de l'AZERTY) parce que hatari ignore `RETROK_OEM_102`
  (`crates/emu-ui/src/computer/keymap.rs`) ; la changer casserait la 2.0. Le PC obtient `RETROK_OEM_102` par la
  surcharge **`[remap]` de `pc.toml`** (`IntlBackslash = "OEM_102"`, mécanisme 3.0), jamais par la table globale.
- **Disposition DOS** = option `dosbox_pure_keyboard_layout`, **défaut calculé** (design 3.0 §3.5.4) déclaré en
  données dès la 4.0 : `layout_option = { option = "dosbox_pure_keyboard_layout", map = { fr = "fr", de = "gr",
  "en-GB" = "uk", … }, fallback = "us" }` ; réglable globalement (`[core_options.dosbox_pure]`) et par jeu comme
  toute option : un AZERTY tape ce qui est gravé. Il n'y a **pas** de clé `[computer] dos_keyboard_layout`. AltGr
  transmis (`RALT`), Alt gauche transmis (`LALT`), Ctrl gauche transmis ; **Ctrl droit = touche hôte**, jamais
  transmis. Sous Windows, Alt + F4 / Alt + Tab / Alt + Espace restent au système (guide d'essai) ; Alt + Entrée
  n'est pas un raccourci du frontend en mode ordinateur (il arrive à DOS : plein écran = Hôte + F).
- F1-F12, flèches, Inser / Suppr / Début / Fin / Pg↑ / Pg↓ arrivent par la couche Fn du Razer (l'OS envoie les vrais
  codes) : rien à faire. Pour le reste et pour le confort :
  - **mode Joystick** de la 2.0 (Hôte + J) : ZQSD / J / K ⇒ RetroPad port 0 ⇒ préréglage du core (flèches, Ctrl,
    Alt… ou correspondance automatique du jeu). Mode **Clavier** par défaut (décision utilisateur, 2.0) ;
  - **clavier virtuel** `data/keyboards/pc.toml` (102 touches, pavé numérique, F11 / F12, Pause, Arrêt défil.,
    légendes `us` / `fr` / `de`), Hôte + K.
- Pas de couche « pavé numérique » sur les lettres en 4.0 (question au design §9).

### 7. Souris, manette, joystick DOS

- Souris relative de la 2.0 (capture, repli sans capture sous noVNC) ; `dosbox_pure_mouse_input` reste `true`
  (mouvement virtuel). Sensibilité : celle du frontend (2.0) ; `dosbox_pure_mouse_speed_factor` par jeu.
- Manette (ADR 0016, livrée en **2.1**) : RetroPad port 0 comme partout ; **ajout des axes analogiques**
  (`RETRO_DEVICE_ANALOG`, sticks gauche et droit) dans `PadState`, nécessaires aux joysticks DOS analogiques et à la
  souris au stick des préréglages du core. ADR 0016 convertit toujours le stick gauche en D-pad : elle est **amendée**
  (2026-11-21) par le champ `analog = true` de `cores.toml`, qui coupe cette conversion pour le core et transmet les
  axes (zone morte commune). Le choix du joystick DOS (Gravis, 2 boutons, ThrustMaster) se fait dans le « Controller
  Mapper » du core (mémorisé dans `PADMAP.DBP` du `.pure.zip`). Sans manette, les directions numériques suffisent.
- Le menu interne du core (L3) reste disponible ; le frontend ne lit pas `SET_CONTROLLER_INFO` en 4.0.

### 8. CD-ROM et disques

- ISO / CUE+BIN / CHD transmis tels quels (lecture seule, MSCDEX émulé par le core, pistes audio CUE / CHD).
  Plusieurs CD / disquettes : regroupement de la bibliothèque (2.0) étendu à `(Disc N)`, `(CD N)` ; changement par le
  menu **Disquettes / CD** (interface disque étendue, 2.0) et Hôte + D.
- **Règles CD de la bibliothèque, posées en 4.0 et indépendantes du système** (elles ne sont pas attendues de la
  6.0) : un `.cue` est lu (`cue_references`, borné à 64 Kio) et les fichiers qu'il référence sont **masqués** —
  sinon le `jeu.bin` d'un CD DOS serait listé comme un jeu Mega Drive (`bin`) ; une image CD (`iso`, `chd`, `cue` et
  ses pistes) n'est **jamais hashée** par le scan (identification par le nom ; la 6.0 ajoute la taille + CRC de la
  piste 01 pour Redump).
- Zip contenant des images : monté par le core, rien à faire. Jeu en deux parties (zip + ISO séparé) : `.m3u8` de
  l'utilisateur, ou l'ISO placé dans le zip (documenté).

### 9. Bibliothèque et identification

- Système `id = "dos"`, `database = "DOS"`, `kind = "computer"`, `dat = ["libretro"]` (3.0), ScreenScraper 135 et
  IGDB 13 à vérifier. Nouveau champ **`folder_games = true`** : sous un dossier dont un composant vaut `dos`
  (règle des dossiers de la 1.0), un sous-dossier contenant au moins un `.exe` / `.com` / `.bat` à sa racine **est un
  jeu** (son contenu n'est plus parcouru). Exécutables isolés à la racine du dossier `dos` : ignorés et comptés
  (« rangez chaque jeu dans un dossier ou un zip »). Extensions ambiguës (`iso`, `cue`, `chd`, `img`) :
  seulement sous un dossier `dos`, par le champ **`folder_extensions` posé en 3.0** (design 3.0 §3.5.2). Schéma de la
  bibliothèque : **v5** (`game.is_dir`).
- **Identification par DAT `dat/DOS.dat`** (téléchargé à l'exécution, jamais redistribué) : CRC32 + taille des
  entrées d'un zip lus dans le **répertoire central** (aucune décompression), ou des exécutables / images de la
  racine d'un dossier (≤ 64 fichiers, ≤ 16 Mio chacun). Une entrée du DAT compte si CRC, taille **et** nom de fichier
  (casse ignorée) correspondent ; le titre qui a le plus de correspondances l'emporte, égalité ⇒ non identifié ;
  repli sur le nom (règle 1.0). ISO / CHD : identification par le nom seulement.
- **Écartés** : TOSEC « IBM PC Compatibles » (absent de libretro-database, URL instables) ; **eXoDOS** (collection
  de jeux commerciaux redistribués : ni téléchargement ni import de ses métadonnées).
- Jaquettes : libretro-thumbnails `DOS` (titres du DAT), puis ScreenScraper / IGDB (1.0).

### 10. Écarts avec la 2.0 et la 3.0

| Sujet | 2.0 (ST) / 3.0 (Amiga) | 4.0 (DOS) |
|---|---|---|
| Unité de jeu | image(s) de disquette | zip, dossier, image CD / disque |
| Écritures | copie de travail des images | `.pure.zip` du core ; instantané zip pour les dossiers ; copie de travail pour img / vhd |
| Lancement | boot de la disquette | exécutable choisi (menu du core, mémorisé par le core) |
| BIOS | EmuTOS téléchargé / Kickstart importé (AROS en repli) | aucun ; MT-32 importé facultatif ; SoundFont libre facultatif |
| FFI | 12, 13, 57, 58 | + VFS v3 (opt-in par core) ; `Core::reset_with(handler)` (reset avec touches tenues) |
| Manette | numérique (stick ⇒ D-pad) | + axes analogiques pour les cores `analog = true` (ADR 0016 amendée) |
| Bibliothèque | TOSEC / WHDLoad, groupes de disquettes ; schéma v4 | DAT `dat/DOS.dat` par fichier, jeux-dossiers, `.cue` masquant ses pistes, images CD non hashées ; schéma v5 |
| Défauts calculés | repli de firmware (3.0) | + `layout_option` (langue ⇒ disposition) |
| Réglages mis en avant | — | `featured_options` par (core, système) |

## Conséquences

- Nouvelles API (détail : design §3) : `emu_libretro::CoreConfig::provide_vfs`, `Core::reset_with` ;
  `emu_runtime::{snapshot_folder, snapshot_status, SnapshotLimits, ContentSpec::Folder}`,
  `Emulator::reset_with_keys`, `PadState` analogique ; `emu_assets` : `FreeBiosKind`, archive `raw`, limite par type
  de ressource, `CorePreferences::{folder_policy, provides_vfs, featured_options(core, système), layout_option,
  analog}` ; `emu_library` : `SystemInfo::folder_games`, jeux-dossiers, identification DOS, `cue_references`.
  Déjà posés en 3.0 et seulement alimentés en données : `game_saves` / `reset_core_game_files`, `folder_extensions`,
  couche « défauts calculés ». Graphe inchangé, aucune nouvelle dépendance (zip écrit avec le crate `zip` déjà
  présent).
- `unsafe` nouveau aussi dans `Core::reset_with` (installation du gestionnaire autour de `retro_reset`, comme
  `Core::run`).
- `unsafe` nouveau : les 19 fonctions VFS `extern "C"` d'`emu-libretro` (revue `reviewer`).
- Réseau nouveau : le SoundFont (32 Mo, hôte `raw.githubusercontent.com`, SHA-256 épinglé) et le DAT `DOS.dat`
  (canal DAT existant).
- Disque : un instantané par jeu-dossier lancé (cache supprimable, bouton « Vider le cache DOS » dans les Réglages).
- La version `dosbox_pure` est une « preview » nightly : régressions possibles, atténuées par la version précédente
  conservée (ADR 0013) et les empreintes de test.
- Les mécanismes génériques posés ici (instantané de dossier, VFS opt-in, `featured_options` par système,
  `layout_option`, axes analogiques, reset avec touches, règles CD de la bibliothèque) servent à d'autres cores
  (cap32, genesis_plus_gx / Mega CD, PlayStation et 3DO en 7.0, ScummVM).
- L'axe analogique (tâche DG1) dépend de la manette livrée en **2.1** ; sans elle, DG1 est reporté sans bloquer la 4.0.

## Amendement du 2026-11-21 — re-validation contre le code (3.0 posée, 2.1 livrée)

Les décisions ci-dessus sont confirmées. Précisions (détail : design 4.0 §10) :

- **Identité de jeu (2.1)** : l'instantané d'un dossier porte le `save_name` du jeu ; le `.pure.zip` est écrit dans le
  dossier de sauvegarde de l'identité (`saves/dosbox_pure/`, ou `saves/dosbox_pure/<save_name>/` pour un homonyme) :
  deux jeux de même nom ne partagent plus leurs modifications. `<nom>` = `Emulator::content_name()`.
- **Règles CD de la bibliothèque (§8)** : `cue_references`, `DISC_EXTENSIONS`, le masquage des pistes et l'absence de
  hash ont été livrés par la 2.1 (Mega CD express, `emu-library/src/disc.rs`) ; la 4.0 les généralise au lieu de les
  poser. Elle restreint `cue` / `chd` / `iso` au dossier pour `megacd` comme pour `dos` et garde la sonde d'en-tête
  Sega CD hors dossier ; un `.chd` hors dossier devient « non classé ». Le refus « CD dans un zip » de la 2.1 ne
  s'applique pas aux zips DOS.
- **Jeux-dossiers (§9)** : `Game::is_dir` (schéma v5 = cette seule colonne) ; les métadonnées `genre` /
  `releaseyear` / `developer` de `DOS.dat` ne sont pas importées en 4.0.
- **MT-32 (§5)** : la section « DOS » de `firmware.toml` existe depuis la 3.0 (A3) ; la 4.0 n'y ajoute rien.
- `m3u8` est traité comme `m3u` dans les trois crates qui lisent des listes.

## Amendement du 2026-11-21 (2) — rechargement de dosbox_pure dans le même processus

dosbox_pure reste en mémoire après `dlclose` (liaisons de `libstdc++` vers ses symboles C++) et plante au 2e
`retro_load_game` du même processus (« Redémarrer le jeu », jeu suivant depuis la bibliothèque). Décision
transversale : **ADR 0022** (détection « resté en mémoire » après déchargement, puis chargement d'une copie privée
sous un nom unique dans `<data>/cache/core-copies/`). Aucune clé `cores.toml` ; `CoreConfig::copy_dir` et
`LibretroError::CoreCopy` s'ajoutent aux API du §« Conséquences ». Détail et découpage : design 4.0 §10 n° 21.
