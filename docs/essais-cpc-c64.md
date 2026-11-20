# Essais Amstrad CPC et Commodore 64 — étude de faisabilité pour la 5.0

Date : 2026-11-20. Essais faits sous Xvfb avec l'appli `emu-rust` (`target/debug/emu-rust`, binaire local de la branche
`wip-2.0`, modes `--core ... --rom ...`) et avec un petit frontend libretro jetable en C (`dlopen`, hors dépôt : journalise
les appels `environment`, les options, injecte clavier / manette, sauvegarde les images, teste l'interface disque et
`retro_serialize`). Aucun code de crate modifié. Sources lues pour confirmer les comportements :
https://github.com/libretro/libretro-cap32 (HEAD `06ad0da`, 2026-10-30) et https://github.com/libretro/vice-libretro
(HEAD `76cde0b`, 2026-11-08 ; les binaires du buildbot du 2026-11-20 portent ces numéros).

**Verdict : faisable, sans blocage.** Les deux cores démarrent sans aucun fichier système, sans contenu, et lancent
disquettes, cassettes et programmes. Les besoins nouveaux pour le frontend sont listés en §7.

## 1. Cores (buildbot libretro)

Tous existent pour Linux, Windows et macOS x86_64 (HEAD 200). URL type :
`https://buildbot.libretro.com/nightly/{linux/x86_64,windows/x86_64,apple/osx/x86_64}/latest/<core>_libretro.{so,dll,dylib}.zip`.

| Core | Linux x86_64 (zip) | `.so` | Windows (zip) | macOS x86_64 (zip) |
|---|---|---|---|---|
| **cap32** | 384 662 o | 1 598 728 o | 441 853 o | 394 892 o |
| crocods | 273 766 o | 615 696 o | 298 516 o | 270 089 o |
| **vice_x64** | 1 358 217 o | 3 625 536 o | 1 711 120 o | 1 312 632 o |
| vice_x64sc | 1 338 920 o | 3 597 376 o | 1 698 166 o | 1 302 378 o |
| vice_x128 (pour mémoire) | 1 507 447 o | 3 960 768 o | 1 858 286 o | 1 426 743 o |
| frodo | 189 213 o | 416 656 o | 293 911 o | 201 805 o |

SHA256 (nightly du 2026-11-20, Linux) :

| Core | zip | `.so` |
|---|---|---|
| cap32 | `c2541f8e36fa18028b1daa22b1f7f6483e4a729a06c9dac532f4271e39809224` | `f223b3fb1d30a4df888d90ab710c8886cc0ba0467d109e00d3d2eda3aaf0bfeb` |
| crocods | `ec692d359245acd12f9ac92fa3614c7310cec8dbddcf4fe8010f03f1a16cbf62` | `49633246c788c7e13ddc1c840bbd4a30f65ad04e8c3889aca1404100012dfa19` |
| vice_x64 | `9ba7ba714e21660d7dfa186f7e4d05e524b36080d410431479664b4593f2cba0` | `18c10bfd3351109c760b2fed000b7b2c29fe658a7feb37296de853c0d74fe531` |
| vice_x64sc | `52f5810acf86da3baa21f3303ad31f694ab91b591b985426df54f3b73dc7b569` | `38e040fe354a5e19ce737b57eb71be9acbb6f8adce36204d26a99eaeee76ee27` |
| vice_x128 | `2a00b460f4aacb3ee491d45d8fbef51e935711d70f68c66f3c6090882995cce4` | `79285865b6ee24559de46f86441bea028fcc5231b58674193e512f1e07d52144` |
| frodo | `21e53ad23983bfb99de69b10a45b29c476fad8b6c2c98502d23674dbf78bf95f` | `67c3f7e82ae7115b3b08dae445c9eeea9d50f7580b22a6e44d3aab4f406bb036` |

`retro_get_system_info` (lu par programme C), API libretro 1 pour tous :

| Core | version | extensions | need_fullpath | block_extract |
|---|---|---|---|---|
| cap32 | `4.5.4 06ad0da HI` | `dsk\|sna\|zip\|tap\|cdt\|voc\|cpr\|m3u` | 1 | 0 |
| crocods | `git a9c63b2` | `sna\|dsk\|kcr` | 0 | 0 |
| vice_x64 / x64sc | `3.10 76cde0b` | `d64\|d71\|d80\|d81\|d82\|g64\|g41\|x64\|t64\|tap\|prg\|p00\|crt\|bin\|zip\|7z\|gz\|d6z\|d7z\|d8z\|g6z\|g4z\|x6z\|cmd\|m3u\|vfl\|vsf\|nib\|nbz\|d2m\|d4m\|/\|tcrt` | 1 | **1** |
| frodo | `V4_2` | `d64\|t64\|x64\|p00\|lnx\|lyx\|zip` | 1 | 0 |

