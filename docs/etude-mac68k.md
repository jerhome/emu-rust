# Étude préliminaire — Macintosh 68k (SimCity)

Date : 2026-11-23 (architecte). Statut : **étude**, ni ADR ni découpage. Aucun code touché, rien compilé ni exécuté,
aucun binaire ni ROM téléchargé : lecture seule du buildbot (index), des `.info` et du code source du core.

## Verdict

**Faisable avec limites** (palier 8.2 « ordinateurs »). Deux surprises par rapport au plan d'essais
(`docs/plan-essais-8x-9x.md` : « Mac Plus ») : le seul core libretro disponible émule un **Mac II**, pas un Mac Plus, et
il **ne lit pas `RETRO_DEVICE_MOUSE`** (souris pilotée par la manette). Il n'a ni savestates ni interface de
changement de disque libretro.

## 1. Core `minivmac` (libretro)

| Point | Constat | Source (consulté le 2026-11-23) |
|---|---|---|
| Dépôt | `libretro/libretro-minivmac`, branche `master`, 75 commits, dernier commit le **2026-04-20**, 4 tickets et 1 PR ouverts ; basé sur Mini vMac « MnvM_b36 » | https://github.com/libretro/libretro-minivmac |
| Buildbot | **présent pour nos 3 cibles**, build du jour : Windows x86_64 `minivmac_libretro.dll.zip` (346 Ko, 2026-11-23 08:28), Linux x86_64 `.so.zip` (301 Ko), macOS x86_64 `.dylib.zip` (304 Ko, 07:27) | https://buildbot.libretro.com/nightly/windows/x86_64/latest/ ; …/linux/x86_64/latest/ ; …/apple/osx/x86_64/latest/ |
| Licence | GPL-2.0 (`.info` : `GPLv2`) ; compatible avec le chargement dynamique (ADR 0001) | https://raw.githubusercontent.com/libretro/libretro-core-info/master/minivmac_libretro.info |
| `.info` | `display_name "Mac II (minivmac)"`, `corename MinivmacII`, `systemid mac68k`, `systemname Mac68k`, extensions **`dsk\|img\|zip\|hvf\|cmd`**, `savestate = false`, `supports_no_game = false`, 1 firmware : `MacII.ROM`, **non facultatif**, `notes` : md5 `66223be1497460f1e60885eeb35e03cc` | même `.info` |
| Modèle | **un seul : Mac II** (68020, `-DMAC2=1`, écran **640×480, 8 bits/pixel**, ROM de 256 Ko, 6 lecteurs de disquettes). Pas de Plus ni de SE ; pas d'option de modèle (seules options : barre d'état, type de clavier Callback/Poll) | https://github.com/libretro/libretro-minivmac/blob/master/Makefile (ligne `CFLAGS += -DMAC2=1`), `minivmac/cfg/CNFGGLOB.h`, `libretro/libretro-core.c` |
| ROM | le code cherche, dans le dossier *system* : `MacIIx.ROM` puis `MacII.ROM` (et d'autres chemins). Le `.info` ne cite que `MacII.ROM`, mais `MacIIx.ROM` est aussi accepté ; la taille attendue est exactement 0x40000 octets | `minivmac/src/OSGLUERETRO.c` (`LoadMacRom`) |
| Disquettes | le jeu chargé (`info->path`) est inséré dans le lecteur ; au démarrage sans disque, le core tente `disk1.dsk`…`disk6.dsk` **dans le répertoire courant** (piège : chemin relatif). Images brutes (« raw mode ») ; support DiskCopy 4.2 non vérifié. Changement de disquette : navigateur de fichiers **interne au core** (touche Y de la manette), **pas** de `SET_DISK_CONTROL_INTERFACE` (0 occurrence) | `OSGLUERETRO.c` (`LoadInitialImages`), `libretro-core.c` |
| Disque dur | `hvf` annoncé en extension ; chargement d'un disque dur SCSI non vérifié |  |
| Son | activé (`MySoundEnabled 1`, 8 bits), tickets utilisateurs « Sound breakup » ouverts ; qualité non testée | `CNFGGLOB.h`, https://github.com/libretro/libretro-minivmac/issues |
| Clavier | `RETRO_DEVICE_KEYBOARD` lu par état (mode Poll) ou callback ; table `MACkeymap.h` ; clavier virtuel interne (L) | `libretro/retrostubs.c` |
| **Souris** | **pas de `RETRO_DEVICE_MOUSE` ni `POINTER`** : le pointeur Mac est piloté par le **stick analogique gauche** / croix directionnelle, vitesse 1 à 6 (R), A/B = bouton (SEL bascule souris/joystick) | `libretro/retrostubs.c` (`input_state_cb(…ANALOG…)`, `MOUSE_EMULATED`) |
| Savestates | **non** (`retro_serialize_size` = 0) | `libretro-core.c` |
| Incohérence | le `.info` dit `supports_no_game = false` alors que le core déclare `SET_SUPPORT_NO_GAME` ; sans contenu, aucun disque de démarrage | idem |

Conséquences : un Mac II sans disquette système bootable affiche l'icône « disquette avec point d'interrogation » ;
il faut donc **au moins une disquette ou image de démarrage System 6/7** (fournie par l'utilisateur), en plus du jeu.

## 2. Alternatives

| Option | État | Verdict |
|---|---|---|
| **BasiliskII-libretro** (r-type) | dépôt personnel, « very minimal and WIP port… only a POC », dernier commit 2017-02-28, exige `BskII_prefs`, fichier de keycodes, `Quadra605.rom`, image disque ; **pas de `.info` ni de build sur le buildbot** | écarté (https://github.com/r-type/BasiliskII-libretro) |
| **MAME** (`mame_libretro`, buildbot : oui) | contient les Mac (`macplus`, `macse`, …) mais c'est le core arcade/ordinateurs géant ; ROMs au format MAME, gestion des médias lourde ; hors périmètre 8.x (ADR 0001 : on n'invente pas de wrapper) | piste lointaine, non étudiée |
| Mini vMac autonome | émule Plus/SE/II selon la compilation (modèle fixé à la compilation) ; même base que le core, mais hors libretro | référence uniquement (https://www.gryphel.com/c/minivmac/) |
| Basilisk II autonome | 68k avec ROM Quadra/LC patchée, très bon pour System 7 ; hors libretro | référence uniquement |
| **Snow** (Rust, MIT) | émulateur matériel de Mac 128K à LC III (Plus, SE, Classic, II, IIx, SE/30…), 2064 commits, très actif ; **application autonome avec son interface, pas de core libretro** | à surveiller ; l'utiliser comme crate (MIT, Rust) serait une architecture « core maison » : **exclu par l'ADR 0001** sans nouvelle ADR (https://github.com/twvd/snow) |

Aucun core libretro maintenu pour Mac Plus/SE. Le Mac II du core n'est pas un défaut pour SimCity (voir §3).

## 3. Faisabilité dans emu-rust

| Besoin | Constat |
|---|---|
| Système | `systemid mac68k` absent des 4 systèmes natifs ; **entrée « système personnalisé »** de la 8.0 ou système natif 8.2 ; extensions `dsk\|img\|hvf\|cmd` très génériques (`.img` ambigu avec DOS/CD) |
| Firmware | `MacII.ROM` (ou `MacIIx.ROM`), md5 donné par le `.info`, à vérifier par hash (ADR 0005) ; **import guidé par l'utilisateur, jamais téléchargé** ; le hash complet SHA-1/CRC32 de référence est à relever (le `.info` ne donne que le md5, la table de hashes d'emu-rust devra l'ajouter) |
| Clavier | clavier complet et clavier virtuel de la 2.0 : **réutilisables** (le core lit `RETRO_DEVICE_KEYBOARD`). Prévoir ⌘ et Option (touches Mac) : mapping `MACkeymap.h` à vérifier |
| Souris | **point dur**. La souris capturée de la 2.0 produit `RETRO_DEVICE_MOUSE`, que le core ignore. Options : (a) mode « souris → stick gauche » dans emu-rust (par core, option `mouse_as_analog`) : déplacements relatifs convertis en axe analogique, imprécis (vitesse 1-6, pas de relatif pur) ; (b) manette/clavier directionnel : utilisable pour SimCity (menus, outils) mais pénible ; (c) demander un correctif amont (ticket/PR libretro-minivmac) : hors de notre contrôle. SimCity est entièrement à la souris : **la qualité du résultat dépend de (a)** |
| Médias | jeu = disquette de SimCity (800 Ko, image `.dsk`) ; **disquette système de démarrage** à insérer en plus. Pas d'interface de changement de disque libretro : l'écran « Changer de média » d'emu-rust ne sait pas piloter le core ; le jeu et le système doivent être présents au démarrage (disques `disk1.dsk`… relatifs au répertoire courant : à contourner par le chemin du jeu, un seul disque par `info->path` ; le 2e lecteur impose de passer par le navigateur interne du core ou un `.zip`/m3u non supporté) |
| Disque dur | un seul fichier disque système + jeu copié dedans serait plus pratique, mais le chargement de disque SCSI n'est pas vérifié |
| Son | à écouter ; SimCity 1.x a des effets sonores 8 bits simples |
| Image | 640×480 fixe, RGB565 ou XRGB8888 : trivial pour `emu-runtime` ; rendu logiciel seulement (compatible 8.0) |
| Sauvegarde | pas de savestates ; SimCity sauvegarde sur la disquette du jeu (image modifiée) : politique d'écriture sur le disque image à décider (copie de travail, jamais l'original) |

**SimCity concret (à confirmer avec l'utilisateur)** : SimCity « Classic » pour Macintosh (1989, disquettes 800 Ko ou
1,4 Mo selon l'édition ; version 1.2 couleur sur Mac II) sur Mac II avec System 6.0.x ou 7.0/7.1 (68020, 8 bits couleur
ou noir et blanc). Source : connaissance générale, **non vérifiée** ici ; le Mac II n'est pas le Plus du plan d'essais,
mais SimCity tourne sur 68000 et 68020 : pas de blocage attendu. SimCity 2000 demande davantage (7.x, couleur, 4 Mo
de RAM, taille de RAM du core non vérifiée) : à ne pas promettre.

**Palier conseillé** : 8.2 (ordinateurs), **après** décision sur le mode « souris → stick » et après
vérification par un essai du core sur un poste de test (démarrage depuis une image System 6, souris, son).

## 4. Recommandation

**Faisable avec limites.** À faire seulement si l'utilisateur accepte : un seul modèle (Mac II), une souris par
conversion stick, pas de savestates, changement de disquette via le navigateur du core, et un travail d'ajout du
système (extensions génériques, firmware, deux médias au démarrage). Étape zéro peu coûteuse : l'utilisateur essaie
le core dans RetroArch (ou un de nos écrans « système personnalisé » de la 8.0) avec `MacII.ROM`, un System 6 et SimCity ;
verdict avant tout développement spécifique. Alternative si la souris est inacceptable : abandonner le Mac 68k (Snow
exclu par l'ADR 0001).

Questions : `docs/questions-ouvertes.md`, section « Mac 68k ».
