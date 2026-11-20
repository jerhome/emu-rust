emu-rust - frontend libretro (usage personnel)
==============================================

Aucun core, BIOS ni jeu n'est fourni. Vous fournissez le core libretro et la ROM.

0. Cores depuis l'application
-----------------------------
Sans argument, emu-rust ouvre la bibliothèque de jeux : indiquez le dossier de vos ROMs dans les Réglages, lancez
le scan, puis installez les cores depuis l'écran Cores, ou en ligne de commande :
  emu-rust --install-core recommended      (aussi : --list-cores, --update-cores)
Les cores sont téléchargés depuis le buildbot libretro (HTTPS), vérifiés, et rangés dans le dossier de données
(cores/). Les BIOS ne sont jamais téléchargés : l'écran Cores indique ceux qui manquent. Les jaquettes sont
désactivées par défaut (activation dans les Réglages).
La méthode manuelle ci-dessous reste possible : --core accepte un chemin.

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
  Windows : .\emu-rust.exe --core .\genesis_plus_gx_libretro.dll --rom .\240pSuite-GenesisMD-1.21.zip
  Aide    : emu-rust --help        Version : emu-rust --version

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
