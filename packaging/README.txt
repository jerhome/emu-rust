emu-rust - frontend libretro (usage personnel)
==============================================

Aucun core, BIOS ni jeu n'est fourni. Vous fournissez le core libretro et la ROM.

1. Télécharger un core (buildbot libretro officiel, puis dézipper)
------------------------------------------------------------------
Mega Drive (genesis_plus_gx) :
  Linux   : https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip
  macOS   : https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/genesis_plus_gx_libretro.dylib.zip
  Windows : https://buildbot.libretro.com/nightly/windows/x86_64/latest/genesis_plus_gx_libretro.dll.zip
SNES : même chemin avec snes9x_libretro.{so,dylib,dll}.zip

ROM de test libre (240p Test Suite 1.21, GPL-2.0 ; le .zip se charge directement) :
  https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download
ROM de test libre SNES (240p Test Suite SNES 1.02, GPL-2.0, même auteur) :
  https://sourceforge.net/projects/testsuite240p/files/OldFiles/SNES_SFC/240pSuite-SNES-1.02.zip/download

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
  Flèches = croix directionnelle    Entrée = Start    Maj droite = Mode (Mega Drive) / Select (SNES)
  Mega Drive (genesis_plus_gx, picodrive) : Z / X / C = A / B / C    A / S / D = X / Y / Z (manette 6 boutons)
  Super Nintendo (snes9x, bsnes, mesen-s) : X = A (valider)    Z = B    S = X    A = Y    Q = L    W = R
  Autre core : disposition SNES + E / R = L2 / R2    F / G = L3 / R3
  Échap = menu (pause)    Espace maintenu = avance rapide    P = pause    H = reset
  F2 / F4 = sauvegarder / charger l'état    F6 / F7 = emplacement -/+    F11 = plein écran    F3 = stats

4. Avertissements système (binaires non signés)
-----------------------------------------------
macOS (Gatekeeper) : après dézippage,
    xattr -dr com.apple.quarantine emu-rust.app
  (ou clic droit > Ouvrir, une fois). Même chose pour une .dylib téléchargée au navigateur :
    xattr -d com.apple.quarantine genesis_plus_gx_libretro.dylib
  Le double-clic sur l'app sans arguments ne fait rien en 0.1.
  macOS 10.13 minimum (Intel) ; Mac d'essai : macOS 12.7.6 (Monterey).
Windows (SmartScreen) : « Informations complémentaires » > « Exécuter quand même ».
  Le core doit être une DLL 64 bits (x86_64).
Linux : dépendances (Debian/Ubuntu) :
    sudo apt install libasound2t64 libasound2-plugins libxkbcommon-x11-0 libgl1 libegl1 \
      libwayland-client0 libxcursor1 libxrandr2 libxi6        (Ubuntu 22.04 : libasound2)
  Son sous WSLg : ~/.asoundrc contenant
    pcm.!default pulse
    ctl.!default pulse

Licence : MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).
Licences des dépendances : THIRD-PARTY-LICENSES.html.
