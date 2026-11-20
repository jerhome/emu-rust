# Essais hatari (Atari ST) — étude de faisabilité des tests pour la 2.0

Date : 2026-11-20. Essais faits sous Xvfb avec l'appli `emu-rust` 1.0.0 (`target/debug/emu-rust`), sans modification du code.

## 1. Core hatari (buildbot libretro)

| Cible | URL | Résultat |
|---|---|---|
| Linux x86_64 | https://buildbot.libretro.com/nightly/linux/x86_64/latest/hatari_libretro.so.zip | téléchargé, 5 673 786 octets (zip), 20 039 968 (`.so`) |
| Windows x86_64 | https://buildbot.libretro.com/nightly/windows/x86_64/latest/hatari_libretro.dll.zip | HEAD 200, 4 509 038 octets, modifié 2026-11-20 |
| macOS x86_64 | https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/hatari_libretro.dylib.zip | HEAD 200, 8 164 703 octets, modifié 2026-11-20 |

Linux (nightly du 2026-11-16) :
- SHA256 zip : `d01a2438d7c527b04592af4c510e4cecf6cda13022cff4c984ed46773d2636c4`
- SHA256 `.so` : `8274196693e38051a8796c733c6e6839507771e5c8da9041f0f0d0430abbd124`
- `retro_get_system_info` (lu par un petit programme C avec `dlopen`) :
  `name=hatari version=2.6.1-devel (Nov 16 2026) ext=st|msa|dim|stx|ipf|m3u|ide|vhd|gem need_fullpath=1 block_extract=0`, API 1.
- Licence : GPL-2.0 (fichier `.info` libretro : `license = "GPLv2"` ; chaîne dans le binaire : « Hatari is free software licensed under the GNU General Public License »).
  Auteurs : Nicolas Pomarède, Thomas Huth, Eero Tamminen et al.
- Écart à noter : le `.info` annonce `zip` dans `supported_extensions`, pas le binaire (`block_extract=0`, pas de `zip`
  dans la liste). Une disquette dans un zip ne sera donc pas décompressée par le core : le frontend (ou la bibliothèque)
  doit extraire ou refuser. `.m3u` est supporté (multi-disquettes).
- `.info` : `needs_fullpath = true`, `supports_no_game = true`, `disk_control = true`, `core_options_version = 2.0`,
  `hw_render = false`, `savestate = true`, `database = "Atari - ST"`, `systemid = atari_st`.
- Symboles exportés : les 20 `retro_*` habituels, rien de particulier.
- Options du core (clés) : `hatari_machinetype`, `hatari_memory_size`, `hatari_ttram_size`, `hatari_cpu_*`, `hatari_fpu_*`,
  `hatari_dsp_type`, `hatari_monitor_type`, `hatari_video_timing`, `hatari_aspect_correct`, `hatari_borders`,
  `hatari_blitter`, `hatari_fastboot`, `hatari_fast_floppy`, `hatari_patch_tos`, `hatari_drive_a/b_*`,
  `hatari_floppy_write_protection`, `hatari_floppy_sound*`, `hatari_joystick_port0/1`, `hatari_joystick_autofire`,
  `hatari_joystick_jump_fire2`, `hatari_vkbd*`, `hatari_spec512_threshold`, `hatari_drive_leds`, `hatari_mmu`, `hatari_timerd`.

## 2. EmuTOS (TOS libre)