- VICE déclare **`block_extract = 1`** : le frontend ne doit **pas** extraire les archives (`zip`, `7z`, `gz`…), le core
  s'en charge (vérifié, §5). cap32, crocods, frodo : `block_extract = 0` ; cap32 annonce `zip` mais ne le gère pas
  (§5). (Lecture faite avec la vraie structure `retro_system_info`, deux `bool` d'un octet.)
- Extension `/` de VICE : un dossier est accepté comme contenu (non testé).
- **Licences** : cap32 GPL-2.0 (`.info` `GPLv2`, `cap32/COPYING.txt`), VICE GPL-2.0 (`COPYING` du dépôt, l'API GitHub
  donne `GPL-2.0`), frodo GPL-2.0, crocods **MIT**. Auteurs : Ulrich Doewich, Colin Pitrat, David Colmenero et al. (cap32) ;
  équipe VICE + Libretro (Alexander Sherwood et al.).
- Écarts `.info` / binaire : `.info` de cap32 annonce `display_version = "v4.2.0"` alors que le binaire est `4.5.4` ;
  `.info` de VICE annonce `zip` mais pas `7z`/`tcrt` que le binaire liste. Même constat que pour dosbox_pure :
  TOFU ou SHA256 du nightly épinglé.
- `.info` cap32 : `supports_no_game = true`, `disk_control = true`, `savestate = true` (`serialized`),
  `needs_fullpath = true`, `needs_kbd_mouse_focus = true`, base de données `Amstrad - CPC|Amstrad - GX4000`.
  `.info` VICE : `supports_no_game = true`, `disk_control = true`, `savestate = true`, `database = "Commodore - 64"`,
  `firmware0..3` = JiffyDOS (tous `opt`).
- **Recommandation : cap32 pour le CPC** (seul core CPC complet : démarrage sans contenu, autorun disque/cassette,
  `.m3u`, CPC+/GX4000, états). **crocods** : essayé, démarre mais affiche **son propre sélecteur de fichier**
  (« Select file to run », aucun autorun), pas de démarrage sans contenu, pas d'interface disque, pas de touche d'état
  → écarté. **vice_x64 pour le C64** (état de l'art, autostart disque/cassette/cartouche/PRG, `.m3u`, états).
  `vice_x64sc` (cycle-exact, même interface, mêmes options) est l'alternative « précision » : 7,1 s contre 1,4 s pour
  2000 images en headless (≈280 contre ≈1500 images/s, large marge dans les deux cas). **frodo** : essayé, ne lance
  pas le `.d64` (reste sur READY, pas d'autostart), pas d'états → écarté. Proposition : `vice_x64` par défaut,
  `vice_x64sc` proposé dans les options du système.
- Performance (headless, 2000 images ≈ 40 s émulées) : cap32 1,25 s, vice_x64 1,36 s, vice_x64sc 7,1 s.

### Options importantes

**cap32** (20 options, v2 ; clés `cap32_*`) :

| Clé | Défaut | Valeurs / rôle |
|---|---|---|
| `model` | `6128` | `464, 464DDI, 664, 6128, 6128+` ; vérifié : 464 = « Amstrad 64K Microcomputer (v1) / BASIC 1.0 », 664 = « (v2) / BASIC 1.1 », 6128 = « 128K (v3) », 6128+ = « 128K (+3) ». `6128+` obligatoire pour `.cpr` (cartouches CPC+/GX4000) |
| `lang_layout` | `english` | `english, french, spanish` (clavier émulé AZERTY / QWERTY, voir §3) |
| `autorun` | `enabled` | `enabled, disabled` : tape `RUN"fichier` (disque) ou `\|TAPE` + `RUN"` (cassette) |
| `tape_fastload` | `maximum` | `disabled, 4, 8, maximum` |
| `floppy_sound` | `enabled` | bruit du lecteur |
| `retrojoy0`, `retrojoy1` | `auto` | `auto, qaop, incentive, joystick_port1` (+ `joystick_port2` pour le 2) : manette → touches ou joystick |
| `combokey` | `select` | touche combo (changement de disque Combo + L/R) |
| `ram` | `128` | `64, 128, 192, 576` (le nom de fichier `[576K]` force aussi) |
| `scr_tube`, `scr_crop`, `scr_intensity`, `gfx_colors` | `color`, `disabled`, `8`, `16bit` | moniteur couleur / vert / blanc ; recadrage ; 8/16/24 bits |
| `statusbar`, `keyboard_transparency`, `lightgun_*`, `db_mapkeys` | — | barre d'état, clavier virtuel, pistolet, base de remap internes |

**vice_x64** (113 options, v2 ; clés `vice_*`) — les plus utiles :

| Clé | Défaut | Valeurs / rôle |
|---|---|---|
| `c64_model` | `C64 PAL auto` | `C64 PAL/NTSC auto`, `C64C …`, `C64 PAL`, `C64 NTSC`, `C64C PAL/NTSC`, `C64SX`, `PET64`, `C64 GS PAL`, `C64 JAP NTSC`. **NTSC change la géométrie et la cadence** : 384x247, 59,8263 i/s (PAL : 384x272, 50,1245 i/s, mesuré) |
| `joyport` | `2` | `1, 2` : port de la manette 1 (les vieux jeux utilisent le port 1) ; `joyport_type` : joystick, paddles, souris… |
| `autostart` | `enabled` | `disabled, enabled, warp` |
| `autoloadwarp` | `disabled` | warp pendant le chargement (`disk`, `tape`, avec ou sans son) |
| `drive_true_emulation` | `enabled` | émulation vraie du lecteur 1541 (chargement lent mais compatible) ; `virtual_device_traps` |
| `warp_boost`, `reset` | `disabled`, `autostart` | `reset` : `autostart, soft, hard, freeze` |
| `jiffydos` | `disabled` | `disabled, enabled` (demande les fichiers JiffyDOS, voir §2) |
| `ram_expansion_unit`, `printer`, `floppy_multidrive`, `floppy_write_protection` | — | matériel optionnel |
| `sid_engine`, `sid_model`, `resid_*`, `sound_sample_rate` | `ReSID`, `default`, —, `48000` | audio (22050, 44100, 48000, 96000) |
| `physical_keyboard_pass_through` | `disabled` | transmettre les touches physiques sans passer par la table du core |
| `keyboard_keymap` | `positional` | `positional, symbolic, positional-user, symbolic-user` (voir §3) |
| `datasette_hotkeys` | `disabled` | active les raccourcis Datasette (flèches par défaut !) |
| `mapper_*` | — | raccourcis clavier / manette (voir §3 pour les conflits) |
| `vice_crop*`, `aspect_ratio`, `vicii_*`, `statusbar*`, `vkbd_*` | — | image et interface du core |

Les options sont déclarées en v2 (`GET_CORE_OPTIONS_VERSION` = 2 → `SET_CORE_OPTIONS_V2`) ; les deux cores
retombent aussi correctement sur v1 / v0 (`SET_VARIABLES`) : mêmes 20 / 113 options.

### Appels `environment` utilisés (flags expérimentaux = 0x10000)

- **cap32** : 18 (no game), 45\|exp (VFS), 35 (controller info : Amstrad Joystick=513, Keyboard=259, Lightgun=260),
  52, 67 (options v2), 27 (log), 42\|exp (achievements), 9, 30, 31 (system / assets / save dir), 11 (input descriptors),
  **12 (keyboard callback)**, 57 + **58 (disk control ext)**, 15, 64 (fastforwarding override), 10 (pixel format),
  17 (variable update).
- **VICE** : mêmes + 28 (perf), 36\|exp (memory maps), 46\|exp (LED), 51\|exp (input bitmasks), 55 (options display),
  69 (options update callback), 12, 57, 58, 64.
- Pixel format : **RGB565 (2)** pour les deux (VICE propose aussi 24 bits via `vice_gfx_colors`, cap32 via `gfx_colors`).
- Retours « non géré » (false) pour 45\|exp, 46\|exp, 51\|exp, 36\|exp, 28, 69… : aucune différence de rendu
  (images identiques bit à bit avec ou sans) → **pas de VFS nécessaire**.
- Messages : aucun `SET_MESSAGE` observé ; les journaux passent par `GET_LOG_INTERFACE`. cap32 journalise ses infos
  de démarrage au niveau **ERROR** (« Got size: 768 x 272 ») : ne pas en déduire une erreur.
- Aucun `SET_HW_RENDER` : rendu logiciel seulement.
- `retro_set_controller_port_device(port, RETRO_DEVICE_JOYPAD)` **doit** être appelé (l'appli le fait déjà,
  `emu-libretro/src/core.rs:396`) : sans lui, VICE ignore la manette.

## 2. ROMs système

- **cap32 : ROMs embarquées dans le core** (`cap32/rom/464.h`, `664.h`, `6128.h`, `6128p.h`, `amsdos.h`, `cpm.h` : OS +
  BASIC + AMSDOS + CP/M compilés dans le `.so`). Vérifié : démarre dans le dossier système vide, les 5 modèles s'affichent.
  Le core ne cherche aucun fichier (`cpc464.rom`, `cpc664.rom`, `cpc6128.rom` n'apparaissent que comme noms de
  configuration internes ; aucun fichier lu). Licence : le code est GPL-2.0 ; les ROMs sont © Amstrad / Locomotive
  Software, **redistribuées avec la permission d'Amstrad** (déclaration historique ; non re-vérifiée ici : cpcwiki
  répond 403 aux accès automatiques). Cohérent avec `docs/bios-utilisateur.md`.
- **VICE : KERNAL / BASIC / CHARGEN / ROM 1541, 1571, 1581 embarqués dans le core.** Vérifié : démarre avec un
  dossier système vide (écran « **** COMMODORE 64 BASIC V2 **** / 64K RAM SYSTEM 38911 BASIC BYTES FREE »), disque
  1541 fonctionnel (`LOAD"*",8,1`). Messages sans gravité : « DriveROM: 2000 / 4000 / CMDHD ROM image not found ».
  Les fichiers `vice/data/C64/*.bin` du dépôt libretro (`kernal-901227-03`, `basic-901226-01`, `chargen-901225-01`…)
  portent la mention du `README` de VICE : « The ROM files … are Copyright C by Commodore Business Machines » ;
  VICE les distribue depuis des années (autorisation historique de l'ayant droit, non re-vérifiée ici, ne figure pas
  dans les sources lues). Aucun fichier à télécharger pour l'appli : l'utilisateur obtient les ROMs avec le core.
- **Fichiers optionnels attendus par VICE** dans `<système>/vice/` (noms exacts, `firmware*_opt = true`) :
  `JiffyDOS_C64.bin` (8192 o, MD5 `be09394f0576cf81fa8bacf634daf9a2`), `JiffyDOS_1541-II.bin` (16384 o,
  `1b1e985ea5325a1f46eb7fd9681707bf`), `JiffyDOS_1571_repl310654.bin` (32768 o, `41c6cc528e9515ffd0ed9b180f8467c0`),
  `JiffyDOS_1581.bin` (32768 o, `20b6885c6dc2d42c38754a365b043d71`) ; aussi `JiffyDOS_SX-64.bin`
  (`f0d3aa7c5a81d1e1d3e8eeda84e9dbbe`) et `JiffyDOS_C128.bin` (`cbbd1bbcb5e4fd8046b6030ab71fc021`). **JiffyDOS est
  commercial** : jamais téléchargé (ADR 0005) ; documenter seulement, et laisser l'import par l'utilisateur
  (`vice_jiffydos = enabled`). SuperCPU : `system/vice/SCPU64` (hors périmètre).
- **Le core écrit dans le dossier système** : `system/vice/` est créé et `vicerc-dump-C64` (≈ 29 Ko, dump des ressources)
  y est (ré)écrit à chaque démarrage → le dossier système doit être **accessible en écriture** (volume Docker en
  lecture seule = journal d'erreur, à tester). VICE cherche aussi `vicerc` dans le dossier de sauvegarde
  (`<saves>/<core>/vicerc`, `<nom du jeu>.vicerc` à côté du contenu et dans `<saves>`) : mécanisme de configuration par jeu
  gratuit, à garder à l'esprit (`vice_read_vicerc`).
- Dézippage : VICE extrait les archives dans `<saves>/TEMP/` (fichiers temporaires dans le dossier de sauvegarde).

## 3. Essais headless (appli et frontend jetable)

Pour reproduire : `xvfb-run -a target/debug/emu-rust --core <core>.so --rom <fichier> --system-dir D --save-dir D
--no-audio --frames 700 --dump-frame out.png` (aucun besoin du réseau).

| Essai | Résultat |
|---|---|
| cap32, sans contenu | **« Amstrad 128K Microcomputer (v3) / ©1985 Amstrad Consumer Electronics plc and Locomotive Software Ltd. / BASIC 1.1 / Ready »**, 768x272, ar 1,4118, 50,08 i/s, 44100 Hz, 131 328 octets d'état |
| VICE, sans contenu | **« **** COMMODORE 64 BASIC V2 **** / 64K RAM SYSTEM 38911 BASIC BYTES FREE / READY. »** 384x272 (max 384x288), ar 1,3221, 50,1245 i/s, 48000 Hz, état 230 415 octets (230 927 avec un disque : **taille variable**, la redemander à chaque sauvegarde) |
| cap32 + `hello.dsk` (appli et jetable) | `[computer_autoload] DSK autorun: RUN"HELLO.BAS` ; affiche « EMU-RUST CPC TEST », bordure rouge |
| VICE + `hello.prg` | `AUTOSTART: Injecting program data at $0801` … `Done`, bordure rouge, fond vert, « EMU-RUST C64 TEST » |
| VICE + `hello.d64` | `LOAD"*",8,1` automatique + `RUN`, même résultat (lecteur 1541 en vraie émulation, ≈ 6 s émulées) |
| VICE + `hello.tap` | « PRESS PLAY ON TAPE / OK / SEARCHING / FOUND HELLO / LOADING / READY. / RUN » : **la cassette est lancée toute seule** (PLAY pressé par l'autostart) |
| cap32 + `hello.cdt` | `\|TAPE` + `RUN"` tapés par le core, « Press PLAY then any key » automatique, chargement rapide (`tape_fastload=maximum`), le binaire Z80 s'exécute et affiche « EMU-RUST CPC TAPE TEST » |
| Appli : pas d'avertissement, pas d'`environment` non géré gênant | `emu_runtime` : 0 sous-run, `audio stats: underruns=0 dropped_samples=0` sur 700 images |

**Clavier — le point critique.**
- **Les deux cores lisent le clavier par le callback `SET_KEYBOARD_CALLBACK` (cmd 12)** : mesure faite en désactivant
  tour à tour le callback et l'interrogation `retro_input_state(RETRO_DEVICE_KEYBOARD)` : sans callback, aucune touche ;
  sans interrogation, tout fonctionne. L'hôte doit donc émettre des événements `retro_keyboard_event(down, RETROK_*,
  caractère, modificateurs)`.
- **Touches positionnelles** (code `RETROK_*`, pas le caractère) : cap32 (`retro_events.c`, `init_keyboard_table`) et
  VICE (`positional` par défaut, table `vice/data/C64/gtk3_pos.vkm`). Le caractère n'est pas utilisé : l'hôte doit envoyer
  les touches physiques (ou, pour un clavier virtuel / une disposition AZERTY, les touches qui produisent le bon signe
  sur le clavier émulé). Ex. mesuré : `Shift + 2` (`"`) fonctionne en `positional` sur les deux.
- **CPC** (source) : `Delete`=CLR, `Backspace`=DEL, `Left/Right Alt`=**COPY**, `Tab`=TAB, `Caps`=CAPS LOCK, `Entrée`=RETURN,
  `Entrée du pavé`=ENTER, `Ctrl gauche`=CONTROL, `Maj`=SHIFT, `Échap`=ESC. **Pas de F1..F10 → touches du pavé numérique**
  `KP0..KP9`=F0..F9, `KP.`=. (point du pavé CPC). `[` = `@`, `]`… : voir la table `retro_events.c` (lignes 613-760) pour
  les signes `^ £ : ; \ [ ]`. Raccourcis du **core** qui prennent le pas sur le CPC : `F9` clavier virtuel, `F10` menu
  du core (GUI), `Home` / `End` / `PageUp` = tape PLAY / STOP / REWIND, `Insert` = bascule curseur ↔ joystick.
  En mode joystick sur curseurs : `Ctrl droit` = FIRE1, `Maj droit` = FIRE2.
- **C64** (essais) : `Échap` = **RUN/STOP** (« BREAK IN 10 » vérifié), `Échap` + `PageUp` = **RUN/STOP+RESTORE** (couleur de
  fond rétablie, vérifié), `Home` = CLR/HOME, `Delete` / `Backspace` = INST/DEL, `Insert` = Maj + INST/DEL,
  `Ctrl gauche` = **C=** (Commodore), `Tab` = CTRL, `` ` `` = flèche gauche ←, `\` = flèche haut ↑, `End` = **£** (mais
  `End` est aussi le raccourci « Reset » du core : conflit), `Page Down` = `=` (voir `gtk3_pos.vkm`, non entièrement
  vérifié). Le `;` physique produit `:` et `'` produit `;` (positionnel). Touches à prévoir dans le clavier virtuel :
  RUN/STOP, RESTORE, C=, CTRL, ←, ↑, £, CLR/HOME, INST/DEL, F1-F8 (F1/F3/F5/F7 non décalées, F2/F4/F6/F8 avec Maj).
- **Raccourcis du core VICE** : `F12` barre d'état, `Ctrl droit` = **changer de port joystick** (conflit avec la touche
  hôte `Ctrl droit` du projet, ADR 0015 : si le frontend l'avale comme touche hôte, le core ne la reçoit pas : bon
  réglage par défaut, et `vice_mapper_joyport_switch` permet de la réaffecter), `End` = Reset, `Flèches` = Datasette
  (seulement si `vice_datasette_hotkeys = enabled`). Voir aussi `vice_mapper_*` pour toutes les touches / boutons.
- **Claviers virtuels du core** : cap32 `F9` (clavier + souris du core, images `ui_keyboard_*.c`), VICE `Select` (court)
  ; navigables à la manette. Rendus **dans l'image du core** : utilisables sans travail côté frontend (utile avec
  la manette en phase 6), mais le clavier virtuel du frontend (`computer/vkbd.rs` + `data/keyboards/*.toml`) reste
  plus souple.

**Manette / joystick** (essais) :
- cap32 : `JOY(0)` = 0 au repos, 1 avec **Haut** (RetroPad id 4), 2 Bas, 4 Gauche, 8 Droite, 16 Feu : opérationnel via
  `retro_input_state(0, RETRO_DEVICE_JOYPAD, …)`. `retrojoy0 = auto` pilote le joystick CPC.
- VICE : `PEEK(56320)` (port 2) change pour chaque bouton (B = feu, Haut / Bas / Gauche / Droite distincts), ainsi
  que `joyport = 1` (port 1, `PEEK(56321)`) : opérationnel. Les boutons RetroPad B/A/X/L2/R2/Select sont déjà liés par le
  core (B = feu 1, A = feu 2, X = espace, L2 = ESC = RUN/STOP, R2 = ENTRÉE, Select = clavier virtuel).
- Le profil par défaut de l'appli (`profil cap32`, `profil vice_x64` : WASD / flèches / Entrée → RetroPad) existe déjà
  mais pour une saisie **joystick** ; le mode **Clavier** (touches physiques → `retro_keyboard_event`) est celui qu'il
  faut pour BASIC et pour `RUN"` / `LOAD` (voir §7).

**Autoload** : entièrement fait par les cores (`RUN"HELLO.BAS`, `|TAPE`+`RUN"`, `LOAD"*",8,1`+`RUN`, injection `.prg`).
Rien à taper côté frontend. À désactiver (`cap32_autorun = disabled`, `vice_autostart = disabled`) pour démarrer
en BASIC avec un support inséré (mode « sans autorun » du système).

## 4. Données de test libres

Aucun programme homebrew tiers n'a été retenu : les licences (souvent absentes ou non précisées) ne sont pas
vérifiables hors ligne ; à la place, **supports générés à la main par des scripts originaux, domaine public**
(nouveaux fichiers dans `tests/tools/`, même démarche que `mkadf.py` / `mkst.py`, aucune donnée tierce) :

| Script | Sortie | Contenu | SHA256 (v. 1) |
|---|---|---|---|
| `tests/tools/mkc64.py OUTDIR` | `hello.prg` (47 o) | BASIC V2 : `10 PRINT"EMU-RUST C64 TEST":POKE53280,2:POKE53281,5` | `cf6a533abf9a406b4c37bfd2b8654847b1902b9ea019d4e80c6a64a18eaf9e08` |
|  | `hello.d64` (174 848 o) | disquette 1541, BAM + répertoire + 1 `PRG` « HELLO » | `28c4f7a47105d71feaff9ee083609ea1b3b2c7a3c5f7786158cef2c916d24237` |
| `tests/tools/mktap.py hello.prg out.tap` | `hello.tap` (44 514 o) | cassette C64 format KERNAL (entête 192 o + données, 2 copies) | `dd024ffccf902b4b6b35149a4718622cd9c72d002e941abddc8db8fa9506e34a` |
| `tests/tools/mkcpc.py out.dsk` | `hello.dsk` (194 816 o) | DSK standard 40 pistes, répertoire AMSDOS, `HELLO.BAS` en ASCII | `212ddb91b40091737b55536d286eb431f178cc642bf38ee273515d6bc9409359` |
| `tests/tools/mkcdt.py out.cdt` | `hello.cdt` (574 o) | cassette CPC (TZX 1.13, blocs `0x11`), 1 binaire Z80 qui affiche un texte | `9461a692e6a517f45122b4b07235c7b43fb60b000b0ef4448cfebae9e57f7eca` |

Tous validés sur les vrais cores (§3) ; les scripts sont déterministes (mêmes SHA256 à chaque exécution).
**Empreintes d'images déterministes** (mêmes valeurs sur 2 exécutions) pour de futurs tests `#[ignore]`.
Exemples (hash FNV-1a 64 bits de l'image RGB en 32 bits du jetable, à recalculer avec le hash du projet) : cap32
sans contenu stable dès l'image 100, VICE sans contenu stable dès l'image 200. Séquence de frappe vérifiée
(« print 12 » + Entrée → « 12 » sur les deux cores, tapé à l'image 150).

Non couverts faute de données libres ni de générateur simple : `.sna` (snapshots CPC), `.cpr` (cartouche CPC+,
modèle `6128+`), `.crt` / `.t64` (C64), `.g64`, `.d71`/`.d81`. Pistes : DSK / CDT / D64 / PRG suffisent à valider
le chemin ; pour un test de cartouche, générer un `.crt` « Normal cartridge » 8 Ko (en-tête CHIP) est faisable.
Origine possible de homebrews libres si on en veut vraiment (à vérifier un par un avant tout épinglage URL + SHA256) :
démos du concours « CPC Retrodev » / « Amstrad CPC Plus Demo » (CPC), `csdb.dk` et `Lemon64` (C64) ; **aucune
n'est téléchargée ni épinglée ici**.

## 5. Formats : natifs ou à traiter par le frontend

| Format | cap32 | VICE | Remarque |
|---|---|---|---|
| `.dsk` / `.d64` `.g64` `.d71` `.d81` `.x64` `.nib` | natif | natif | disque ; tous gérés par `need_fullpath` (le core ouvre le fichier) |
| `.prg` `.p00` `.t64` | — | natif (autostart) | |
| `.tap` (C64) / `.cdt` `.tap` `.voc` (CPC) | natif | natif | cassette ; autostart + PLAY automatique (vérifié) |
| `.crt` `.tcrt` `.bin` | `.cpr` natif (modèle 6128+) | natif | cartouches |
| `.sna` | natif | — | snapshot mémoire |
| `.vsf` | — | natif | snapshot VICE |
| `.m3u` | natif (vérifié : 2 disques) | natif (vérifié : 2 disques, `images=2`) | multi-disques ; lignes de chemin relatif ; VICE accepte `#SAVEDISK:Nom` (crée un disque de sauvegarde `<m3u>.save<N>.d64` dans les sauvegardes) et `DISQUE\|Libellé` |
| `.zip` | **le core ne charge pas** (essai : `retro_load_game` retourne vrai mais 0 image, aucun autorun) | **le core extrait** dans `<saves>/TEMP/` (vérifié : `.prg`, `.d64`) ; `.7z`, `.gz`, `.d6z`… aussi | À traiter côté frontend pour cap32 (extraction avant chargement, comme pour hatari) ; VICE (`block_extract = 1`) : lui laisser le zip |

- **Multi-faces / multi-disques** : seule voie = `.m3u` (ou plusieurs fichiers créés en jeu `GameContent` multi-fichiers
  → un `.m3u` généré, comme prévu par l'ADR 0015). Le contrôle se fait par l'**interface de disque** (cmd 58) :
  `get_num_images`, `set_image_index`, `set_eject_state`. Vérifié : VICE `images=2`, `set_image_index(1)` accepté ;
  cap32 déclare les deux entrées (`dc added`). cap32 propose aussi **Combo + L/R** pour changer de disque.
- **Cassettes** : pas de contrôle par l'interface disque dans ces cores. Le lecteur de cassette se pilote par des
  **raccourcis clavier du core** : cap32 `Home`=PLAY, `End`=STOP, `PageUp`=REWIND ; VICE `vice_mapper_datasette_*`
  (défaut : `↑` PLAY, `↓` STOP, `←` REWIND, `→` F.FWD, **seulement si** `vice_datasette_hotkeys = enabled`), plus
  `vice_datasette_sound` (bruit du lecteur) et `vice_autoloadwarp`. Avec l'autostart, il n'y a rien à faire pour le
  jeu courant ; les raccourcis servent aux cassettes multi-chargements. Le frontend peut les proposer comme
  « touches de fonction » sur son clavier virtuel (envoi de la touche libretro correspondante).
- **Support de sauvegarde** : VICE écrit sur la disquette si elle n'est pas protégée (`vice_floppy_write_protection`) ;
  `vice_work_disk` (disque de travail global) ; pour cap32, les modifications du disque sont écrites dans l'image
  (essai non fait : tester la persistance sur copie avant d'exposer l'option).

## 6. États et déterminisme

Mesuré sur 400-700 images puis `retro_serialize` / `retro_unserialize` : cap32 (131 328 o) et VICE (230 415 o, 230 927 o
avec disque), **rejeu identique au bit près** (mêmes empreintes après 60 et 120 images). VICE : **taille d'état variable**
selon le support inséré (ne pas la mettre en cache). Rejeu des images identique d'une exécution à l'autre
(base de tests d'empreintes).

## 7. Recommandations pour l'architecte (réutilisation des infrastructures 2.0-4.0)

1. **Clavier complet** (`computer/host.rs`, `keyboard.rs`) : réutilisable tel quel (callback cmd 12 déjà géré par
   `emu-libretro`) ; les deux cores sont **positionnels**. Ajouter des dispositions `cpc.toml` et `c64.toml`
   dans `crates/emu-ui/data/keyboards/` (même format que `atari_st.toml`) :
   - CPC : COPY = Alt, CLR = Delete, ENTER = pavé numérique, **F0..F9 = KP0..KP9** (à afficher sur le clavier virtuel
     mais aussi à lier à F1..F10 de l'hôte pour le confort), ESC, TAB, CAPS LOCK, SHIFT, CTRL ; AZERTY via
     `cap32_lang_layout = french` (change l'affichage de l'image du clavier du core, pas la table de touches :
     à confirmer avec l'utilisateur francophone).
   - C64 : RUN/STOP = Échap, RESTORE = Page précédente, C= = Ctrl gauche, CTRL = Tab, ← = `` ` ``, ↑ = `\`, £ = Fin
     (**mais Fin = Reset du core** : réaffecter `vice_mapper_reset` à « --- » ou à une autre touche pour libérer £),
     CLR/HOME = Début, INST/DEL = Retour arrière, F1-F8.
2. **Touche hôte Ctrl droit** (ADR 0015) : à conserver. Aucun conflit côté cap32 (RCTRL ne sert qu'en mode
   curseur→joystick). VICE utilise `RCTRL` par défaut pour « changer de port joystick » : le frontend l'avale, donc le
   core ne la voit pas ; proposer le changement de port par un **menu du frontend** qui écrit `vice_joyport`. Ne pas
   transmettre non plus `F9`/`F10` (cap32) ni `F12` (VICE) sans choix explicite : ils ouvrent des interfaces du core.
3. **Mode Clavier / Joystick** : les deux ont un mode clavier ET un mode joystick. À modéliser comme pour le ST :
   - mode *Clavier* : touches physiques → `retro_keyboard_event` ; manette = souris / joystick selon option ;
   - mode *Joystick* : touches (WASD / flèches + feu) → RetroPad ; pour cap32 laisser `retrojoy0 = auto`
     (joystick CPC) ; pour VICE régler `vice_joyport` (1 ou 2 selon le jeu ; beaucoup de jeux
     1984-86 = port 1, après = port 2).
   Option par jeu `joyport` (1/2) indispensable pour le C64.
4. **Médias multiples** : réutiliser `GameContent` + génération de `.m3u` (déjà prévus par `content.rs` / ADR 0015) pour
   plusieurs disques (jeux 2-4 faces en CPC, 2-8 disques en C64). Ajouter le *libellé* `DISQUE|Nom` dans le `.m3u`
   généré pour VICE. Menu de changement de disque (interface cmd 58) ; **cassette** : entrées de menu qui émettent
   les raccourcis (`Home/End/PageUp` pour cap32 ; `vice_datasette_hotkeys=enabled` + flèches pour VICE).
5. **Options par jeu** (`game_options.rs`) à exposer en priorité : CPC `cap32_model` (464 pour les jeux de 1984-86 en cassette,
   `6128+` pour `.cpr`), `cap32_autorun`, `cap32_tape_fastload`, `cap32_ram` ; C64 `vice_c64_model` (PAL / NTSC,
   **change fréquence et géométrie** → le frontend doit relire `get_system_av_info` / réagir à `SET_SYSTEM_AV_INFO`
   à chaud : cap32 → 768x272 fixe, VICE → 384x272 PAL, 384x247 NTSC), `vice_joyport`, `vice_autostart`,
   `vice_drive_true_emulation` (désactiver = chargement quasi instantané mais moins compatible), `vice_autoloadwarp`
   (chargements rapides), `vice_jiffydos`, `vice_sid_engine`/`vice_sid_model`.
6. **Import de BIOS** (`docs/bios-utilisateur.md`) : **aucun fichier requis** pour le CPC ni le C64 ; seul JiffyDOS est
   facultatif (4 fichiers listés avec MD5 ci-dessus) à placer dans `<système>/vice/` ; vérification MD5 + message clair
   « facultatif, commercial ». `--check-bios` : rien à signaler pour ces deux systèmes (ne pas les marquer
   « manquants »). Le dossier système de VICE doit être accessible en écriture (`vice/vicerc-dump-C64`).
7. **Zip** : étendre l'extraction du frontend (déjà pour hatari) à cap32 (le core ne décompresse pas). VICE a
   `block_extract = 1` : lui passer le `.zip` tel quel (ou extraire côté frontend pour éviter `<saves>/TEMP`).
8. **Systèmes / bibliothèque** : identification via le catalogue libretro `Amstrad - CPC`, `Amstrad - GX4000`,
   `Commodore - 64`. Extensions à reconnaître : CPC `dsk sna cdt tap voc cpr m3u zip` ; C64
   `d64 t64 prg p00 tap crt x64 g64 d71 d81 d82 nib m3u vsf zip`. Attention à `.tap` : **ambigu** (cassette CPC
   ou cassette C64) → décider par le dossier / le système choisi (les deux cores déclarent `tap`) ; `.bin` (VICE) et
   `.d64`/`.t64` sans ambiguïté. Un même `.zip` peut contenir des supports des deux systèmes (à ouvrir pour déduire).
9. **Core alternatif** : prévoir `vice_x64sc` dans le gestionnaire de cores (même interface, mêmes options, précis au cycle
   mais ~5x plus lent) et laisser cap32 comme seul core CPC. `vice_x128` (C128) hors périmètre : mêmes fichiers, même
   clavier (à garder en tête pour une 5.x).
10. **Tests** : DSK / CDT / D64 / PRG / TAP générés (§4) suffisent pour des tests `#[ignore]` d'empreintes d'image
    (démarrage sans contenu, autorun de chaque format, frappe clavier, manette via `JOY(0)` / `PEEK(56320)`),
    à écrire par `qa-systems` une fois le système défini.

## 8. Points ouverts

- Permissions de redistribution des ROMs (Amstrad, Commodore/Cloanto) : non re-vérifiées en ligne (pas d'accès à
  cpcwiki). Sans conséquence pour l'appli (rien n'est téléchargé en dehors du core), à citer dans `THIRD-PARTY-DATA.txt`
  comme « fournies avec le core ».
- Non essayés : `.sna`, `.cpr`, `.crt`, `.t64`, cassettes multi-chargements, écriture sur disque, `m3u` avec
  `#SAVEDISK`, modèle NTSC avec le frontend complet (géométrie / cadence à chaud), VICE `vice_x64sc` avec disque
  en vraie émulation sous charge, Windows / macOS (HEAD seulement).
