emu-rust - frontend libretro (usage personnel)
==============================================

Aucun core, BIOS ni jeu n'est fourni. Vous fournissez le core libretro et la ROM.

0. Cores depuis l'application
-----------------------------
Sans argument, emu-rust ouvre la bibliothèque de jeux : indiquez le dossier de vos ROMs dans les Réglages, lancez
le scan, puis installez les cores depuis l'écran Cores, ou en ligne de commande :
  emu-rust --install-core recommended      (aussi : --list-cores, --update-cores)
Sous Windows, deux exécutables : emu-rust.exe (interface, sans console) et emu-rust-cli.exe (console : à utiliser pour
les commandes en ligne, p. ex. emu-rust-cli --install-core recommended, --help, --version, afin de voir leur sortie).
Les cores sont téléchargés depuis le buildbot libretro (HTTPS), vérifiés, et rangés dans le dossier de données
(cores/). Les BIOS ne sont jamais téléchargés : l'écran Cores indique ceux qui manquent. Les jaquettes sont
désactivées par défaut (activation dans les Réglages).
La méthode manuelle ci-dessous reste possible : --core accepte un chemin.

0a. Rangement des ROMs, Journal, console (7.1)
----------------------------------------------
Rangement : bibliothèque > « Ranger ». L'application construit d'abord un APERÇU (déplacements prévus, par système,
sans rien toucher) ; le bouton Ranger n'agit qu'ensuite. Chaque rangement est journalisé et « Annuler » rétablit
l'état précédent tant que les fichiers n'ont pas été modifiés depuis. Les fichiers non identifiés ne sont pas déplacés.
Journal : panneau « Journal » (pied de page de la fenêtre) : messages et erreurs récents de l'application.
Console (Windows) : Réglages > « Afficher la console » ouvre une console avec emu-rust.exe, utile au diagnostic ;
sans ce réglage emu-rust.exe n'ouvre aucune fenêtre de terminal.