- Source officielle : https://emutos.sourceforge.io/ ; versions sur SourceForge
  (https://sourceforge.net/projects/emutos/files/emutos/). Dernière version stable : **1.4** (2025-06-07).
  Pas de releases GitHub exploitables (l'API renvoie 404).
- Licence : GPL-2.0-or-later (page du projet : « GNU General Public License, version … »). Redistribution possible
  (ADR 0005 : BIOS libre), à citer dans `THIRD-PARTY-DATA.txt`.
- Archives (URL de téléchargement : `https://sourceforge.net/projects/emutos/files/emutos/1.4/<nom>/download`) :
  - `emutos-192k-1.4.zip` (2 260 230 o) — SHA256 `59abac06a2d29b0864c5a7cfb2af65f022c337aed34188e174a9a08cc737e4bc` :
    images pour ST/STE/Mega (192 Ko, 22 langues).
  - `emutos-512k-1.4.zip` (3 708 741 o) — SHA256 `1ef7bd25f61bcfc66d19debc2f0ebb0d5e5ba811ccd7f0fddfcb105bb72d1663`
    (MD5 donné par SourceForge `23c29ba41d6306bffb10be68d813d299`) : TT/Falcon (readme : « TT, Falcon »).
- Fichiers recommandés :

| Fichier | Taille | SHA256 | SHA1 | CRC32 |
|---|---|---|---|---|
| `etos192us.img` (ST/STE, US) | 196 608 | `8fbbf8b44fc3e34281eaf8cda5265510e9af9ccda0e3e409111648060d244cfc` | `c03ae6379c88b31303dd6ebe63087054f716cba0` | `198c29f9` |
| `etos192fr.img` (ST/STE, FR) | 196 608 | `9fe2fc9c4458818bd0d401fc7a5ad5d487c83a5d5501d75f7737fbb84b9a7692` | `c4241b0e55c02e9171d407615b6e1d5bd76cdcb4` | `42cf26d0` |
| `etos512us.img` (TT/Falcon, US) | 524 288 | `167f5f148419a684a3646519ef12cd7be00e1a35e10040358184f4d471e27da9` | `7d28ee58f00854954073198983572e6921b87a21` | `b7e04d9f` |

  (Hashes des autres langues : recalculables ; le script de fetch devrait épingler le SHA256 de l'archive et de l'image choisie.)
- Attente du core : fichier **`tos.img`** dans le dossier système (`GET_SYSTEM_DIRECTORY`), nom fixe (chaîne `tos.img` et message
  « System BIOS (tos.img) is missing from your system folder » dans le binaire). Le `.info` indique « TOS 1.02 US »
  (md5 `c1c57ce48e8ee4135885cee9e63a68a2`, commercial) mais n'importe quelle image TOS valide est acceptée : essai fait avec
  EmuTOS. Il suffit donc de copier/renommer `etos192us.img` en `<system>/tos.img`.
  Pas d'option du core pour le nom ; la langue/pays se règle par le choix de l'image (EmuTOS « multi-pays » 512 Ko : option
  interne de pays). `hatari_machinetype` pilote ST/STE/MegaST/TT/Falcon ; la taille de ROM doit correspondre (192 Ko pour ST/STE).
- Conséquence pour `emu-assets` : un BIOS libre `atari_st` = `tos.img` (EmuTOS 192 Ko), vérifié par SHA256, extrait de l'archive SourceForge.

## 3. Essai headless avec l'appli existante

Commande (dossier système contenant `tos.img` = `etos192us.img`, disquette vide formatée) :

```text
xvfb-run -a -s "-screen 0 1280x800x24" target/debug/emu-rust --core hatari_libretro.so --rom blank.st \
  --data-dir D --system-dir D/sys --frames 600 --dump-frame f.png --no-audio
```

Résultats :
- Le core démarre avec EmuTOS **sans disquette utile** (disquette vierge en A:) : bureau GEM visible (barre de menus
  Desk/File/View/Options, icônes DISK A, DISK B, TRASH, fond vert, curseur souris), basse résolution 320x200.
- AV info : annonce 416x276 puis 320x200, 60 fps, 44 100 Hz ; le runtime suit le changement (`SET_SYSTEM_AV_INFO` / géométrie) sans erreur.
  Rendu logiciel (XRGB8888/RGB565 classique, `hw_render = false`) : **aucun besoin OpenGL côté core**.
- Aucun crash, 0 underrun audio sur 10 s, `exit 0`.
- Messages du core : `Hatari v2.6.1-devel (Nov 16 2026)`, `IPF : capsimage library version release=5 revision=1`,
  `NVRAM not found at <saves>/hatari/hatari.nvram` (info : le core écrit un NVRAM dans le dossier de sauvegarde),
  `Inserted disk '…/blank.st' to drive A:`, `Floppy A: has been removed from drive.` à l'arrêt.
- Avertissement du frontend : `DiskControl: frontend does not support the extended disk control interface, disk swapping via
  the frontend won't be available.`
- Callbacks d'environnement non gérés (niveau DEBUG `unhandled libretro environment command`) :
  - cmd 58 `SET_DISK_CONTROL_EXT_INTERFACE` : le core se rabat sur l'interface simple (13, gérée) ; l'échange de disquette
    via le frontend dépend du menu disque (à vérifier : prise en charge de `DiskControl` côté UI).
  - cmd 12 `SET_KEYBOARD_CALLBACK` : non géré. Les entrées clavier sont lues par `input_state(RETRO_DEVICE_KEYBOARD)` ;
    à vérifier si le frontend transmet bien les touches (un Atari ST est un ordinateur : clavier complet requis).
  - cmd 0x1002D (45 | EXPERIMENTAL) `GET_HW_RENDER_INTERFACE` : sans conséquence (pas de hw_render).
  - Souris (`RETRO_DEVICE_MOUSE` / pointeur) : le bureau GEM exige la souris ; aucune trace d'erreur mais le profil de touches
    actuel est « joypad » (20 touches liées, profil `hatari` existant) ; **non testé** (pas de pilotage souris dans `ScriptedInput`).
  - Notes : `GET_VARIABLE`/options v2 fonctionnent ; `SET_SUPPORT_NO_GAME` est accepté (le core annonce `supports_no_game`),
    mais le CLI exige `--rom`, donc le démarrage sans contenu n'a pas été essayé.
- Essai complémentaire (programme réel) : disquette 720 Ko avec `\AUTO\NOVA.PRG` (Nova Defense, voir §4) : TOS exécute le
  dossier AUTO au démarrage ; après 900 images, l'écran titre « NOVA DEFENSE — PRESS FIRE TO START » (couleurs, sprites)
  s'affiche correctement. C'est une base d'empreinte d'image fiable et déterministe (aucun clic nécessaire).

## 4. Données de test libres

Il n'y a pas, à notre connaissance, de disquette ST « officielle » de test sous licence libre (type 240p Test Suite).
Solution retenue : **fabriquer** les disquettes à partir de programmes sous licence libre, avec `tests/tools/mkst.py`
(Python 3 stdlib, FAT12 720 Ko, sortie reproductible : mêmes octets à chaque exécution ; pas de `mtools` ici).

| Programme | Licence | Source / URL | SHA256 |
|---|---|---|---|
| Nova Defense (ST, mono-fichier `NOVA.PRG`) | MIT | https://github.com/geekychris/atari_st_games/releases/download/games-2026-10-30/nova_defense-atari.zip | zip `e7bce16099f3ae508deb35b386bf9cba4c73f96379f2a14f8eb0b2366cb80c4b` ; `NOVA.PRG` `94a24e206f435522c9e731866841a01a407abfe64c76a16d971f9fb6453f01f5` |
| Frank the Frog (ST) | MIT | …/games-2026-10-30/frank_the_frog-atari.zip | zip `d3ab9243c29f43f88fbfe249a3ebe73722e5d913a4eb8ebb8ee8808262f3b616` (PRG non essayé) |
| Rayoid (jeu type Asteroids, 1992) | Unlicense | https://github.com/gorhill/rayoid (`dist/RAYOID.PRG`, 90 430 o) | non calculé |
| Disquette vide formatée | sans objet | générée (720 Ko, FAT12, boot sector avec BPB) | à épingler après génération |

- Les releases de `geekychris/atari_st_games` sont étiquetées par date (`games-2026-10-30`) : épingler l'URL de ce tag et le SHA256.
  Licence MIT du dépôt confirmée par l'API GitHub (`LICENSE`), sources fournies. Le tag peut être déplacé par l'auteur : le SHA256 pinné protège.
- Ce sont des **PRG dans un dossier**, pas des images disque : il faut `mkst.py` (ou un `.gem`/dossier GEMDOS, non essayé).
  `nova.st` généré (`mkst.py nova.st NOVA.PRG`) : SHA256 `bb3bb0c633094f0df0545017b2e6993d2a15df1f3def774be1614d2e60d7e681`.
- Les entrées : Nova Defense se joue avec souris/clavier/joystick (« Press fire to start »). Un test d'entrée nécessitera
  soit le joystick libretro (port 1 ou 0, option `hatari_joystick_port*`), soit le clavier.
- Le zip des jeux contient aussi `READ_ME.TXT` (instruction : copier le dossier sur disquette, ou `--harddrive` en Hatari natif).

## 5. Recommandations pour l'architecte

1. **Faisabilité : oui.** Le core tourne tel quel avec notre frontend 1.0, rendu logiciel, audio sans underrun, bureau GEM atteint.
2. **BIOS** : `tos.img` obligatoire dans le dossier système ; fournir EmuTOS 1.4 (192 Ko US, éventuellement choix de langue)
   via `emu-assets` (archive SourceForge, SHA256 épinglé, licence GPL-2.0+). Ajouter la mention dans `THIRD-PARTY-DATA.txt`.
   Le message d'erreur du core est clair ; prévoir un contrôle préalable côté appli (`--check-bios`).
3. **Contenu** : extensions `st|msa|dim|stx|ipf|m3u|ide|vhd|gem`, `need_fullpath`, pas de zip géré par le core
   → scan bibliothèque : décider si on extrait les zip (comme pour d'autres systèmes) avant lancement.
4. **Fonctions frontend à prévoir** (ordinateur, pas console) :
   - clavier complet (RETRO_DEVICE_KEYBOARD, éventuellement `SET_KEYBOARD_CALLBACK`) et souris (bureau GEM) — le profil de
     touches actuel `hatari` ne couvre que le joypad ; ADR sur les entrées « ordinateur » ;
   - contrôle de disque : le core utilise l'interface simple ; implémenter `SET_DISK_CONTROL_EXT_INTERFACE` (58) pour le
     changement de disquette et `.m3u` ;
   - changements de géométrie AV (320x200 / 640x400 / 640x200 selon la résolution ST) : déjà suivis, vérifier le ratio d'aspect ;
   - NVRAM dans le dossier de sauvegarde (`hatari/hatari.nvram`).
5. **Tests** : test `#[ignore]` d'empreinte sur le bureau EmuTOS (disquette vide, 600 images) et sur Nova Defense (`AUTO`,
   900 images) ; ajouter à `scripts/fetch-test-assets.sh` : core hatari (TOFU), EmuTOS 1.4 (SHA256 épinglé), Nova Defense
   (SHA256 épinglé), génération de la disquette via `tests/tools/mkst.py`. Tests d'entrée (clavier/souris) après le travail
   frontend. Un test « sans disquette » dépend de la prise en charge de `--rom` facultatif (`SET_SUPPORT_NO_GAME`).
6. **Licences** : hatari GPL-2.0 et EmuTOS GPL-2.0+ sont compatibles avec un usage en tant que données/cores externes ; ne pas
   redistribuer de TOS Atari commerciaux.
7. **Version du core** : nightly (`2.6.1-devel`), sans version stable épinglée → TOFU comme les autres cores.

## Fichiers créés

- `docs/essais-hatari.md` (ce rapport) ; `tests/tools/mkst.py` (générateur de disquette ST, utilitaire de test).
- Scratchpad nettoyé ; aucun conteneur ni image Docker créé.
