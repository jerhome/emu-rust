# Essais puae (Amiga) — étude de faisabilité pour la 3.0

Date : 2026-11-20. Essais sous Xvfb avec `target/debug/emu-rust` (binaire de 18:08 construit depuis la branche `wip-2.0`,
donc **antérieur** à la prise en charge actuelle, dans les sources, des commandes 12/13/57/58 ; à rejouer avec un build neuf),
sans modification du code. Modèle : `docs/essais-hatari.md`.

## 1. Cores puae (buildbot libretro)

| Core | Cible | URL (`https://buildbot.libretro.com/nightly/…/latest/`) | Résultat |
|---|---|---|---|
| puae | Linux x86_64 | `linux/x86_64/puae_libretro.so.zip` | téléchargé, 7 160 253 o (zip), 20 933 288 (`.so`) |
| puae | Windows x86_64 | `windows/x86_64/puae_libretro.dll.zip` | HEAD 200, 6 509 262 o |
| puae | macOS x86_64 | `apple/osx/x86_64/puae_libretro.dylib.zip` | HEAD 200, 6 995 454 o |
| puae2021 | Linux x86_64 | `linux/x86_64/puae2021_libretro.so.zip` | téléchargé, 3 807 573 o (zip), 9 533 224 (`.so`) |
| puae2021 | Windows x86_64 | `windows/x86_64/puae2021_libretro.dll.zip` | HEAD 200, 3 638 391 o |
| puae2021 | macOS x86_64 | `apple/osx/x86_64/puae2021_libretro.dylib.zip` | HEAD 200, 3 947 042 o |

Les trois plateformes existent pour les deux cores (modifiés le 2026-11-20). Seules les versions Linux ont été téléchargées et essayées.

Linux (nightly du 2026-11-20) :
- puae : zip `bca7e55b08dd814de814f6152c125ce37ea7b0a0dccaf4bc443d3427b4068e03`, `.so` `b2df568c5e0125740680a1fe0a1752659bd400a0d37837908bcd1ab64e44a1a7`.
- puae2021 : zip `75ec87c68f178402f3671029847da05ff394cf4cf9dc480572eb6f6da28cbc08`, `.so` `0c728172210aca413d7d7e44253d279824bcde822577888334f4e93d8f6918ac`.
- `retro_get_system_info` (programme C avec `dlopen`, lié à libm : le `.so` a besoin de `libm`) :
  - puae : `name=PUAE version=5.3.1 d366f77 ext=adf|adz|dms|fdi|raw|ipf|hdf|hdz|lha|slave|info|cue|ccd|nrg|mds|iso|chd|uae|m3u|zip|7z|/ need_fullpath=1 block_extract=1`
  - puae2021 : `name=PUAE 2021 version=2.6.1 5dbac2b`, mêmes extensions, mêmes drapeaux.
