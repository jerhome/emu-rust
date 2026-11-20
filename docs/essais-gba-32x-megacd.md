# Essais mgba (GBA), picodrive (32X) et genesis_plus_gx (Mega CD) — étude de faisabilité pour la version 6.0

Date : 2026-11-20. Périmètre fixé par décision de l'utilisateur : **GBA (mgba)**, **32X (picodrive)** et **Mega CD (genesis_plus_gx)**.
Essais sous Xvfb (Mesa llvmpipe, l'appli annonce `OpenGL 4.5 Core, Mesa 22.3.6`) avec le binaire `target/debug/emu-rust`
de la branche `wip-2.0` (sans modification du code). Un petit programme C (`dlopen`, hors dépôt) a servi à lire
`retro_get_system_info` et la liste des commandes d'environnement demandées.

## 1. Cores (buildbot libretro)

| Cible | URL | mgba | picodrive |
|---|---|---|---|
| Linux x86_64 | `https://buildbot.libretro.com/nightly/linux/x86_64/latest/<core>_libretro.so.zip` | téléchargé ; zip 524 108 o, `.so` 3 314 664 o | téléchargé ; zip 864 823 o, `.so` 2 040 336 o |
| Windows x86_64 | `…/windows/x86_64/latest/<core>_libretro.dll.zip` | HEAD 200, 569 964 o | HEAD 200, 910 644 o |
| macOS x86_64 | `…/apple/osx/x86_64/latest/<core>_libretro.dylib.zip` | HEAD 200, 488 500 o | HEAD 200, 819 348 o |

Tous modifiés le 2026-11-20. SHA256 des `.so` Linux (nightly du jour, TOFU, non épinglable) :
mgba `c840f305825d07c9bec7a6ec18841073c4b539fd234f73623474f97c507f1cdc` ; picodrive `fd4010afa91f69d3c61be49c95e14a5c37117c4d0899d3e593ccc4339c1417d5`.

| | mgba | picodrive |
|---|---|---|
| `retro_get_system_info` | `name=mGBA version=0.11-212-7a12d6d ext=gba\|gb\|gbc\|sgb need_fullpath=0 block_extract=0` | `name=PicoDrive version=2.05-1890c29 ext=bin\|gen\|smd\|md\|32x\|cue\|iso\|chd\|m3u\|sms\|gg\|sg\|sc\|68k\|sgd\|pco\|vgm\|vgz need_fullpath=1 block_extract=0` |
| Licence | **MPL-2.0** (`.info` : `MPLv2.0` ; GitHub mgba-emu/mgba : MPL-2.0). Libre, redistribuable. | **« MAME » non commerciale** (`.info` : `MAME` ; `COPYING` de libretro/picodrive : « may not be sold, nor used in a commercial product or activity »). Usage personnel : compatible avec le projet ; **non libre au sens OSI** (déjà le cas de genesis_plus_gx). À citer dans `THIRD-PARTY-DATA.txt`. |
| Rendu | **logiciel**, `hw_render = false` ; aucun symbole GL/Vulkan importé ; `SET_PIXEL_FORMAT` = RGB565 (2) | **logiciel**, `hw_render = false` ; aucun symbole GL/Vulkan importé |
| AV | 240x160, 59,7275 fps, **65 536 Hz** (non standard, géré sans underrun) | 320x240 (MD), 60 fps, 44 100 Hz |
| Taille d'un savestate | 528 448 o | 679 178 o (MD ; à vérifier en 32X, plus gros) |
| `.info` | `savestate = true` (deterministic), `disk_control = false`, `needs_fullpath = false` | `savestate = true` (deterministic), `disk_control = true`, `needs_fullpath = true`, base « Sega - 32X » incluse |

### BIOS

- **mgba** : `gba_bios.bin` (md5 `a860e8c0b6d573d191e4ec7db1b1e4f6`), `gb_bios.bin` (md5 `32fbbd84168d3482956eb3c5051637f5`),
  `gbc_bios.bin` (md5 `dbfce9db9deaa2567f6a84fde55f9680`), `sgb_bios.bin` : **tous facultatifs** (`firmware*_opt = true`).
  Sans BIOS, mgba utilise un BIOS HLE intégré : démarrage correct constaté (voir §2). Options `mgba_use_bios`, `mgba_skip_bios`.
  Le BIOS GBA est commercial : à ne jamais télécharger (ADR 0005) ; déjà listé côté Game Boy dans `docs/bios-utilisateur.md`,
  à compléter avec la ligne `gba_bios.bin` (facultatif : animation de démarrage et quelques jeux sensibles au BIOS).
- **picodrive** : **aucun BIOS pour la 32X** (BIOS 32X M/S/G intégrés au core : symboles `p32x_bios_*`). Les `bios_CD_E/U/J.bin`
  (md5 `e66fa1dc…`, `2efd74e3…`, `278a9397…`) ne servent qu'au Mega-CD, facultatifs ici et hors périmètre.

### Callbacks d'environnement demandés (numéros libretro) et état du frontend

Comparés à `crates/emu-libretro/src/callbacks.rs` :

| Commande | mgba | picodrive | Frontend |
|---|---|---|---|
| 10 SET_PIXEL_FORMAT, 9 GET_SYSTEM_DIRECTORY, 15/16/17 variables, 52 options v1, 11 input descriptors, 27 log, 37 SET_GEOMETRY, 8 perf level, 13 disk control, 57 disk version, 65 content info override, 36 memory maps, 42 achievements, 51 input bitmasks | oui | oui | gérés |
| 45 GET_VFS_INTERFACE (expérimental) | oui | oui | **non géré**, sans conséquence constatée |
| 62 SET_AUDIO_BUFFER_STATUS_CALLBACK, 63 SET_MINIMUM_AUDIO_LATENCY | oui | oui | non gérés, sans conséquence (consultatifs) |
| 66 GET_GAME_INFO_EXT | non | oui | non géré, sans conséquence |
| 23 GET_RUMBLE_INTERFACE | oui | non | non géré ; seul usage : jeux GBA à vibration (Drill Dozer, Wario Ware Twisted) ; à traiter avec la manette (phase 6) |
| 14 SET_HW_RENDER | non | non | sans objet : **ni mgba ni picodrive n'ont besoin du rendu matériel** |

Aucun callback manquant ne bloque : les deux cores démarrent et tournent avec le frontend actuel.

Options (clés, pour la fiche de l'UI) — mgba : `mgba_gb_model`, `mgba_sgb_borders`, `mgba_gb_colors(_preset)`, `mgba_use_bios`,
`mgba_skip_bios`, `mgba_color_correction`, `mgba_interframe_blending`, `mgba_idle_optimization`, `mgba_solar_sensor_level`,
`mgba_force_gbp`, `mgba_frameskip*`, `mgba_audio_low_pass_*`, `mgba_allow_opposing_directions`.
picodrive : `picodrive_region`, `picodrive_renderer`, `picodrive_aspect`, `picodrive_sound_rate`, `picodrive_drc`, `picodrive_overclk68k`,
`picodrive_sprlim`, `picodrive_fmchip`, `picodrive_input1/2`, `picodrive_ramcart`, `picodrive_frameskip*`, filtres audio, options SMS/GG.

## 2. Essais headless

Commande (dossier système vide, pas de BIOS) :

```text
xvfb-run -a -s "-screen 0 1280x800x24" target/debug/emu-rust --core <core>.so --rom <rom> \
  --data-dir D --system-dir D/sys --save-dir D/saves --frames 300 --dump-frame f.png --no-audio
```

### mgba

- Le frontend reconnaît déjà mgba (profil de touches `mgba`, 20 touches liées ; `cores.toml` : Game Boy → gambatte, sameboy, mgba).
  Il manque l'association **GBA** dans `cores.toml` (voir §5).
- `hello.gba` (jsmolka/gba-tests, ppu/) : « Hello world! » affiché, 240x160, 0 underrun, exit 0. Image PNG sans anomalie.
- `arm.gba` (600 images) : écran « **Failed test 522** » ; `thumb.gba` : « **Failed test 230** ». Ce sont des suites de tests CPU ;
  l'échec survient **sans BIOS** (BIOS HLE) et n'a pas été analysé (hypothèse : test lisant le BIOS réel ; ne pas conclure à un défaut du frontend).
  À ne pas utiliser comme test de non-régression « pass » sans BIOS ; `hello.gba` suffit comme empreinte d'image.
- Sauvegardes : mgba expose SRAM/Flash/EEPROM via `retro_get_memory_data` (écriture `.srm` par le runtime, comme les autres cores) ;
  non exercé ici (aucun homebrew à sauvegarde).

### picodrive

- **32X : non essayé faute de ROM libre prête à l'emploi** (voir §3). Le seul `.32x` trouvé (`viciious/d32xr/extras/megasave/D32XR.32x`,
  32 768 o, SHA256 `f930e600b8e93bb1f69204c571e28972e0ab624d957a8b6efa581b9c9ea77146`) n'est pas une ROM démarrable :
  picodrive répond `bad reset vector` puis `Failed to detect ROM/CD image type` ; le frontend affiche « core rejected game »
  (erreur propre, pas de crash).
- Mega Drive : `240pSuite-1.21.bin` (ROM de test existante) : 320x240, 60 fps, 0 underrun, exit 0. Le core est donc utilisable par le
  frontend ; le mode 32X ne diffère que par le contenu (aucun besoin GL, aucun BIOS).
- `couldn't open carthw.cfg!` dans le journal du core : sans conséquence (fichier de cartouches spéciales facultatif).

## 3. Données de test libres

| Système | Source | Licence | Hash | Remarque |
|---|---|---|---|---|
| GBA | https://github.com/jsmolka/gba-tests, commit `a7113b67e63f83a9b321696ddd7042ccfad6c881`, `ppu/hello.gba` via `https://raw.githubusercontent.com/jsmolka/gba-tests/a7113b67e63f83a9b321696ddd7042ccfad6c881/ppu/hello.gba` | MIT (`LICENSE`, © 2019 Julian Smolka) | SHA256 `38aed48b67bc0f701e8aa222b0c3334bd306bd29888707bb7224d81f5576c264` (1 300 o) | **Recommandé** : URL épinglée au commit |
| GBA | idem, `arm/arm.gba` | MIT | `77ee88662552bdc885c1080c0172ff119d54db791bd73b21808cf1ff1fe5b40e` (8 824 o) | « Failed test 522 » sans BIOS, voir §2 |
| GBA | idem, `thumb/thumb.gba` | MIT | `b5cb2291df4ab314b31c598acd9bff2ccfa0b38efff29daadfe97422ce369b67` (3 680 o) | « Failed test 230 » sans BIOS |
| 32X | **aucune ROM prête trouvée** (GitHub : aucun binaire publié). Sources libres à compiler : https://github.com/viciious/yatssd (MIT, démo tilemap/scaling 32X), https://github.com/andwn/marsdev (MIT, toolchain ; releases Linux ≈ 100 Mo) | MIT | — | Piste : construire une ROM de test dans l'image Docker avec marsdev, sortie non commitée, SHA256 de l'archive de la toolchain épinglé ; la ROM produite n'est reproductible que si la toolchain l'est |
| 32X | https://github.com/viciious/d32xr (Doom 32X Resurrection) | « Other » : MIT pour le code original, mais code issu de Jaguar Doom sous licence non libre | — | **À écarter** (licence ambiguë, WAD à fournir) |

Piste complémentaire pour la 32X : ROM minimale écrite par QA (≈ en-tête MD « SEGA 32X » + code M68K/SH2) — non réalisée, coût élevé ;
à ne lancer que si l'architecte juge un test 32X automatisé indispensable. Sans ROM, le test d'intégration 32X reste manuel (ROM de l'utilisateur).

## 4. Formats et besoins frontend

- **GBA** : `.gba` (le core lit aussi `.gb`, `.gbc`, `.sgb`). `needs_fullpath = false` : le frontend passe le contenu en mémoire, y compris
  depuis un zip extrait par le frontend (`block_extract=0`). Un seul fichier, pas de disque.
- **32X / MD** : `.32x` (ROM 32X), `.bin/.gen/.md/.smd`, `.cue/.chd/.iso` (Mega-CD, hors périmètre), `.m3u`. `needs_fullpath = true`, `block_extract = 0` :
  comme hatari, un zip n'est pas décompressé par le core ; la bibliothèque/le frontend doit extraire ou refuser.
- **Sauvegardes** : GBA = SRAM / Flash / EEPROM (selon la cartouche, détectée par le core) ; 32X/MD = SRAM/EEPROM de cartouche.
  Le runtime sait déjà écrire les `.srm` ; vérifier la détection de taille pour l'EEPROM GBA (non testé).
- **Savestates** : 0,5 à 0,7 Mo, aucun problème de taille.
- **Bibliothèque** : bases libretro « Nintendo - Game Boy Advance » et « Sega - 32X » ; identification par CRC32/SHA1 comme pour les autres systèmes ;
  extension `.bin` ambiguë (MD / 32X / Saturn) : s'appuyer sur l'en-tête (« SEGA 32X » à 0x100) ou sur la base de données.
- **Entrées** : GBA = pad 2 boutons + L/R (profil `mgba` existant) ; 32X = pad 3/6 boutons de la Mega Drive (profil picodrive à créer ou réutiliser celui de genesis_plus_gx).
- **Cores** : deux cores déjà dans `cores.toml` pour MD, aucun changement d'architecture. Ajouter `game_boy_advance → [mgba]` (et éventuellement `vba_next`, GPLv2,
  moins précis, sans intérêt) et `sega_32x → [picodrive]`.

## 4 bis. Mega CD / Sega CD (genesis_plus_gx)

Core : genesis_plus_gx, déjà le core MD de la 1.0 (`tests/cores/genesis_plus_gx_libretro.so`, SHA256 `ac060194c55aa6b596a9cc1b8fdacad8fb13349f52a1a7f4d51b8a321bad0551`,
`v1.7.4 b9c2316`). Buildbot : même schéma d'URL que §1 (`genesis_plus_gx_libretro.so.zip`).
Licence : **« Non-commercial »** (`.info` ; GitHub libretro/Genesis-Plus-GX : « Other ») — usage personnel seulement, à citer dans `THIRD-PARTY-DATA.txt`.
`.info` : `needs_fullpath = true`, `disk_control = true`, `supported_extensions = mdx|md|smd|gen|bin|cue|iso|sms|bms|gg|sg|68k|sgd|chd|m3u`,
base « Sega - Mega-CD - Sega CD ». Rendu logiciel (pas de `hw_render`).

- **BIOS** : `bios_CD_E.bin` (Mega CD PAL), `bios_CD_U.bin` (Sega CD NTSC-U), `bios_CD_J.bin` (Mega CD NTSC-J), dans le dossier système.
  Commerciaux : **jamais téléchargés** (ADR 0005), fournis par l'utilisateur. Le `.info` de genesis_plus_gx les marque `firmware*_opt = true` et ne donne **pas** leur md5 ;
  le `.info` de picodrive (mêmes noms) donne : `bios_CD_U.bin` md5 `2efd74e3232ff260e371b99f84024f7f`, `bios_CD_E.bin` md5 `e66fa1dc5820d254611fdcdba0662372`,
  `bios_CD_J.bin` md5 `278a9397d192149e84e820ac621a8edd` (à revérifier avec l'architecte avant de les écrire dans `firmware.toml`). En pratique **requis** pour tout disque
  (le core choisit le fichier selon la région lue dans l'en-tête du disque ; U par défaut) ; à ajouter à `docs/bios-utilisateur.md` (section Mega Drive : « requis pour le Mega CD »).
  Optionnel : `bios_MD.bin` (TMSS).
- **Comportement sans BIOS (constaté)** : avec un `.cue` factice (disque de zéros, aucune donnée commerciale) et un dossier système vide :
  journal `ERROR … Unable to open CD BIOS: "<system>/bios_CD_U.bin"` puis le frontend affiche
  `erreur : core rejected game …` suivi du conseil générique « Vérifiez que le core est compilé pour ce système… » — **message trompeur** : le frontend
  devrait relayer l'erreur du core (ou vérifier les BIOS requis avant lancement, cf. `--check-bios`) et afficher « BIOS Mega CD manquant : bios_CD_U.bin ».
  Pas de crash, exit propre.
- **Formats** : `.cue` + `.bin` (pistes audio multiples : plusieurs `.bin` ou `.wav/.mp3` référencés par le `.cue` — le frontend passe le chemin complet, le core lit le dossier),
  `.chd`, `.iso` (données seules, sans audio), `.m3u` pour les jeux multi-disques (interface disque de la 2.0 déjà prévue : `disk_control = true`).
  Les `.cue` référencent des fichiers relatifs : la bibliothèque doit conserver le dossier ; un zip n'est pas lu par le core.
- **Sauvegardes** : RAM interne (~8 Ko) et cartouche RAM, fichiers `.brm` / `.srm` dans le dossier de sauvegarde du core ; option du core « Per-BIOS / Per-Game »
  (partager ou non la RAM interne entre jeux d'une région). Choisir **Per-Game** par défaut (la RAM interne est minuscule). Savestate plus gros qu'en cartouche (RAM PRG 512 Ko + WORD 256 Ko ; non mesuré faute de BIOS).
- **Homebrew libre** : aucun disque Mega CD libre prêt à l'emploi trouvé. `jvisser/md-mode1-mcd-asic-test` (https://github.com/jvisser/md-mode1-mcd-asic-test, CC0-1.0,
  commit `c2f3a38bb16678adfeaab789b6b2dda3a393e8b4`, `md-mode1-mcd-asic-test.bin` 182 572 o, SHA256 `d0d4c85ce3f4351f3c49e93f4a0048f07e25d70aa9069f18fd2dacdc2f761d8d`)
  est une ROM « mode 1 » (cartouche + matériel Mega CD) : le core réclame aussi le BIOS (`Unable to open CD BIOS`) ; essai sans BIOS sans résultat exploitable. Autres dépôts
  (matteusbeus/ModPlayer MIT, megacd-vgm-player) : sources sans binaire. **Aucun test réel possible sans BIOS** : test d'intégration manuel (BIOS et disque de l'utilisateur) ;
  côté automatisé, seulement le test négatif « BIOS manquant → erreur lisible » avec le `.cue` factice généré à la volée (aucune donnée commerciale).
- Cohabitation : picodrive sait aussi lire le Mega CD (mêmes BIOS) ; genesis_plus_gx reste le choix par défaut (plus précis pour le CD, imposé par l'utilisateur).

## 5. Recommandations à l'architecte

1. **GBA (mgba)** : faisable immédiatement, rendu logiciel, licence MPL-2.0, BIOS facultatif. Travail : entrée `cores.toml` (système GBA), profil de touches
   (existe), extensions de la bibliothèque, test d'intégration `#[ignore]` sur `hello.gba` (URL + SHA256 ci-dessus) avec empreinte d'image ; mgba est déjà
   téléchargé par `scripts/fetch-test-assets.sh` (`tests/cores/mgba_libretro.so`). Option de l'UI : BIOS GBA facultatif (`gba_bios.bin`, hash md5 ci-dessus, import par l'utilisateur).
2. **32X (picodrive)** : faisable sans nouvelle infrastructure (logiciel, pas de BIOS, 20 Mo de RAM max), mais **sans ROM de test libre prête** : décider entre
   (a) test manuel avec ROM de l'utilisateur, (b) construction d'une ROM marsdev/yatssd dans la CI. Mentionner la licence non commerciale MAME (comme genesis_plus_gx).
   Distinguer MD et 32X pour le choix du core (picodrive requis pour la 32X ; genesis_plus_gx ne la gère pas).
3. Callbacks à ajouter au frontend (non bloquants) : rumble (23) avec la manette, éventuellement 62/63 (latence audio). Aucun rendu matériel nécessaire pour la 6.0.
4. **Mega CD** : aucun nouveau core ; travail = BIOS requis (import utilisateur + vérification md5, message d'erreur lisible), formats `.cue/.chd/.m3u`, option de sauvegarde Per-Game, bibliothèque (identification par base « Sega - Mega-CD - Sega CD », `.cue` + pistes).
5. **PS1 / Saturn / 3DO : non prévus** (décision de l'utilisateur). Constat gardé pour mémoire : la demande de rendu matériel (`SET_HW_RENDER`) est refusée par le frontend
   actuel ; pcsx_rearmed (logiciel, BIOS HLE) a démarré un `.exe` PS1 sans BIOS, swanstation et beetle-psx exigent un BIOS commercial.

## Fichiers créés / nettoyage

- `docs/essais-gba-32x-megacd.md` (ce rapport) ; aucun code modifié. Scratchpad (`q7/`) nettoyé de tous les cores ; les cores PS1 essayés avant la
  réduction de périmètre (pcsx_rearmed, swanstation, mednafen_psx, mednafen_psx_hw) ont été supprimés.
