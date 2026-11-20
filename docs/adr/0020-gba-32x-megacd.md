# ADR 0020 — Version 6.0 : Game Boy Advance, 32X et Mega CD (rendu logiciel)

Statut : proposé (2026-11-20). Remplace l'intitulé de la phase 7 de la feuille de route (« 32 bits : PS1, Saturn, 3DO,
GBA, 32X ; rendu matériel GL ») par décision de l'utilisateur du 2026-11-20 : **la 6.0 = GBA + 32X + Mega CD**,
**PS1, Saturn et 3DO ne sont pas prévus**. Précise ADR 0005 (BIOS commerciaux), 0011 (sauvegardes), 0013 (choix des
cores, BIOS), 0014 (DAT, jaquettes) ; réutilise ADR 0015 (médias multiples, interface disque) et 0017 (« Importer un
BIOS… », `MediaRules`). Étude préalable : `docs/essais-gba-32x-megacd.md`. Conception : `docs/design/v6.0-gba-32x-megacd.md`.
Graphe des crates inchangé.

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
- Usage personnel uniquement : licences non commerciales compatibles (ADR 0001, roadmap), citées dans
  `THIRD-PARTY-DATA.txt` comme pour la 1.0. Les cores ne sont jamais redistribués par le projet.

### 2. BIOS

- **Jamais téléchargés** (ADR 0005). Import par « Importer un BIOS… » (3.0), md5 vérifié, nom attendu par le core.
- Nouvelle notion **BIOS requis par système** dans `cores.toml` (le `.info` de genesis_plus_gx les dit facultatifs,
  à tort pour le CD) : `[cores.genesis_plus_gx.required_firmware."Sega - Mega-CD - Sega CD"]` = au moins un des trois
  BIOS CD, avec préférence par région du jeu (`(Europe)` → E, `(USA)` → U, `(Japan)` → J, d'après le DAT ou le nom ;
  sinon U, comme le core). Même règle pour picodrive.
- **Contrôle avant lancement** : BIOS requis absent ou de mauvais hash ⇒ dialogue « BIOS Mega CD manquant :
  bios_CD_U.bin (Sega CD, USA) » avec « Importer un BIOS… », « Lancer quand même » (règle d'ADR 0013 §8 : les
  données peuvent être inexactes) et « Annuler ». Le refus d'un contenu par le core affiche en plus la dernière
  erreur journalisée par le core pendant `retro_load_game`, à la place du conseil générique.
- `gba_bios.bin` : facultatif ; présent ⇒ `mgba_use_bios = ON` (défaut du core) ; aucun avertissement s'il manque.
- 32X : rien.

### 3. Formats et médias

- **GBA** : `.gba` (zip accepté, contenu en mémoire). **32X** : `.32x` ; `.bin` seulement si l'en-tête porte
  `SEGA 32X` à 0x100 ; zip extrait par le runtime comme pour tout core `need_fullpath` (mécanisme 1.0).
- **Mega CD** : `.cue` (+ pistes `.bin` / `.wav` / `.ogg` référencées), `.chd`, `.iso`, `.m3u`. Les images CD sont
  **transmises en place, jamais copiées ni extraites** (`MediaRules::direct_extensions` = `cue`, `chd`, `iso`) ;
  **zip de CD refusé** avec un message (« décompressez, ou convertissez en CHD ») : 300-700 Mo dépassent
  `MAX_CONTENT_SIZE` et la copie n'apporterait rien (le core n'écrit pas dans les images).
- Multi-CD : regroupement `(Disc N)` / `(CD N)` de la bibliothèque (2.0) ⇒ `MediaSet` + `.m3u` généré dans
  `saves/<core_id>/` (chemins absolus vers les `.cue` / `.chd`) ; changement par le menu **Disquettes / CD**
  (interface disque 2.0). Un `.m3u` de l'utilisateur est utilisé tel quel.
- Bibliothèque : un `.cue` lu (borné à 64 Kio) **masque** les fichiers qu'il référence (pas de pistes `.bin` prises
  pour des ROM MD) ; un `.m3u` masque ses `.cue` / `.chd`. Une piste `.bin` orpheline commençant par
  `SEGADISCSYSTEM` est ignorée avec un avertissement.

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
  sinon numéro de série lu dans l'en-tête du disque (`SEGADISCSYSTEM`, champ série façon MD) ; `.chd` : nom de
  fichier puis série (lecture du CHD reportée). Le DAT Redump est déjà distribué par libretro-database, même canal
  et même mention que les DAT No-Intro (`THIRD-PARTY-DATA.txt`).

### 6. Entrées

- GBA : nouveau `PadStyle::Gba` (disposition SNES sans X/Y : A = K, B = J, L = O, R = L, flèches / ZQSD, Start,
  Select), profil `mgba` ; un `.gb` / `.gbc` lancé avec mgba garde le style Game Boy.
- 32X et Mega CD : pad Mega Drive (profils `picodrive` et `genesis_plus_gx`, déjà là) ; 6 boutons par défaut.
- Manette (ADR 0016) : L1 / R1 = L / R de la GBA ; rien d'analogique. La vibration (GET_RUMBLE_INTERFACE, quelques
  jeux GBA) reste hors périmètre (amendement d'ADR 0016 si demandé).

### 7. Hors périmètre

PS1, Saturn, 3DO (décision de l'utilisateur) ; rendu matériel (`SET_HW_RENDER`, toujours refusé : aucun core de la
6.0 n'en a besoin) ; Vulkan ; vibration ; Sega CD 32X (« CD 32X » : picodrive seulement, non testé) ; MSU-MD.
Si un jour PS1 ou Saturn étaient demandés : nouvelle ADR ; pistes connues — pcsx_rearmed (logiciel, BIOS HLE) avant
tout rendu matériel ; rendu matériel en **contexte GL unique** partagé avec egui (FBO du frontend, profil core
3.3 / 4.1 sur macOS, compatibilité et GLES refusés ⇒ yabasanshiro exclu), Vulkan écarté (pas de pilote natif sous
macOS Intel ni WSLg).

## Conséquences

- Aucune nouvelle dépendance, aucun `unsafe` nouveau attendu, aucun nouveau téléchargement (cores du buildbot,
  DAT et jaquettes par les canaux existants).
- Nouvelles API (détail : design §3) : `emu_assets` — BIOS requis par système, préférence de région ;
  `emu_library` — systèmes `gba`, `32x`, `megacd`, `DatSource::Redump`, masquage cue / m3u, identification CD ;
  `emu_libretro` — dernière erreur du core jointe à `LoadGameFailed` ; `emu-ui` — `PadStyle::Gba`, contrôle BIOS
  avant lancement.
- **Tests automatiques limités** : GBA réel (hello.gba, sans BIOS) ; 32X et Mega CD seulement par le pipeline
  (bibliothèque, choix du core, BIOS manquant ⇒ message lisible). Le jeu réel 32X / Mega CD est vérifié par
  l'utilisateur (ses ROM et BIOS). Construire une ROM 32X libre (yatssd + marsdev) est une option ouverte (design §9).
- Les fichiers `.brm` du Mega CD ne passent pas par le runtime : « réinitialiser les sauvegardes » d'un jeu doit
  aussi viser `<jeu>.brm` (motifs par core, mécanisme `reset_core_game_files` de la 4.0).
