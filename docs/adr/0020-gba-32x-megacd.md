# ADR 0020 — Version 6.0 : Game Boy Advance, 32X et Mega CD (rendu logiciel)

Statut : **accepté** (2026-11-21, après re-validation contre le code réel : les amendements en fin de document
(2026-11-21 ×2, 2026-11-23 multi-CD) **priment** sur le texte antérieur ; `docs/design/v6.0-gba-32x-megacd.md` §10) — décisions utilisateur du 2026-11-21 intégrées (et décision du 2026-11-20 sur la suite : **7.0 =
PlayStation + 3DO en rendu logiciel** ; Saturn, Dreamcast et rendu matériel plus tard ; revue croisée : corrections
I2, I9, I13, M3, M4, M5, M6, M8). Remplace l'intitulé de la phase 7 de la feuille de route (« 32 bits : PS1, Saturn,
3DO, GBA, 32X ; rendu matériel GL ») : **la 6.0 = GBA + 32X + Mega CD**. La mention initiale « PS1, Saturn et 3DO ne
sont pas prévus » est **caduque** (`docs/questions-ouvertes.md`, décisions « 7.0 » et « Rendu matériel (futur) »).
Précise ADR 0005 (BIOS commerciaux ; ROM intégrées aux cores : règle « Builtin »), 0011 (sauvegardes), 0013 (choix
des cores, BIOS), 0014 (DAT, jaquettes) ; réutilise ADR 0015 (médias multiples, interface disque), ADR 0017
(« Importer un BIOS… ») et les **règles communes du design 3.0** (`docs/design/v3.0-amiga.md` §3.2 `MediaRules` /
`media_direct`, §3.5 : ordre d'évaluation des médias, contrôle de firmware unique par variante, `game_saves`,
notices), ainsi que les règles CD de la bibliothèque posées en 4.0 (ADR 0018 §8). Étude préalable :
`docs/essais-gba-32x-megacd.md`. Conception : `docs/design/v6.0-gba-32x-megacd.md`. Graphe des crates inchangé.

## Contexte