- Licence : **GPL-2.0** (`.info` : `license = "GPLv2"` ; dépôt https://github.com/libretro/libretro-uae, `COPYING` GPLv2). Basé sur WinUAE 5.3.1 (README).
- `.info` (https://raw.githubusercontent.com/libretro/libretro-super/master/dist/info/puae_libretro.info) : `supports_no_game = true`,
  `disk_control = true`, `savestate = true` (serialized), `hw_render = false`, `needs_fullpath = true`, `core_options = true`,
  `database = "Commodore - Amiga|Commodore - CD32|Commodore - CDTV"`, `systemid = commodore_amiga`, `display_version` 5.3.1 (puae) / 2.6.1 (puae2021).
- **`block_extract = 1`** : contrairement à hatari, le frontend ne doit PAS décompresser (zip/7z) : le core le fait lui-même
  (`Unzip: <saves>/puae/TEMP/Boot.adf`, répertoire temporaire vidé à la sortie, jamais supprimé).
- Choix : **puae** (5.3.1, précis, A500 à A4000/CD32) comme core par défaut ; puae2021 (2.6.1, moins précis, plus léger : 9,5 Mo contre 20,9)
  en option « machine faible ». Les deux démarrent dans nos essais ; l'image de puae2021 diffère de puae (empreinte différente) sur la disquette de test.

## 2. ROM libre AROS

**Fait majeur : le core embarque déjà AROS.** Aucun téléchargement n'est nécessaire.
- `.info` : « The core has a built-in AROS fallback Kickstart that is compatible enough for some A500 games, but a proper Kickstart ROM is preferred. »
  README upstream : « The core has a somewhat compatible built-in AROS Kickstart, which is used as a fallback when the proper Kickstart is not found. »
- Dans le `.so` : chaînes `AROS KS ROM (built-in)`, `aros.bin`, `aros.gz`. Source : `sources/src/aros.rom.c` du dépôt libretro-uae
  (3,4 Mo, tableau C d'un gzip `aros.bin` ; dernier commit « Revert AROS update », 2025-01-27).
- ROM extraite du binaire (gzip → 1 048 576 o, ROM + extension AROS EXT) : SHA256 `1211897caea79785f2441da4b75a460255b7f85e5129e03fe0ab61a8aebfb2c1`
  (build 2022-01-26 : `graphics.library 45.1 (26.1.2022)`, `keymap.library 41.2 (26.1.2022)`).
- Licence AROS : **AROS Public License (APL) 1.1** (https://aros.sourceforge.io/license.html), compatible avec une redistribution
  (ADR 0005 : BIOS libre) ; le dépôt https://github.com/aros-development-team/AROS n'expose pas de licence SPDX (`NOASSERTION`). Cité dans `THIRD-PARTY-DATA.txt`
  si nous redistribuons ; ici rien à redistribuer, la ROM est dans le core (GPL-2.0, qui l'inclut).
- Un téléchargement séparé « aros.rom / aros-ext.bin » (fournis par libretro ou nightly AROS) n'a **pas** été trouvé nécessaire et n'a pas été vérifié : les URL
  officielles aros.sourceforge.io n'exposent pas de ROM m68k directe (pages de téléchargement sans lien ROM) ; à ne pas poursuivre.
- **Sélection du Kickstart** (README upstream) : le core cherche, dans le **dossier système** (`GET_SYSTEM_DIRECTORY`, directement ou dans `Devs/Kickstarts`), le fichier du modèle
  choisi ; noms Amiga Forever et TOSEC acceptés (Amiga Forever chiffré : `rom.key` dans le même dossier). Si absent : message
  `Kickstart ROM "kick34005.A500" not found!` (niveau ERROR dans le log, mais **non fatal**, repli sur AROS intégré). Il n'y a pas d'option
  « fichier » : le modèle (option `puae_model`, Automatic → A500 pour une disquette, A1200 pour un disque dur/LHA, CD32 pour un CD) et l'option `puae_kickstart`
  (Automatic par défaut) pilotent le choix ; si le Kickstart d'origine du modèle est présent, il est utilisé, sinon AROS.
  L'effet exact de `puae_kickstart` avec une ROM présente n'a pas été testé (aucune ROM commerciale téléchargée : ADR 0005).
- Noms et MD5 attendus (doc libretro, `.info` + README ; **à ne pas télécharger**) :

| Fichier | Contenu | Taille | MD5 |
|---|---|---:|---|
| `kick33180.A500` | KS 1.2 (A500-A2000) | 262 144 | `85ad74194e87c08904327de1a9443b7a` |
| `kick34005.A500` | KS 1.3 (A500-A2000-CDTV) | 262 144 | `82a21c1890cae844b3df741f2762d48d` |
| `kick37175.A500` | KS 2.04 (A500+) | 524 288 | `dc10d7bdd1b6f450773dfb558477c230` |
| `kick37350.A600` | KS 2.05 (A600) | 524 288 | `465646c9b6729f77eea5314d1f057951` |
| `kick40063.A600` | KS 3.1 (A600-A2000) | 524 288 | `e40a5dfb3d017ba8779faba30cbd1c8e` |
| `kick39106.A1200` | KS 3.0 (A1200) | 524 288 | `b7cc148386aa631136f510cd29e42fc3` |
| `kick40068.A1200` | KS 3.1 (A1200) | 524 288 | `646773759326fbac3b2311fd8c8793ee` |
| `kick39106.A4000` | KS 3.0 (A4000) | 524 288 | `9b8bdd5a3fd32c2a5a6f5b1aefc799a5` |
| `kick40068.A4000` | KS 3.1 (A4000) | 524 288 | `9bdedde6a4f33555b4a270c8ca53297d` |
| `kick34005.CDTV` | extension CDTV | 262 144 | `89da1838a24460e4b93f4f0c5d92d48d` |
| `kick40060.CD32` | KS 3.1 CD32 (524 288, ou combiné 1 048 576 `f2f241bf094168cfb9e7805dc2856433`) | 524 288 | `5f8924d013dd57a89cf349f4cdedc6b1` |
| `kick40060.CD32.ext` | extension CD32 | 524 288 | `bb72565701b1b6faece07d68ea5da639` |
| `kick31034.A1000` / `kick32034.A1000` | KS 1.1 NTSC / PAL | 262 144 | `0b8442c311caa54fb12ec88eaaa9facf` / `1fa1f93d3d7b51271dd1356b8b2b45a9` |

  Attention : `.info` marque `kick34005.A500`, `kick40068.A1200`, `kick40060.CD32` et `.ext` comme **non optionnels** (`firmware*_opt = false`), alors que le core
  démarre sans (AROS). Le contrôle de BIOS de la 3.0 (`--check-bios`) doit traiter l'Amiga comme « aucun BIOS requis, Kickstart d'origine recommandé » et ne pas bloquer.
  Le README insiste : « It is critical to use ROMs with the correct MD5 ».
- Avertissement de compatibilité : AROS convient à certains jeux A500 seulement ; beaucoup de jeux et de démos exigent le vrai Kickstart (indication à l'utilisateur).

## 3. Essai headless avec l'appli existante

Commande :

```text
xvfb-run -a -s "-screen 0 1280x800x24" target/debug/emu-rust --core puae_libretro.so --rom <fichier> \
  --data-dir D --system-dir D/sys --save-dir D/saves --frames 900 --dump-frame f.png --no-audio
```

(dossier système **vide**, donc AROS intégré.) Résultats :
- Démarre, `exit 0`, aucun crash, **0 underrun audio**, 44 100 Hz, **50 fps** (PAL).
- AV info : 360x287 au chargement ; puis la géométrie change (AROS : 720x574 entrelacé « Waiting for bootable media » avec les yeux de chat, ~après 600 images ;
  à 600 images l'écran est encore gris uniforme, prévoir ≥ 1000 images pour un test sur AROS vierge) ; disquette de test : 720x287 (hires, une ligne).
  Le runtime suit les changements de géométrie sans erreur. Rendu logiciel (`hw_render = false`) : aucun OpenGL requis côté core.
- Messages du core (info) : `PUAE 5.3.1 d366f77, Nov 20 2026`, `No model specified in … Model: "A500"`, `Kickstart ROM "kick34005.A500" not found!` (ERROR **non fatal**, le message
  remonte tel quel dans l'UI : `core : Kickstart ROM 'kick34005.A500' not found!` → à filtrer ou reformuler « AROS intégré utilisé »),
  `Disk (1) inserted in drive DF0`, `CPU=68000 … cycle exact`, `PAL mode V=50.0000Hz`, `Illegal instruction 4e7b` / `B-Trap` (AROS sonde le CPU, sans conséquence).
- Bruit : le core écrit sur **stdout** des lignes `retro_vfs_file_open_impl '<chemin>' <mode>` (sondage de fichiers `*_save.adf`, `blank.save_adf`, etc.), sans conséquence mais très verbeux.
- Fichiers créés par le core : `<saves>/puae/` (dossier de travail, `TEMP/` pour les zip, disquette de sauvegarde `<nom>_save.adf` si demandée) ; `WHDLoad.prefs` dans le dossier système (README) ;
  option `puae_shared_nvram`.
- Callbacks d'environnement **non gérés** vus dans le binaire essayé (niveau DEBUG) : 12 `SET_KEYBOARD_CALLBACK`, 13 `SET_DISK_CONTROL_INTERFACE`, 28 `GET_PERF_INTERFACE`,
  57 `GET_DISK_CONTROL_INTERFACE_VERSION`, 64 `SET_FASTFORWARDING_OVERRIDE`, `45|exp` `GET_VFS_INTERFACE`, `46|exp` `GET_LED_INTERFACE`. À rejouer sur un build neuf (les sources wip-2.0 gèrent déjà 12, 13, 57, 58).
  Le core utilise le clavier (touches Amiga complètes, `puae_physical_keyboard_pass_through`, clavier virtuel), le pointeur/souris (`puae_physicalmouse`, `puae_joyport`), le joystick (RetroPad)
  et le contrôle de disque (M3U, ejection/insertion). Non essayé : entrées (pas de pilotage souris/clavier dans le harnais headless).
- Aucun besoin : OpenGL, hw_render, caméra, BIOS obligatoire.

Empreintes (screenshots PNG 720x287, déterministes sur deux exécutions) :
| Contenu | Images | SHA256 du PNG |
|---|---:|---|
| `boot.adf` (arc-en-ciel, §4) | 900 | `6ae7d5c9157f524779e295489ec84c48a9dcbfcca65ef62f7e08b9f6662522eb` |
| `boot.zip`, `Boot.adz` (même ADF compressé) | 900 | identique à `boot.adf` : zip et adz sont gérés par le core |
| AmigaTestKit 1.21 | 900 | `6eda690dac3b2041bcac1cf24875ca20cea228073bdf7282cd134cb36da9acd3` |

Image d'AmigaTestKit : menu « Amiga Test Kit 1.21 — by Keir Fraser » (F1 Memory, F2 Keyboard, F3 Floppy Drive, F4 Controller Ports, F5 Audio, F6 Video, F7 CIA/Chipset,
F8 RTC, F9 Serial/Parallel), « 68000 - OCS/PAL - 50Hz », « ROM: Kickstart 3.1.4 (46.12) » (la ROM intégrée se présente ainsi), curseur de souris visible.
Image de `boot.adf` : barres horizontales multicolores sur tout l'écran. Image d'une disquette vierge : écran AROS sombre avec deux yeux de chat bleus et « Waiting for bootable media ».

## 4. Données de test libres

| Contenu | Licence | Source / URL | SHA256 |
|---|---|---|---|
| **Amiga Test Kit 1.21** (Keir Fraser) — suite de test système, `AmigaTestKit.adf` bootable | **Unlicense** (domaine public ; dépôt https://github.com/keirf/amiga-stuff, `COPYING`) | https://github.com/keirf/amiga-stuff/releases/download/testkit-v1.21/AmigaTestKit-1.21.zip | zip `0c609dc991394ba9f1831496e61ce40c74993a581d8a20541ba291d601fcb959` ; `AmigaTestKit.adf` `abe7426c93619a7bb61ce10e3e66a4747fcaf22acd1d1876310033faa700ad28` (901 120 o) |
| `boot.adf` : bootblock original (arc-en-ciel) | domaine public (code écrit pour le projet, 12 octets de code) | généré par `tests/tools/mkadf.py` (Python 3, stdlib, reproductible) | `428db5644b11a2b5daafd3b001f6902c91c2ff730aecc2a1a7479db706fee18d` |
| arnaud-carre/ldos (démos de bootblock, ADF prébuilts dans le dépôt) | MIT | https://github.com/arnaud-carre/ldos | non téléchargé |
| alpine9000/amiga_examples (BSD-2-Clause), codebase/amiga-game-kit (MIT, jeux AGA/OCS en zip) | indiquées | dépôts GitHub | non téléchargés / non essayés |

- **Recommandation** : épingler Amiga Test Kit (release taggée `testkit-v1.21`, fichiers immuables par tag mais SHA256 épinglé quand même) : il couvre disquette, clavier, souris/ports manettes,
  audio et vidéo, donc il servira aussi aux tests d'entrées (F2 Keyboard, F4 Controller Ports) quand le frontend 3.0 enverra clavier/souris. `boot.adf` sert de test minimal sans téléchargement (démarrage
  déterministe) ; tous les deux fonctionnent **sans Kickstart propriétaire**, avec l'AROS intégré.
- `amitools`/`xdftool` : non installé ici (pas de pip/pas d'accès nécessaire) ; licence du dépôt https://github.com/cnvogelg/amitools non confirmée (pas de `LICENSE` à la racine via l'API : 404, à vérifier avant tout usage).
  Inutile pour l'instant : `mkadf.py` + Amiga Test Kit suffisent.
- Ajout fait : `tests/tools/mkadf.py` (nouveau). Rien d'autre modifié hors `docs/essais-puae.md`.

## 5. Formats

Vérifié (essais) : **adf** (natif), **adz** (gzip d'un ADF, natif : même image que l'ADF), **zip** (décompressé par le core dans `<saves>/puae/TEMP/`, `block_extract=1`),
**m3u** (natif : « Disk (1) inserted in drive DF0 », « Disk (2) … DF1 » avec mon M3U de deux disques ; la doc dit M3U pour le changement via contrôle de disque et `(MD)` dans
le nom pour un disque par lecteur, maximum 4).

Documenté (README upstream, non testé faute de données libres) :

| Format | Pris en charge par puae | Traitement frontend |
|---|---|---|
| adf, adz, dms, fdi, raw, ipf (nécessite la bibliothèque CAPS : non vérifié, hatari embarque la sienne) | oui | aucun |
| hdf, hdz, lha (archive WHDLoad « préinstallée » lancée directement, `puae_use_whdload`), slave, info | oui (`WHDLoad.hdf` intégré au core ; fichiers d'aide générés dans `saves`) | aucun ; WHDLoad : touche de sortie codée en dur `LCtrl + \` ; `WHDLoad.prefs`, `WHDLoad.key`, `rom.key` copiés depuis le dossier système |
| zip, 7z | oui (extrait par le core, temp dans `saves`, pas de réempaquetage donc sauvegardes in-zip perdues) ; `archive.zip#disque.adf` pour choisir | ne **pas** extraire côté frontend ; la bibliothèque doit scanner le contenu du zip pour identifier |
| m3u | oui (`#SAVEDISK:Nom` crée un disque de sauvegarde `<m3u>.save<n>.adf`, étiquettes `FICHIER|LIBELLÉ`, chemins relatifs au M3U) | génération de M3U pour les jeux multidisques détectés en bibliothèque (ou laisser le core : « Automatic M3U playlist generation » pour les zip) |
| cue, ccd, nrg, mds, iso, chd | oui (CD32/CDTV) ; `uae` (fichier de configuration) ; `rp9` (dans le `.info` mais absent de la liste du binaire) | CD32 demande `kick40060.CD32(.ext)` ou AROS (faible compatibilité) |

Le modèle se déduit du chemin (balises `(A500)`, `(A1200)`, `AGA`, `(NTSC)`, `(MD)`, `(CE)` etc., table du README) : **le nom de fichier fait partie de la configuration**.
Le frontend doit donc éviter de renommer/déplacer les images et peut proposer ces balises (ou plutôt l'option `puae_model` par jeu).

## 6. Recommandations pour l'architecte

1. **Faisabilité : oui.** Le core tourne tel quel, sans BIOS, avec AROS intégré. Pas besoin de télécharger AROS : *la ligne « AROS (libre, téléchargé) » de `docs/roadmap.md`
   devient « AROS intégré au core »*. Le Kickstart d'origine reste fourni par l'utilisateur (vérification MD5 : le core utilise MD5, nous aurons SHA1/CRC32/MD5 dans `emu-library` ?
   prévoir un hash MD5 pour les ROM Amiga ; l'ADR 0005 parle de « hashes » sans MD5 imposé).
2. **BIOS** : dossier système, noms fixes ci-dessus. L'écran de BIOS du gestionnaire de cores doit lister les Kickstart en « recommandé » (jamais « manquant ») et expliquer le repli AROS.
   L'identification par nom de fichier ne suffit pas (« critical to use ROMs with the correct MD5 ») : vérifier le MD5 et proposer de renommer/copier au bon nom (`kick34005.A500`…).
   Les ROM Amiga Forever (`amiga-os-130.rom`…) sont acceptées telles quelles par le core.
3. **Réutilisation de la 2.0 (Atari ST)** :
   - **Touche hôte** : indispensable, le core laisse Échap, Ctrl, F1-F10, etc. à la machine. **Conflit mineur** : puae lie **Ctrl droit** à « Switch joystick/mouse » ; la touche hôte par défaut
     (Ctrl droit, ADR 0015) n'est jamais transmise au core, donc ce raccourci du core devient inaccessible ; la bascule joystick-au-clavier (Hôte + J, mémorisée par jeu) joue ce rôle, ou
     l'option `puae_joyport`/`puae_mapper_*`. Autre piège : la touche **WHDLoad `#`** (RETROK_HASH) ; `LCtrl + \`, **Amiga gauche/droite** (touches Windows/⌘ → `Meta`) : ⌘ droit est la touche hôte par défaut
     sous macOS, donc **Amiga droit** (⌘ droit) est perdue : à documenter, ou proposer une autre touche hôte pour l'Amiga.
   - **Clavier complet et `SET_KEYBOARD_CALLBACK`** (cmd 12, demandé par le core) : même code que pour le ST. Le core gère aussi son clavier virtuel (Select court) et `puae_physical_keyboard_pass_through`.
   - **Souris capturée** : le Workbench et beaucoup de jeux l'exigent ; le core propose `puae_physicalmouse` et l'émulation par stick analogique ; même infrastructure que le ST (capture/libération par la touche hôte).
   - **Disquettes multiples / changement à chaud** : interface de disque standard (`SET_DISK_CONTROL(_EXT)_INTERFACE`, M3U) : le menu Disquettes et Hôte + D de la 2.0 fonctionnent tels quels.
     Différences : jusqu'à **4 lecteurs** (`puae_floppy_multidrive`, balise `(MD)`), disque de sauvegarde `#SAVEDISK` (nouveau besoin d'écriture dans `saves`), et disquettes IPF/DMS/ZIP lues par le core.
   - **Options par jeu** : cruciales pour l'Amiga (`puae_model`, `puae_video_standard` PAL/NTSC, `puae_cpu_*`, mémoire, `puae_video_resolution`, `puae_use_whdload`). Même mécanisme que pour hatari ; prévoir un jeu d'options
     « prérégler le modèle » (A500, A1200…) ; le modèle automatique par balise de chemin (§5) est un second mécanisme à ne pas casser.
   - **BIOS libre** : modèle différent du ST (AROS intégré, rien à copier) ; le mécanisme `emu-assets` des BIOS ne s'applique qu'au Kickstart fourni par l'utilisateur.
   - **Géométrie** : PAL 360x287 / 720x287 / 720x574 (entrelacé) en cours de jeu (valeurs essayées ; README : 360-1440 x 288-576, NTSC 240/480). Le ratio d'aspect (option `puae_video_aspect`) et le suivi
     du changement de taille sont critiques ; 50 fps (PAL) ou 60 (NTSC) : le fps du core change avec la norme (`SET_SYSTEM_AV_INFO` à surveiller).
4. **Tests** (`#[ignore]`, après fetch) : core puae (TOFU comme les autres), Amiga Test Kit (SHA256 épinglé, empreinte d'image après 900 images), `boot.adf` (généré, 900 images, empreinte
   ci-dessus), zip/adz/m3u pour valider la politique « ne pas extraire côté frontend ». Un test AROS vierge (écran « Waiting for bootable media ») demande ≥ 1000 images. Les tests d'entrée
   attendent le travail clavier/souris de la 2.0 (Amiga Test Kit F2/F4 pour les valider).
5. **Licences** : puae GPL-2.0 (+ AROS APL incluse dans le binaire) ; Amiga Test Kit Unlicense ; **ne rien redistribuer de Commodore/Cloanto** (Amiga Forever, Kickstart, Workbench). Mentionner AROS dans
   `THIRD-PARTY-DATA.txt` si des fichiers sont fournis ; sinon dans les notices du core.
6. **Version du core** : nightly (`5.3.1 d366f77`), pas de version stable : TOFU. Choisir **puae** par défaut, **puae2021** comme second.
7. **UI du dossier de ROMs** (demande utilisateur, 3.0) : sans lien avec ces essais ; noter seulement que les images Amiga peuvent être des **dossiers** (WHDLoad « / » dans la liste d'extensions) et des
   archives : la bibliothèque doit accepter un dossier comme contenu.

## Fichiers créés

- `docs/essais-puae.md` (ce rapport) ; `tests/tools/mkadf.py` (générateur d'ADF bootable minimal).
- Scratchpad nettoyé ; aucun conteneur ni image Docker créé.
