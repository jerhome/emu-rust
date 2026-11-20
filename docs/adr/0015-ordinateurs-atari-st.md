# ADR 0015 — Ordinateurs (version 2.0, Atari ST) : core, BIOS libre, clavier, souris, disquettes, options par jeu

Statut : accepté (2026-11-20). Réponses de l'utilisateur (`docs/questions-ouvertes.md`) : touche hôte Ctrl droit
(partout, macOS compris ; ⌘ droit en repli réglable — décision du 2026-11-20) ; EmuTOS 192 Ko **français** ; mode d'entrée par défaut d'un jeu = **Clavier** (jeux surtout
point-and-click : la souris est prioritaire) ; 2.0 d'abord, manette en parallèle. Autres questions de
`docs/design/v2.0-atari-st.md` §9 : défauts recommandés. Précise ADR 0003 (entrées
« ordinateur »), 0005 (BIOS libres), 0006 / 0011 (emplacements), 0013 (cores, BIOS), 0014 (catalogue, DAT).
Points de contact avec la manette : ADR 0016. Étude préalable : `docs/essais-hatari.md`.

## Contexte

La 2.0 ajoute l'Atari ST et toute l'infrastructure « ordinateur » réutilisée par la 3.0 (Amiga, puae + AROS) et la 4.0
(DOS, dosbox_pure) : clavier complet, clavier virtuel, souris capturée, disquettes multiples et changement à chaud,
BIOS libre téléchargé et vérifié, options par jeu. L'utilisateur joue au **clavier** (clavier 60 % AZERTY sans flèches,
rangée F ni pavé : accès par Fn) ; la manette PS4 (ADR 0016) viendra en parallèle ou après, sans être requise.

Faits vérifiés le 2026-11-20 :