0b. Atari ST (2.0, en développement)
------------------------------------
Core : hatari (écran Cores). BIOS : EmuTOS 1.4 (GPL-2.0-or-later, libre), proposé et téléchargé automatiquement
par l'application (HTTPS, SHA-256 vérifié) sous le nom system/tos.img. Aucun TOS Atari commercial n'est téléchargé ;
un tos.img que vous avez placé vous-même n'est jamais écrasé.
Disquettes : .st .msa .stx .ipf .dim, ou .m3u pour plusieurs disquettes (copies de travail : l'original reste intact).
Touche hôte : Ctrl droit partout, macOS compris (⌘ droit réglable en repli). Seule : menu. Avec une autre touche :
  S / L = sauvegarder / charger l'état   , / . = emplacement -/+   R = reset   P = pause   F = plein écran
  I = stats   Espace (maintenu) = avance rapide   K = clavier virtuel   M = capturer / libérer la souris
  J = mode Clavier / Joystick   D = disquette suivante (menu Disquettes aussi)
Souris : capturée au premier clic dans la fenêtre ; libérée par la touche hôte (menu) ou Hôte + M.
Sous noVNC ou WSLg la capture n'est pas disponible : la souris reste libre (utiliser le clavier virtuel).

0c. Amiga (3.0, en développement)
---------------------------------
Cores : puae (par défaut ; précis, A500 à A4000 / CD32) et puae2021 (plus léger, recommandé sur Mac Intel), écran Cores.
Disquettes : .adf .adz .dms .ipf (lecture seule) .fdi, disques durs .hdf, archives WHDLoad .lha, ou .m3u pour plusieurs
disquettes (DF0, DF1, ...). Les écritures se font sur des copies de travail : l'original reste intact. Rangez vos jeux
dans un dossier amiga/ de la bibliothèque (le dossier distingue .ipf Amiga et Atari ST). Le modèle de machine se déduit
des étiquettes du nom de fichier, p. ex. (A1200) ou (A500+), sinon A500 ; réglable par jeu.
Kickstart : jamais fourni ni téléchargé (propriété de Cloanto / Commodore). Sans Kickstart, le core utilise AROS, une
ROM libre intégrée au core (compatibilité limitée : certains jeux ne démarrent pas). Pour une meilleure compatibilité,
importez le vôtre : écran Cores ou dialogue AROS, bouton « Importer un BIOS... » ; l'application reconnaît le fichier
par son empreinte et l'installe sous le nom attendu par le core (system/kick34005.A500, kick40068.A1200, ...). Un
fichier remplacé est conservé (suffixe .user-<date>). Voir aussi docs/bios-utilisateur.md.
Touches : Alt gauche / Alt droit = Amiga gauche / droite. Touche hôte : Ctrl droit (menu), comme pour l'Atari ST.
Mode d'entrée par défaut : Clavier (souris capturée au premier clic) ; Hôte + J bascule en joystick.
Le bouton « Ouvrir le dossier des ROMs » (bibliothèque) ouvre le dossier dans l'explorateur de fichiers ; sous Docker
il affiche le chemin du volume (/data/roms) et, si EMU_HOST_ROMS_DIR est défini, le dossier de l'hôte monté dessus.
Licences non embarquées dans le binaire : AROS (AROS Public License 1.1) est fourni par les cores puae et puae2021
(GPL-2.0) ; les DAT d'identification (No-Intro, TOSEC, WHDLoad) sont téléchargés à la demande depuis leurs dépôts et
gardent leur propre licence. Aucune ROM, aucun Kickstart ni Workbench n'est distribué avec emu-rust.

0d. PC-DOS (4.0, en développement)
----------------------------------
Core : dosbox_pure (écran Cores). Jeux : un dossier par jeu ou un .zip dans un dossier dos/ de la bibliothèque ; les
dossiers sont copiés dans le cache (cache/dosbox_pure/, supprimable : Réglages > Vider le cache DOS), l'original reste
intact. Aucun jeu ni système DOS commercial n'est fourni ou téléchargé.
Musique MIDI (facultative) : SoundFont GeneralUser GS 2.0.3, téléchargé **seulement sur votre action** (Réglages DOS ou
écran Cores) depuis https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/684543d5e5efaef08d02be50dcda8d552478fa60/GeneralUser-GS.sf2
(HTTPS, SHA-256 vérifié), rangé dans system/GeneralUser-GS.sf2. Licence : « GeneralUser GS License v2.0 » (S. Christian
Collins ; usage libre, redistribution et modification permises ; pas une licence OSI), https://github.com/mrbumpy409/GeneralUser-GS .
Il n'est ni empaqueté ni redistribué avec emu-rust. MT-32 / CM-32L : ROM Roland commerciales, à importer vous-même.
Licences non embarquées dans le binaire : dosbox_pure (GPL-2.0) est téléchargé depuis le buildbot libretro et garde sa
licence ; le DAT d'identification DOS.dat (libretro-database, dépôt sous CC-BY-SA-4.0 ; source
https://github.com/robloach/libretro-database-dos, sans licence propre) est téléchargé à la demande et n'est pas
redistribué. Les données de THIRD-PARTY-DATA.txt (section « DOS » de System.dat : empreintes MT-32) sont inchangées.

0e. Amstrad CPC et Commodore 64 (5.0, en développement)
-------------------------------------------------------
Cores (écran Cores, téléchargés depuis le buildbot libretro, HTTPS, vérifiés) : cap32 (Amstrad CPC / GX4000, GPL-2.0) ;
vice_x64 (C64, GPL-2.0, par défaut) et vice_x64sc (C64, plus précis mais environ 5 fois plus lent : à réserver aux démos
et jeux à effets de raster ; installable depuis l'écran Cores).
ROM système : intégrées aux cores, jamais extraites, copiées ni téléchargées par emu-rust ; aucun fichier à fournir
pour démarrer (l'écran Cores indique « intégrées au core »).
  CPC : "Amstrad has kindly given its permission for its copyrighted material to be redistributed but Amstrad retains
  its copyright." (une partie du code est (c) Locomotive Software, CP/M (c) Digital Research ; ROM fournies par cap32).
  C64 : KERNAL, BASIC, CHARGEN et DOS 1541 (c) Commodore Business Machines, fournis par le core VICE ; statut de licence
  incertain (aucune permission connue), utilisées ici pour un usage personnel.
JiffyDOS (commercial, facultatif) : jamais fourni. Si vous le possédez, importez-le (écran Cores, « Importer un
BIOS... ») : l'application le reconnaît par son empreinte et l'installe dans system/vice/.
Contenus : CPC .dsk .cdt .voc .sna .cpr .zip .m3u ; C64 .d64 .d71 .d81 .g64 .nib .t64 .tap .prg .p00 .crt .zip .m3u.
Les écritures sur disquette se font sur des copies de travail (l'original reste intact). Cassettes : menu Cassette
(Lecture, Arrêt, Rembobiner) ; le chargement est accéléré automatiquement quand le core le permet.
Mode d'entrée par défaut : Joystick (CPC et C64) ; Ctrl droit + J bascule en mode Clavier. Touche hôte : Ctrl droit.
Bases d'identification (non embarquées, téléchargées à la demande depuis libretro-database et gardant leur licence) :
CPC = clean-cpc-db (MIT) puis TOSEC ; C64 = No-Intro. Aucune ROM, disquette ni cassette commerciale n'est distribuée.
Programmes de test libres (outils de développement seulement) : rpg-adventure (CPC, MIT), uberswap et Kidelyneen
(C64, licences du dépôt d'origine), Wild Snake Boa (C64, licence du dépôt d'origine).

Game Boy Advance (mgba, 6.0) : fichiers .gba (ou .zip). Le BIOS gba_bios.bin (dossier system/) est FACULTATIF : mgba en
  intègre un équivalent ; s'il est présent il est utilisé, s'il manque aucun avertissement. Sauvegardes .srm par jeu.
32X (picodrive, 6.0) : fichiers .32x (ou .bin reconnu par son en-tête). AUCUN BIOS à fournir (BIOS 32X intégrés au core).
Mega CD (6.0) : le BIOS dépend de la région du disque et n'est jamais téléchargé : à vous de le déposer (system/,
  bios_CD_E.bin / bios_CD_U.bin / bios_CD_J.bin). Format conseillé : .chd ; .cue avec toutes ses pistes, .m3u (multi-disques).
Licences non embarquées (rien de plus dans THIRD-PARTY-DATA.txt) : mgba MPL-2.0 ; picodrive et genesis_plus_gx : licences
  non commerciales de leurs auteurs (cores téléchargés à la demande) ; DAT Redump téléchargé à la demande, il garde sa licence.
Données de test libres (outils de développement seulement, jamais distribuées) : gba-tests de jsmolka (MIT), 240p Test Suite
  mini et Sega CD (GPL-2.0), hexgl32x (MIT).

PlayStation et 3DO (7.0, en développement) : rendu logiciel seulement (pas d'OpenGL ni Vulkan).
  Cores (écran Cores, buildbot libretro, HTTPS, vérifiés) : PlayStation = pcsx_rearmed (GPL-2.0, par défaut, rapide),
  swanstation (GPL-3.0, rendu logiciel) et mednafen_psx / Beetle PSX (GPL-2.0, précis, plus lent), au choix dans
  « Core utilisé » ; 3DO = opera (licence LGPL modifiée, usage NON COMMERCIAL ; téléchargé à la demande, jamais redistribué).
  Contenus : PlayStation .cue (+ pistes) .chd .iso .pbp .m3u ; 3DO .cue .chd .iso. Rangez vos CD dans roms/psx/ et roms/3do/
  (un titre existe souvent sur plusieurs machines : un .chd hors dossier peut rester « non classé »). Pas de CD dans un zip.
  BIOS FACULTATIFS, fournis par vous, JAMAIS téléchargés, vérifiés par empreinte (écran Cores, « Importer un BIOS... »,
  dossier system/) : scph5500.bin (Japon), scph5501.bin (USA), scph5502.bin (Europe) pour la PlayStation ; panafz10.bin pour
  la 3DO. S'ils sont présents, ils sont préférés ; sinon le core utilise son remplaçant intégré, avec un avertissement
  unique et une compatibilité réduite : HLE de pcsx_rearmed (GPL-2.0), OpenBIOS (swanstation, mednafen_psx ; licence
  MIT, pcsx-redux/nugget), Opera Boot (core opera, GPL-2.0-only). Rien n'est extrait ni copié par emu-rust.
  Cartes mémoire : une par jeu (saves/<core>/<jeu>.srm), reprise automatique d'un core PlayStation à l'autre.
  Manette : pad numérique par défaut ; mode DualShock (sticks) réglable par jeu.
Licences non embarquées (rien de plus dans THIRD-PARTY-DATA.txt que les empreintes de BIOS) : DAT Redump téléchargé à la
  demande, il garde sa licence. Données de test libres (outils de développement seulement, jamais distribuées) :
  ps1-tests de JaCzekanski (MIT), 240p Test Suite PS1 (GPL-2.0). Aucun jeu ni BIOS commercial.

1. Alternative manuelle : télécharger un core (buildbot libretro officiel, puis dézipper)
------------------------------------------------------------------
Mega Drive (genesis_plus_gx) :
  Linux   : https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip
  macOS   : https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/genesis_plus_gx_libretro.dylib.zip
  Windows : https://buildbot.libretro.com/nightly/windows/x86_64/latest/genesis_plus_gx_libretro.dll.zip
SNES : même chemin avec snes9x_libretro.{so,dylib,dll}.zip
Game Boy / Color : gambatte_libretro.*    NES : mesen_libretro.*    Master System / Game Gear : genesis_plus_gx_libretro.*

ROM de test libre (240p Test Suite 1.21, GPL-2.0 ; le .zip se charge directement) :
  https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download
ROM de test libre SNES (240p Test Suite SNES 1.02, GPL-2.0, même auteur) :
  https://sourceforge.net/projects/testsuite240p/files/OldFiles/SNES_SFC/240pSuite-SNES-1.02.zip/download

ROM de test libres (240p Test Suite mini, GPL-2.0, v0.23) :
  Game Boy : https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/gb240p.gb
  NES      : https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/240pee.nes
  Master System : https://github.com/haroldo-ok/sub-rescue/releases/download/v0.3/sub_rescue-0.3.zip (Sub Rescue, MIT)
  Game Gear     : https://github.com/joyrider3774/waternet/releases/download/v1.0/Waternet.Sega.Game.Gear.zip (Waternet, MIT)

Mega CD / Sega CD (genesis_plus_gx, 2.1) : disques .cue + .bin/.iso (ou .chd, .m3u). Le BIOS du Mega CD est commercial :
  il n'est jamais téléchargé, vous le déposez vous-même dans le dossier system/ (Réglages : dossier de données) sous le
  nom bios_CD_U.bin (USA, et région inconnue), bios_CD_E.bin (Europe) ou bios_CD_J.bin (Japon) ; le core choisit celui
  de la région du disque, l'application vérifie son empreinte avant le lancement.
Disque de test libre (240p Test Suite Sega CD 1.21, GPL-2.0, même auteur ; zip de 5,5 Mo, version USA .cue + .iso + 2 .wav) :
  https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-SegaCD-1.21.zip/download
  (à dézipper : le .cue référence les autres fichiers par leur nom)

Le core et la ROM peuvent être n'importe où : on passe leur chemin en ligne de commande.

2. Lancer (ligne de commande)
-----------------------------
  Linux   : ./emu-rust --core ./genesis_plus_gx_libretro.so --rom ./240pSuite-GenesisMD-1.21.zip
  macOS   : ./emu-rust.app/Contents/MacOS/emu-rust --core ./genesis_plus_gx_libretro.dylib --rom ./240pSuite-GenesisMD-1.21.zip
  Windows : .\emu-rust-cli.exe --core .\genesis_plus_gx_libretro.dll --rom .\240pSuite-GenesisMD-1.21.zip
            (emu-rust.exe accepte les mêmes options mais n'affiche rien dans le terminal : voir plus haut)
  Aide    : emu-rust --help        Version : emu-rust --version        (Windows : emu-rust-cli.exe)

Données (config.toml, saves/, states/) : --data-dir DIR, sinon EMU_DATA_DIR, sinon :
  Linux ~/.local/share/emu-rust   macOS ~/Library/Application Support/emu-rust   Windows %APPDATA%\emu-rust\data

3. Touches par défaut (modifiables : menu Échap > Touches)
----------------------------------------------------------
  Directions main gauche : W A S D (touches physiques : Z Q S D sur un clavier AZERTY) ; les flèches marchent aussi.
  Boutons main droite : rangée du bas J K L, rangée du haut U I O.   Entrée = Start    Maj droite = Mode (Mega Drive) / Select
  Mega Drive (genesis_plus_gx, picodrive) : J / K / L = A / B / C    U / I / O = X / Y / Z (manette 6 boutons)
  Super Nintendo (snes9x, bsnes, mesen-s) : K = A (valider)    J = B    I = X    U = Y    O = L    L = R
  Master System / Game Gear : J = bouton 1    K = bouton 2    Entrée = Pause (SMS) / Start (GG)
  Game Boy / NES : K = A    J = B    Entrée = Start    Maj droite = Select
  Autre core : disposition SNES + E / R = L2 / R2    F / G = L3 / R3
  Échap = menu (pause)    Espace maintenu = avance rapide    P = pause    F5 = reset
  F2 / F4 = sauvegarder / charger l'état    F6 / F7 = emplacement -/+    F11 = plein écran    F3 = stats
  Sans touches F (portable où elles exigent Fn) : Ctrl+S / Ctrl+L = sauvegarder / charger l'état    Ctrl+, / Ctrl+. =
  emplacement -/+    Ctrl+F ou Alt+Entrée = plein écran    Ctrl+R = reset    Ctrl+I = stats
  Les touches enregistrées dans config.toml restent prioritaires sur ces dispositions.

4. Avertissements système (binaires non signés)
-----------------------------------------------
macOS (Gatekeeper) : après dézippage,
    xattr -dr com.apple.quarantine emu-rust.app
  (ou clic droit > Ouvrir, une fois). Même chose pour une .dylib téléchargée au navigateur :
    xattr -d com.apple.quarantine genesis_plus_gx_libretro.dylib
  Le double-clic sur l'app (sans argument) ouvre la bibliothèque.
  macOS 10.13 minimum (Intel) ; Mac d'essai : macOS 12.7.6 (Monterey).
Windows (SmartScreen) : « Informations complémentaires » > « Exécuter quand même ».
  Le core doit être une DLL 64 bits (x86_64).
Linux : dépendances (Debian/Ubuntu) :
    sudo apt install libasound2t64 libasound2-plugins libxkbcommon-x11-0 libgl1 libegl1 \
      libwayland-client0 libxcursor1 libxrandr2 libxi6 libudev1        (Ubuntu 22.04 : libasound2)
  Son sous WSLg : ~/.asoundrc contenant
    pcm.!default pulse
    ctl.!default pulse

Licence : MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).
Licences des dépendances : THIRD-PARTY-LICENSES.html.
Données embarquées (empreintes de BIOS, libretro-database, CC-BY-SA-4.0 ; base de manettes
SDL_GameControllerDB, zlib) : THIRD-PARTY-DATA.txt.