Faits vérifiés le 2026-11-20 (essais headless sous Xvfb / llvmpipe avec le binaire `wip-2.0`, `.info` de
https://github.com/libretro/libretro-core-info, `.index-extended` du buildbot pour `linux/x86_64`, `windows/x86_64`,
`apple/osx/x86_64`, `dat/System.dat` et `metadat/` de https://github.com/libretro/libretro-database) :

| | mgba | picodrive | genesis_plus_gx (Mega CD) |
|---|---|---|---|
| Buildbot (3 plates-formes) | oui | oui | oui (déjà installé en 1.0) |
| Licence | **MPL-2.0** | « MAME », **non commerciale** | **non commerciale** |
| Rendu | logiciel (RGB565) | logiciel | logiciel |
| `need_fullpath` | non | **oui** (`block_extract = 0`) | oui |
| AV | 240×160, 59,7275 i/s, **65 536 Hz** | 320×224/240, 60 i/s, 44,1 kHz | idem MD |
| BIOS | `gba_bios.bin` **facultatif** (BIOS HLE intégré) | **aucun** pour la 32X (BIOS 32X intégrés) | `bios_CD_E.bin`, `bios_CD_U.bin`, `bios_CD_J.bin` **requis en pratique** (`.info` : `opt = true`, inexact) |
| Savestate | ≈ 0,5 Mo | ≈ 0,7 Mo | quelques centaines de Ko à ~1 Mo (non mesuré, sans BIOS) |
| Interface disque | non | oui | oui (`.m3u`) |

- md5 des BIOS (System.dat, déjà dans `crates/emu-assets/data/firmware.toml`) : `gba_bios.bin`
  `a860e8c0b6d573d191e4ec7db1b1e4f6` ; `bios_CD_E.bin` `e66fa1dc5820d254611fdcdba0662372` ; `bios_CD_U.bin`
  `2efd74e3232ff260e371b99f84024f7f` ; `bios_CD_J.bin` `278a9397d192149e84e820ac621a8edd` (131 072 o chacun).
- Sans BIOS Mega CD, genesis_plus_gx journalise `Unable to open CD BIOS: "<system>/bios_CD_U.bin"` et refuse le
  contenu ; l'appli affiche aujourd'hui « core rejected game » avec un conseil trompeur.
- genesis_plus_gx écrit lui-même la RAM interne du Mega CD (`scd_E.brm`… ou `<jeu>.brm`, option
  `genesis_plus_gx_system_bram` = `per bios` (défaut) / `per game`) et la cartouche RAM (`genesis_plus_gx_cart_bram`,
  `genesis_plus_gx_cart_size`) dans `GET_SAVE_DIRECTORY` ; `RETRO_MEMORY_SAVE_RAM` ne couvre que la SRAM de cartouche.
  Avec un `.m3u`, le nom de jeu du core est celui du `.m3u`.
- DAT : `metadat/no-intro/Nintendo - Game Boy Advance.dat`, `metadat/no-intro/Sega - 32X.dat`,
  `metadat/redump/Sega - Mega-CD - Sega CD.dat` (Redump via libretro-database : **seule la piste 01** est décrite,
  `size`, `crc`, `md5`, `sha1`, `serial`). Jaquettes : `libretro-thumbnails/Nintendo_-_Game_Boy_Advance`,
  `Sega_-_32X`, `Sega_-_Mega-CD_-_Sega_CD` existent.
- Données de test libres : GBA `jsmolka/gba-tests` (MIT) `ppu/hello.gba` démarre sans BIOS (les suites CPU
  `arm.gba` / `thumb.gba` échouent sans BIOS réel : inutilisables comme « réussite ») ; GBA `240pee_mb.gba`
  (240p-test-mini v0.23, GPL-2.0, même release que `gb240p.gb` déjà utilisé). **Aucune ROM 32X libre démarrable
  publiée** (sources MIT seulement : `viciious/yatssd`, toolchain `andwn/marsdev` ≈ 100 Mo). **Aucun disque Mega CD
  libre utilisable** : tout disque exige le BIOS commercial.

## Décision

### 1. Cores (`cores.toml`, ADR 0013 §7)

| Système (clé libretro) | Recommandé | Alternatives | Licences |
|---|---|---|---|
| Nintendo - Game Boy Advance | **mgba** | gpsp, vbam | MPL-2.0 ; GPL-2.0 ; GPL-2.0 |
| Sega - 32X | **picodrive** (seul core 32X) | — | MAME (non commerciale) |
| Sega - Mega-CD - Sega CD | **genesis_plus_gx** | picodrive | non commerciale ; MAME (non commerciale) |

- mgba pour la GBA : le plus précis des trois, maintenu, BIOS HLE, licence libre ; gpsp (dynarec, très léger) proposé
  si le Mac Intel peine, vbam pour mémoire. Pour Game Boy / Color, gambatte reste le choix (1.0) : mgba n'y est
  qu'alternative.
- picodrive est déjà l'alternative MD / SMS / GG de la 1.0 : la 32X n'ajoute **aucun core**, seulement une entrée
  système. genesis_plus_gx ne gère pas la 32X.
- genesis_plus_gx reste le core MD de la 1.0 et devient celui du Mega CD (plus précis pour le CD, choix de
  l'utilisateur). Un seul core installé couvre donc MD, SMS, GG et Mega CD.
- Usage personnel uniquement : licences non commerciales compatibles (ADR 0001, roadmap), citées dans le README de
  release et l'écran Cores (`THIRD-PARTY-DATA.txt` ne décrit que les données embarquées : design 3.0 §3.5.7). Les
  cores ne sont jamais redistribués par le projet.

### 2. BIOS

- **Jamais téléchargés** (ADR 0005). Import par « Importer un BIOS… » (3.0), md5 vérifié, nom attendu par le core.
- **BIOS requis du Mega CD = données du contrôle de firmware unique posé en 3.0** (design 3.0 §3.5.5) : la 6.0
  n'ajoute **aucune API ni aucun état** côté `emu-assets`, seulement une table `required_firmware` par (core,
  système) avec `variant = "region"` — `europe` → `bios_CD_E.bin`, `usa` → `bios_CD_U.bin`, `japan` →
  `bios_CD_J.bin`, `default = "usa"` (comme le core), sans repli — pour genesis_plus_gx et picodrive (le `.info` de
  genesis_plus_gx les dit facultatifs, à tort pour le CD). La région vient du jeu (`(Europe)`, `(USA)`, `(Japan)`
  d'après le DAT ou le nom).
- **Contrôle avant lancement** (le même que pour le Kickstart) : BIOS requis absent ou de mauvais hash ⇒ dialogue
  « BIOS Mega CD manquant : bios_CD_U.bin (Sega CD, USA) » avec « Importer un BIOS… », « Lancer quand même » (règle
  d'ADR 0013 §8 : les données peuvent être inexactes) et « Annuler ». Le refus d'un contenu par le core affiche en
  plus la dernière erreur journalisée par le core pendant `retro_load_game`, à la place du conseil générique.
- Rien de ceci n'est propre à Sega : la table par région, `DiscRegion` et `disc_serial` (§5) sont **génériques** et
  resserviront tels quels en 7.0 (PlayStation : `scph5500.bin` / `scph5501.bin` / `scph5502.bin` par région ; 3DO).
- `gba_bios.bin` : facultatif ; présent ⇒ `mgba_use_bios = ON` (défaut du core) ; aucun avertissement s'il manque.
- 32X : rien à fournir.
- **Statut des BIOS intégrés aux cores** (règle « Builtin » d'ADR 0005 amendée / ADR 0019 §2 : rien n'est extrait ni
  redistribué par emu-rust) :
  - **mgba** : BIOS HLE = réimplémentation fournie par le projet mGBA dans son source (MPL-2.0), pas une copie du
    BIOS Nintendo — **à vérifier** (fichier source et licence à citer par GV1 ; l'étude d'essais constate seulement
    « BIOS HLE intégré », `docs/essais-gba-32x-megacd.md`) ;
  - **picodrive** : BIOS 32X (M / S / G) intégrés au binaire (symboles `p32x_bios_*`, étude d'essais) ; **origine et
    licence non sourcées : à vérifier** par GV1 (réimplémentations libres du projet PicoDrive, ou code Sega ?). Dans
    les deux hypothèses la règle « Builtin » couvre l'usage (comme les ROM de VICE, acceptées par l'utilisateur le
    2026-11-21) ; si ce sont des réimplémentations libres, la notice le dira. État affiché : `Builtin` avec une
    notice prudente tant que la vérification n'est pas faite (« BIOS 32X intégrés au core picodrive »).

### 3. Formats et médias

- **GBA** : `.gba` (zip accepté, contenu en mémoire). **32X** : `.32x` ; `.bin` seulement si l'en-tête porte
  `SEGA 32X` à 0x100 ; zip extrait par le runtime comme pour tout core `need_fullpath` (mécanisme 1.0).
- **Mega CD** : `.cue` (+ pistes `.bin` / `.wav` / `.ogg` référencées), `.chd`, `.iso`, `.m3u`. Les images CD sont
  **transmises en place, jamais copiées ni extraites** (`media_direct` = `cue`, `chd`, `iso` ; ordre d'évaluation du design 3.0 §3.5.1) ;
  **zip de CD refusé** avec un message (« décompressez, ou convertissez en CHD ») : 300-700 Mo dépassent
  `MAX_CONTENT_SIZE` et la copie n'apporterait rien (le core n'écrit pas dans les images).
- Multi-CD : regroupement `(Disc N)` / `(CD N)` de la bibliothèque (4.0) ⇒ `MediaSet` + `.m3u` généré dans un
  **dossier temporaire privé** (chemins absolus vers les `.cue` / `.chd`) : c'est le comportement du code en
  politique `direct` (`dest_dir` / `write_playlist`, `crates/emu-runtime/src/media.rs`), **pas**
  `saves/<core_id>/` ; rien n'est écrit à côté des sauvegardes. Changement par le menu **Disquettes / CD**
  (interface disque 2.0). Un `.m3u` de l'utilisateur est utilisé tel quel.
- Bibliothèque : le masquage des fichiers référencés par un `.cue`, celui des entrées d'un `.m3u` et l'absence de
  hash des images CD sont **déjà en place depuis la 4.0** (ADR 0018 §8, indépendants du système). La 6.0 ajoute
  seulement : une piste `.bin` orpheline commençant par `SEGADISCSYSTEM` est ignorée avec un avertissement, et les
  règles `disc = true` du système `megacd`.

### 4. Sauvegardes

- GBA, 32X : SRAM / Flash / EEPROM par `RETRO_MEMORY_SAVE_RAM` ⇒ `<jeu>.srm` (ADR 0011, compatible RetroArch).
- Mega CD : fichiers `.brm` écrits **par le core** dans `saves/genesis_plus_gx/`. Défaut emu-rust :
  `genesis_plus_gx_system_bram = per game` (8 Ko de RAM interne : partagée, elle se remplit vite), cartouche RAM
  désactivée (défaut du core) ; les deux réglables par jeu (options par jeu, 2.0). Avec un `.m3u` (multi-CD), la
  sauvegarde est commune aux disques du jeu. Un passage `per bios` ⇄ `per game` ne déplace aucun fichier (documenté).
- Savestates : conteneur d'ADR 0011 inchangé ; tailles < 2 Mo.

### 5. Identification

- GBA, 32X : CRC32 / SHA1 sur DAT No-Intro (inchangé, ADR 0014).
- Mega CD : nouvelle source `DatSource::Redump` (`metadat/redump/`). `.cue` / `.iso` : taille de la piste 01
  comparée aux tailles du DAT, puis CRC32 **seulement si une taille correspond** (pas de hash de 500 Mo pour rien) ;
  sinon numéro de série lu dans l'en-tête du disque (`disc_serial` : fonction **générique** à sondes par format —
  Sega `SEGADISCSYSTEM` en 6.0, PlayStation `SYSTEM.CNF` en 7.0) ; `.chd` : nom de fichier puis série (lecture du CHD
  reportée). Schéma de la bibliothèque : **v6** (`game.region`).
- Le DAT Redump est distribué par libretro-database mais **sous un chemin que l'application ne télécharge pas
  encore** : `emu-assets/src/dat.rs` ne connaît que `metadat/no-intro/` et `metadat/tosec/` (et `dat/` en 3.0). La
  6.0 ajoute l'URL `…/libretro-database/master/metadat/redump/` (tâche GA1, revue réseau GV1). Téléchargé à
  l'exécution, jamais embarqué : cité dans le README de release, pas dans `THIRD-PARTY-DATA.txt`.

### 6. Entrées

- GBA : nouveau `PadStyle::Gba` (disposition SNES sans X/Y : A = K, B = J, L = O, R = L, flèches / ZQSD, Start,
  Select), profil `mgba` ; un `.gb` / `.gbc` lancé avec mgba garde le style Game Boy.
- 32X et Mega CD : pad Mega Drive (profils `picodrive` et `genesis_plus_gx`, déjà là) ; 6 boutons par défaut.
- Manette (ADR 0016, livrée en 2.1) : `PadStyle::Gba` a aussi un mapping manette (colonne GBA ajoutée à ADR 0016 ;
  `crates/emu-ui/src/gamepad/mapping.rs`, tâche GU1) : Croix = B, Rond = A, L1 / R1 = L / R, Share = Select,
  Options = Start ; rien d'analogique. La vibration (GET_RUMBLE_INTERFACE, quelques jeux GBA) reste hors périmètre
  (amendement d'ADR 0016 si demandé).

### 7. Hors périmètre de la 6.0, et suite décidée

- Hors 6.0 : rendu matériel (`SET_HW_RENDER`, toujours refusé : aucun core de la 6.0 n'en a besoin) ; vibration ;
  Sega CD 32X (« CD 32X » : picodrive seulement, non testé) ; MSU-MD.
- **Version 7.0 (décision de l'utilisateur du 2026-11-20) : PlayStation et 3DO en rendu logiciel.** Une ADR dédiée
  sera écrite au lancement de la 7.0 (choix des cores : pcsx_rearmed en logiciel a démarré dans l'étude d'essais ;
  core 3DO à étudier). Elle réutilise sans changement ce que pose la 6.0 : CD (`media_direct`, `.m3u`, masquage des
  pistes), Redump, `disc_serial`, `DiscRegion`, BIOS requis par région (`scph5500` / `5501` / `5502`,
  `panafz10.bin`), import de BIOS. C'est pourquoi ces éléments sont définis ici sans rien de propre à Sega.
- **Plus tard, sans version fixée : Saturn, Dreamcast et rendu matériel.** Consigne de l'utilisateur pour le rendu
  matériel : cibler **OpenGL, DirectX (contexte libretro D3D11 sous Windows) et Vulkan**. L'utilisateur ne pourra
  pas essayer Vulkan (sa machine le supporte mal) : les **tests automatiques sous lavapipe valident les étapes
  Vulkan** ; essai réel facultatif par un camarade (à lui rappeler). L'affirmation initiale « Vulkan écarté » est
  retirée.
- Pistes techniques notées pour l'ADR du rendu matériel (non décidées) : contexte GL unique partagé avec egui (FBO
  du frontend, profil core 3.3 / 4.1 sur macOS ; les cores qui exigent un profil de compatibilité ou GLES, comme
  yabasanshiro, demanderont un autre chemin) ; macOS Intel et WSLg n'ont pas de pilote Vulkan natif, d'où
  l'importance de lavapipe pour les tests.

## Conséquences

- Aucune nouvelle dépendance, aucun `unsafe` nouveau attendu. **Un téléchargement nouveau** : le DAT Redump
  (`metadat/redump/`, même hôte `raw.githubusercontent.com` que les autres DAT, nouveau chemin dans
  `emu-assets/src/dat.rs`) — code réseau à faire valider par `reviewer`. Cores et jaquettes : canaux existants.
- Nouvelles API (détail : design §3) : `emu_assets` — URL du DAT Redump ; **données seulement** pour les BIOS requis
  par région (contrôle et types posés en 3.0) ; `emu_library` — systèmes `gba`, `32x`, `megacd`,
  `DatSource::Redump`, `DiscRegion`, `disc_serial`, `probe_bin`, `disc = true`, schéma v6 (`game.region`) ;
  `emu_libretro` — dernière erreur du core jointe à `LoadGameFailed` ; `emu-ui` — `PadStyle::Gba` (clavier et
  manette), région du jeu passée comme variante au contrôle de firmware.
- **Tests automatiques limités** : GBA réel (hello.gba, sans BIOS) ; 32X et Mega CD seulement par le pipeline
  (bibliothèque, choix du core, BIOS manquant ⇒ message lisible). Le jeu réel 32X / Mega CD est vérifié par
  l'utilisateur (ses ROM et BIOS). Construire une ROM 32X libre (yatssd + marsdev) est une option ouverte (design §9).
- Les fichiers `.brm` du Mega CD ne passent pas par le runtime : « réinitialiser les sauvegardes » d'un jeu doit
  aussi viser `<jeu>.brm` (motifs par core, mécanisme `game_saves` / `reset_core_game_files` de la 3.0).

## Amendement du 2026-11-21 — Mega CD avancé en 2.1 (« express »)

Décision de l'utilisateur : essayer Dune (Mega CD) dès le 21/11. Un sous-ensemble du Mega CD est livré en **2.1**
avec les mécanismes de la 2.0 seulement (`docs/design/v2.1-megacd-express.md`) : système `megacd` (`id` et
`database` de cette ADR, désormais **persistés**), `.cue` / `.chd` / `.iso` en lecture directe, pistes masquées
(`cue_references`), aucun hash d'image, sonde d'en-tête et région, contrôle du BIOS dans l'UI, RAM interne par jeu.
Conséquences pour la 6.0 :

- son périmètre Mega CD devient **« compléter »** : Redump, `game.region` (v6), contrôle unique par région
  (`required_firmware`), `core_error`, multi-CD, `game_saves`, import de BIOS dans le dialogue ;
- **correction** : la région d'un disque Mega CD se lit d'abord dans **l'en-tête** (octet 0x20B du secteur 0 :
  `0x64` Europe, `0xA1` Japon, sinon USA — règle constatée du core genesis_plus_gx `v1.7.4 b9c2316`) ; le DAT
  Redump et l'étiquette du nom ne servent que si l'en-tête est illisible (`.chd`). Le §2.2 du design 6.0 est à
  mettre à jour à sa re-validation ;
- `DiscRegion`, `DiscFormat`, `DISC_EXTENSIONS` et `cue_references` existent dès la 2.1 dans
  `emu-library/src/disc.rs` : GX0 et DX0 les reprennent sans les redéfinir.

## Amendement du 2026-11-21 (2) — re-validation contre le code (3.0 posée, Mega CD express livré en `v2.1.0-rc.1`)

Complète l'amendement précédent ; détail : design 6.0 §10.

- **§2, contrôle du BIOS** : fait par `check_core_firmware` (3.0) avec la variante `Game::region` ; il **remplace** le
  contrôle propre au Mega CD de la 2.1 (`emu-ui/src/library/megacd.rs`), sans perte de comportement : quand la région
  est inconnue (`.chd` sans étiquette), le lancement n'est pas bloqué dès qu'un des trois BIOS est présent (règle de
  la 2.1, tenue côté UI grâce à `others_present`) ; un md5 différent de la référence avertit sans bloquer.
- **§3, bibliothèque** : la restriction de `cue` / `chd` / `iso` au dossier et la sonde d'en-tête hors dossier sont
  posées par la 4.0 (ADR 0018, amendement) ; la 6.0 ajoute `.m3u`, Redump et `disc = true`. `id = "megacd"` et
  `database = "Sega - Mega-CD - Sega CD"` sont persistés.
- **§4, sauvegardes** : motif `game_saves = ["{game}.brm"]`, appliqué au dossier de sauvegarde de l'identité de jeu
  (2.1) ; la cartouche RAM partagée (`cart.brm`) n'est jamais effacée par « Réinitialiser ».
- **§5, schéma** : v6 = colonne `game.region` seulement ; la région des jeux déjà en base est complétée au scan.
- **Contexte, données de test** : les phrases « aucune ROM 32X libre démarrable » et « aucun disque Mega CD libre »
  sont dépassées — `hexgl32x` (MIT) et 240p Test Suite Sega CD (GPL-2.0) existent
  (`docs/design/donnees-test-4-7.md`) ; il reste qu'aucun jeu Mega CD ne démarre sans BIOS commercial.
- **§7, suite** : la 7.0 n'est pas abordée sans le feu vert de l'utilisateur (ADR 0021 abandonnée) ; le §7 vaut
  pour mémoire.

## Amendement du 2026-11-23 — multi-CD Mega CD sans `.m3u` généré (correctif 7.0.1)

**Contexte.** Essai utilisateur (Night Trap, 2 `.chd` regroupés), reproduit en headless par qa-systems avec les vrais
cores : le `.m3u` généré dans le dossier temporaire privé (§3, chemins **absolus**) est illisible par
`genesis_plus_gx` (« could not load … from m3u file list ») et `picodrive` (« invalid CD image ») : ces deux cores
préfixent **toujours** chaque ligne par le dossier du `.m3u`, sans tester l'absolu (Linux et Windows). Les cores PS1
(`pcsx_rearmed`, `swanstation`, `mednafen_psx`) acceptent l'absolu ; `opera` ne lit pas de `.m3u` (déjà
`group_discs = false`, ADR 0021 §6). Écrire le `.m3u` à côté des ROMs est exclu (on n'écrit jamais chez
l'utilisateur) ; des chemins relatifs au dossier temporaire sont impossibles sous Windows quand les ROMs sont sur un
autre lecteur que `%TEMP%`.

**Décision.** Remplace, pour ces deux cores seulement, la phrase « `MediaSet` + `.m3u` généré … » du §3 :

- Nouvelle clé `cores.toml` `multi_disc = "playlist" | "disk_control"` (défaut `"playlist"` = comportement actuel).
  `"disk_control"` sur `genesis_plus_gx` et `picodrive` ; refusée au chargement du fichier avec
  `media = "working_copy"` (comme `m3u_launch = "program"`, à l'inverse).
- Pour un `MediaSet` de N ≥ 2 disques avec `disk_control` : **aucun `.m3u`**, le disque 1 est donné en place à
  `retro_load_game` ; juste après un chargement réussi (avant `restore_sram`), les disques 2..N sont ajoutés par
  l'interface disque (`add_image_index` + `replace_image_index` sur le nouvel index), **sans manipuler le tiroir** :
  le disque inséré (index 0) n'est jamais touché, et une éjection / réinsertion avant la première image ferait
  relire le disque 1 au core. Échec (pas d'interface disque, refus du core) ⇒ échec du lancement
  (`RuntimeError::NoDiskControl` / `Libretro`), pas de session amputée de disques.
- Rien d'autre ne change : politique `Direct`, `media_direct`, menu **Disquettes / CD**, `.m3u` de l'utilisateur
  (`ContentSpec::File`, ses entrées relatives fonctionnent), cores PS1 (`playlist`). `set_initial_image` n'est pas
  utilisé (emu-rust ne mémorise pas le disque inséré : chaque lancement commence au disque 1, comme avant) ; la liste
  des disques étant identique à celle d'un `.m3u`, les savestates ne sont pas affectés.
- §4 précisé : le nom du jeu vu par le core est celui du **disque 1** (`content_name`), donc `<disque 1>.brm`, commun
  à tous les disques (le core ne change pas de fichier en cours de session) ; `game_saves = ["{game}.brm"]` suit.
  Aucune migration : ce lancement n'avait jamais fonctionné.

**Conséquences.** Correctif minimal, sans `unsafe` ni réseau nouveaux (l'interface disque d'`emu-libretro` existe).
Non vérifiable avec un vrai jeu ici (aucun BIOS Mega CD libre) : test sur core factice + essai utilisateur (Night
Trap Mega CD, genesis_plus_gx). Le CD 32X (7.2, picodrive, ADR 0024) hérite de la clé. Détail et découpage :
design 6.0 §12.

## Amendement du 2026-11-24 — CD 32X (7.4) : à prouver avant de coder

Le texte « picodrive lance le CD 32X » est une **hypothèse non vérifiée** : à ma connaissance picodrive émule le
Mega CD **ou** la 32X, pas la combinaison Sega CD + 32X. La 7.4 commence donc par un essai-sonde (design 7.4 §3) ;
le code de reconnaissance n'est écrit que si picodrive démarre le disque. Sinon : la 7.4 se réduit à documenter
que les éditions 32X se lancent en mode Mega CD seul (genesis_plus_gx) si le disque le permet. Détail : `docs/design/v7.4-sega-cd-32x.md`.