- **Buildbot** (`.index-extended` des 3 plates-formes, https://buildbot.libretro.com/nightly/{linux/x86_64,windows/x86_64,apple/osx/x86_64}/latest/) :
  `hatari_libretro` (nightly 2026-11-16 à 2026-11-20), `hatarib_libretro` et `hatari2014_libretro` présents sur les trois.
- **`hatari`** (https://github.com/libretro/hatari, « rebasing of Hatari based on v2.6.1 upstream », actif en 2026-11) :
  GPL-2.0, `display_version = "tech preview"`, binaire `ext = st|msa|dim|stx|ipf|m3u|ide|vhd|gem`, `need_fullpath = 1`,
  `block_extract = 0` : **ne lit pas les zip** malgré son `.info` ; capsimage lié statiquement (IPF/STX/CTR lus) ;
  `supports_no_game` ; `SET_KEYBOARD_CALLBACK` (12) ; souris par `RETRO_DEVICE_MOUSE` (X/Y relatifs, boutons) ;
  `SET_DISK_CONTROL_EXT_INTERFACE` (58) seulement, sinon « disk swapping via the frontend won't be available » ;
  `.m3u` (chemins relatifs au `.m3u` ou absolus) ; **`tos.img` obligatoire** à la racine du dossier système (nom fixe,
  `firmware0_opt = false`, md5 `.info` = TOS 1.02 commercial) ; écrit **dans les images disque** par défaut
  (`hatari_floppy_write_protection = off`) ; NVRAM dans `<saves>/hatari/` ; clavier virtuel interne (`hatari_vkbd`,
  activé par défaut, bouton X du RetroPad). Sources : `src/retro/{main_retro,keymap,disk_control,gui_event,options}.c`.
- **`hatarib`** (https://github.com/bbbradsmith/hatariB, v1.1, GPL-2.0) : EmuTOS **intégré** (utilise `system/tos.img`
  s'il existe), lit zip/gz, `needs_fullpath = false`, écrit les disquettes modifiées dans `saves/` (jamais l'original),
  savestates déterministes ; IPF/CTR seulement avec `capsimg` fourni par l'utilisateur.
- **EmuTOS** (https://emutos.sourceforge.io/download.html) : version **1.4** (2025-06-07), GPL-2.0-or-later, publiée
  **uniquement sur SourceForge** (https://sourceforge.net/projects/emutos/files/emutos/1.4/ ; pas de release GitHub).
  `https://downloads.sourceforge.net/project/emutos/emutos/1.4/<archive>.zip` répond 302 vers un miroir
  `*.dl.sourceforge.net`. Archives : `emutos-192k-1.4.zip` (ST/STE, 22 langues, une image par langue),
  `emutos-512k-1.4.zip` (TT/Falcon), `emutos-1024k-1.4.zip` (multilingue, « optimal for Hatari », anglais par défaut,
  langue choisie par NVRAM : TT/Falcon seulement). Hatari livre EmuTOS comme `tos.img`
  (https://github.com/libretro/hatari/blob/main/doc/emutos.txt). Hashes : design §3.3.
- **Base de données** : `libretro-database/metadat/no-intro/Atari - ST.dat` ne couvre que l'IPF (404 jeux) ;
  `metadat/tosec/Atari - ST.dat` (clrmamepro, ≈ 12 700 images `.st`/`.stx`/`.ipf`/`.dim`, multi-disques nommés
  `(Disk N of M)`). Vignettes : https://github.com/libretro-thumbnails/Atari_-_ST.
- **libretro.h** (https://github.com/libretro/libretro-common/blob/master/include/libretro.h) : 12
  `SET_KEYBOARD_CALLBACK`, 13 `SET_DISK_CONTROL_INTERFACE`, 57 `GET_DISK_CONTROL_INTERFACE_VERSION`, 58
  `SET_DISK_CONTROL_EXT_INTERFACE` ; `retro_keyboard_event_t(bool down, unsigned keycode, uint32_t character,
  uint16_t key_modifiers)` ; `RETROK_LAST = 342` ; `RETRO_DEVICE_ID_MOUSE_*` 0 à 10.

## Décision

### 1. Core

- Système `Atari - ST` dans `emu-assets/data/cores.toml` : **`hatari`** recommandé, **`hatarib`** en alternative
  (licences GPL-2.0 toutes deux). `hatari` : maintenu par l'organisation libretro sur Hatari 2.6.1, déjà essayé avec
  notre frontend (`docs/essais-hatari.md`), IPF/STX intégrés, et c'est le core qui exerce toute l'infrastructure
  (BIOS libre, copies de travail, interface disque étendue) dont l'Amiga aura besoin. `hatarib` est plus « fini »
  (EmuTOS intégré, saves séparées) : bon repli si `hatari` (« tech preview ») déçoit à l'essai ; question §9.
  `hatari2014` n'est pas proposé (Hatari 1.8).
- Nouvelles propriétés par core dans `cores.toml` (tableau `[cores.<id>]`, facultatif) :
  `media = "direct" | "working_copy"` (§5) et `options = { clé = valeur }` = **défauts frontend** appliqués sous les
  réglages de l'utilisateur. `hatari` : `media = "working_copy"`, `options = { hatari_vkbd = "disabled" }` (le clavier
  virtuel du frontend le remplace ; sinon le bouton X du pad l'ouvrirait). `hatarib` : `media = "direct"`.
- Catalogue `emu-library/data/systems.toml` : `id = "atarist"`, extensions `st msa stx ipf dim m3u` (+ `zip`, géré par
  le frontend), `database = "Atari - ST"`, **`kind = "computer"`** (nouveau champ, défaut `"console"`),
  **`dat = "tosec"`** (nouveau champ, défaut `"no-intro"` ; dossier `metadat/tosec/`). Identifiants ScreenScraper 42 et
  IGDB 63 à vérifier comme les autres.

### 2. BIOS libre (EmuTOS)

- Catalogue embarqué `emu-assets/data/free_bios.toml` : archives avec **URL, taille et SHA-256 épinglés**, variantes
  avec nom d'entrée, taille, SHA-256 et md5 épinglés. Pas de TOFU : la version est figée (1.4) ; une nouvelle version
  d'EmuTOS = une mise à jour du fichier de données (vérifiée par `reviewer`).
- Téléchargement HTTPS avec redirections suivies vers d'autres hôtes HTTPS (`same_host_redirects = false`, déjà
  prévu par `HttpConfig`) : l'intégrité vient du SHA-256 épinglé, pas du miroir. Limite 16 Mio, extraction en mémoire
  (noms d'entrée comparés, jamais utilisés comme chemin, comme `cores::archive`). Installation hors ligne possible depuis
  une archive téléchargée par l'utilisateur (mêmes vérifications).
- Installé sous le nom attendu par le core : **`<system>/tos.img`**, écriture atomique, manifeste
  `<system>/.emu-rust-bios.toml` (fichier, variante, SHA-256). Un `tos.img` **non installé par emu-rust** (TOS d'origine
  fourni par l'utilisateur) n'est **jamais écrasé** sans confirmation ; s'il l'est, il est renommé
  `tos.img.user-<unix>`. La suppression ne touche qu'un fichier à nous et intact.
- Variante par défaut : **EmuTOS 192 Ko dans la langue du système** (`sys-locale`, repli `fr`, décision de l'utilisateur), car la langue de TOS
  fixe aussi la **disposition du clavier ST** (§3) : AZERTY → `192fr`. `1024k` proposé pour TT/Falcon (anglais).
  Choix modifiable dans l'écran Cores (ligne BIOS du core) ; jamais de téléchargement sans action de l'utilisateur
  (bouton, ou proposition au premier lancement d'un jeu ST si `tos.img` manque).
- Vérification : nouvel état `FirmwareState::Free { label }` quand le md5 du fichier correspond à un BIOS libre installé
  (« EmuTOS 1.4 — français ») : le dialogue « BIOS manquant / hash différent » ne s'affiche pas pour EmuTOS.
- Rien n'est empaqueté (images Docker et archives de release sans EmuTOS) : pas de redistribution, donc pas d'ajout à
  `THIRD-PARTY-DATA.txt` ; le README cite la source et la licence.

### 3. Clavier complet

- `emu-libretro` gère `SET_KEYBOARD_CALLBACK` et répond aux requêtes `RETRO_DEVICE_KEYBOARD` (id = `RETROK_*`) : les
  deux mécanismes existent selon les cores (hatari : callback ; d'autres : interrogation). `GET_INPUT_DEVICE_CAPABILITIES`
  annonce joypad, souris et clavier.
- **Correspondance positionnelle** : `winit::keyboard::KeyCode` (touche physique) → `RETROK_*` de la touche à la même
  place sur un clavier US ; le core convertit en scancode ST ; **la langue de TOS donne le caractère**. Avec EmuTOS `fr`,
  un clavier AZERTY tape donc ce qui est gravé sur ses touches (sauf les quelques touches qui diffèrent entre AZERTY PC
  et AZERTY ST). Le champ `character` reçoit le texte produit par l'OS (`KeyEvent::text`), utile à d'autres cores.
- Les répétitions automatiques de l'OS sont ignorées (la machine émulée répète elle-même). Sous Windows, le
  `ControlLeft` factice qui précède `AltRight` (AltGr) est filtré. L'IME est désactivé pendant une partie.
- Les événements sont mis en file par l'UI et livrés **entre deux `retro_run`, sur le thread du core** (ADR 0010), dans
  l'ordre ; l'état « touches enfoncées » est aussi fourni pour l'interrogation.
- Profil par système (`kind = "computer"`) : toutes les touches vont au core **sauf la touche hôte** (§4).

### 4. Raccourcis en mode ordinateur : touche hôte (compatible clavier 60 %)

- En mode console, rien ne change (Échap = menu, raccourcis Ctrl + lettre de la rc.2).
- En mode ordinateur, Échap, Ctrl et les lettres appartiennent à la machine émulée. Les raccourcis passent par une
  **touche hôte** (modèle VirtualBox) : **Ctrl droit** par défaut partout, macOS compris (décision utilisateur) ; **⌘ droit** en repli réglable
  (MacBook sans Ctrl droit) ; configurable (`[hotkeys] host_key`, valeurs conseillées `ControlRight`, `SuperRight`,
  `ContextMenu`, `AltRight` déconseillé : AltGr). La touche hôte n'est **jamais** transmise au core (l'ST n'a qu'un
  Control, tenu par Ctrl gauche).
- Hôte seul (appui puis relâchement sans autre touche) = **menu** (pause, libère la souris). Hôte + touche = action,
  mêmes lettres qu'en console : S/L sauver/charger l'état, `,`/`.` emplacement, R reset, P pause, F plein écran,
  I statistiques, Espace (maintenu) avance rapide ; nouveaux : **K** clavier virtuel, **M** capture souris, **J** bascule
  « joystick au clavier », **D** disquette suivante (et menu Disquettes depuis le menu). Ctrl droit + lettre fonctionne
  aussi en console (c'est un Ctrl), ce qui donne une seule habitude sur tous les systèmes. Aucune touche F, flèche ou
  pavé n'est requise.
- **Bascule « joystick au clavier »** (Hôte + J, mémorisée par jeu, OSD à chaque bascule) :
  - *Clavier* : toutes les touches vont au clavier ST (bureau GEM, saisie, jeux au clavier) ;
  - *Joystick* (défaut d'un jeu lancé depuis la bibliothèque) : les touches du profil `KeyMap` du système (gauche :
    ZQSD sur AZERTY = positions WASD ; tir J, tir 2 K) pilotent le joystick ST (RetroPad port 0) ; les autres touches
    vont toujours au clavier ST (Espace, F1… restent utilisables). Démarrage sans disquette (bureau) : *Clavier*.
  Avec une manette (ADR 0016), le joystick est sur la manette et le mode *Clavier* suffit : aucune dépendance.

### 5. Disquettes

- `emu-libretro` gère 13, **58** et répond **1** à 57 ; API sûre `Core::disk_control() -> Option<DiskControl>`.
  `emu-runtime` enchaîne éjecter → choisir → insérer (`Emulator::disks()`), l'UI ne fait que demander.
- **Les fichiers de l'utilisateur ne sont jamais modifiés.** Politique par core (`media`) : `working_copy` (hatari, qui
  écrit dans les images) ⇒ copie unique dans `<data>/saves/<core_id>/<jeu>.disks/` au premier lancement, réutilisée
  ensuite (la progression y est), « Réinitialiser les disquettes » dans le menu ; `direct` (hatarib, qui sauve
  ailleurs) ⇒ fichiers transmis tels quels. Les empreintes de la bibliothèque restent valides.
- **Jeux à plusieurs disquettes** : la bibliothèque regroupe `(Disk N of M)`, `(Disk N)`, `(Side A/B)` (nommage TOSEC)
  d'un même dossier, et tout `.m3u` présent (ses fichiers n'apparaissent plus séparément) ; un zip contenant plusieurs
  images = un jeu. Au lancement : `.m3u` de l'utilisateur tel quel en `direct`, sinon `.m3u` **généré** dans le dossier
  de travail (`<jeu>.m3u`, chemins relatifs) ; une seule disquette ⇒ pas de `.m3u`.
- Zip : extraits par le frontend (le core hatari ne les lit pas), avec les limites existantes de `content.rs`, dans
  le dossier de travail (`working_copy`) ou un dossier temporaire privé (`direct`).
- Changement à chaud : menu **Disquettes** (liste avec libellé du core, disquette courante, éjecter, insérer, ajouter
  une image du même dossier) et Hôte + D (suivante, OSD « Disquette 2/3 : … »). Démarrage sans disquette possible
  (`SET_SUPPORT_NO_GAME`) : entrée « Bureau Atari ST » de la bibliothèque.
- Savestates : la sérialisation du core n'inclut pas le contenu des disquettes ; recharger un état après des écritures
  disque peut être incohérent. Avertissement unique dans l'OSD, pas de mécanisme supplémentaire en 2.0.

### 6. Souris

- `RETRO_DEVICE_MOUSE` (port 0) : déplacements **relatifs** accumulés par image (lus une fois par image ; deux
  lectures dans la même image rendent la même valeur), boutons gauche/droit/milieu/4/5, molettes. Sensibilité réglable.
- Capture : clic dans l'image quand le core a déjà interrogé la souris ⇒ curseur masqué + `CursorGrabMode::Locked`
  (repli `Confined`, Windows) + mouvements bruts `DeviceEvent::MouseMotion`. Libération : touche hôte (menu), Hôte + M,
  perte de focus, ouverture d'un écran egui. OSD « Souris capturée — Ctrl droit pour libérer ».
- Sans capture possible (noVNC, certains WSLg) : mode **non capturé** de repli, déplacements calculés depuis
  `CursorMoved` au-dessus de l'image, curseur système masqué au-dessus de l'image. Jamais d'échec.

### 7. Clavier virtuel

- Clavier virtuel **du frontend** (egui, par-dessus l'image), le même pour tous les ordinateurs : dispositions en
  fichiers de données (`emu-ui/data/keyboards/atari_st.toml` en 2.0 ; Amiga, PC ensuite), légendes par langue (`us`,
  `fr`, `de` au minimum, choisies d'après la variante EmuTOS ou l'option du jeu). Il donne accès aux touches absentes
  d'un clavier 60 % (F1–F10, Help, Undo, pavé numérique, flèches).
- Hôte + K ou menu ; utilisable à la souris (non capturée), aux flèches/Entrée et, plus tard, à la manette ;
  modificateurs collants (Shift, Control, Alternate) relâchés à la fermeture. L'émulation continue.
- Les claviers virtuels internes des cores sont désactivés par défaut (§1) mais restent activables dans leurs options.

### 8. Options par jeu

- Fichier `<data>/game-options/<core_id>/<jeu>.toml` (nom de jeu d'ADR 0011 ; pour un jeu multi-disques, nom du
  groupe), créé seulement quand l'utilisateur change quelque chose : `[core_options]`, `[input]`
  (`keyboard_mode = "joystick" | "keyboard"`, `mouse_capture = "auto" | "off"`, `keyboard_layout`), `[media]`
  (`last_disk`). Hors de la base SQLite (cache dérivé, ADR 0014) et hors de `config.toml` (qui grossirait).
- Ordre d'application des options du core : défauts du core < défauts frontend (`cores.toml`) <
  `[core_options.<core_id>]` de `config.toml` < options du jeu. Dans le menu, chaque option porte « pour ce jeu /
  pour tous les jeux ». Les options dont la description contient « Requires restarting » proposent « Redémarrer ».
- Valable pour **tous** les systèmes (consoles comprises).

### 9. Manette (ADR 0016) sans dépendance

- Aucun code de la 2.0 ne suppose l'absence ou la présence d'une manette : clavier, souris, clavier virtuel et manette
  sont des `InputBackend` combinés (OU / somme des déplacements). Les raccourcis manette (PS + bouton) déclenchent les
  mêmes `HotkeyAction` ; les nouvelles actions (clavier virtuel, capture, disquette suivante) sont ajoutées à
  `HotkeyAction` pour qu'ADR 0016 puisse les lier (ex. PS + R3 = clavier virtuel). Souris émulée au stick : hors 2.0.

## Conséquences

- Nouvelles API publiques : `emu_libretro::{RetroKey, KeyModifiers, KeyEvent, mouse_id, DiskControl, DiskControlKind}`,
  `Core::{disk_control, has_keyboard_callback, send_key_event, supports_no_game, load_no_game}` ;
  `emu_runtime::{KeyboardState, MouseState, MouseButton, ContentSpec, MediaSet, MediaPolicy, PreparedMedia, Disks,
  DiskSlot, prepare_media, reset_working_copies}`, `InputFrame::{keyboard, key_events, mouse}`,
  `Emulator::{load_content, disks, prepared_media, uses_keyboard, uses_mouse}` ; `emu_assets::bios`. Graphe des
  crates inchangé.
- Nouvelle dépendance : `sys-locale` (MIT/Apache-2.0, `emu-ui`) ; rien d'autre (pas de nouveau réseau hors EmuTOS).
- Schéma de la bibliothèque : version 3 (multi-disques) ; `systems.toml` gagne `kind` et `dat`.
- Les disquettes modifiées vivent dans `saves/` : les sauvegarder = sauvegarder `<data>`, comme la SRAM.
- `hatari` est une « tech preview » : risque de régressions nightly, atténué par la version précédente conservée
  (ADR 0013) et l'alternative `hatarib`.
- 3.0 Amiga : ajouter `aros` à `free_bios.toml` (puae a aussi un AROS intégré), la disposition `amiga.toml`, la
  politique `media` de puae ; aucun changement d'architecture attendu.
