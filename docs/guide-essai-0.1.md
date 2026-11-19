# Guide d'essai de la 0.1

À relire au retour. Les builds sont sur la page des releases :
https://github.com/jerhome/emu-rust/releases (pré-release `v0.1.0`).

## 1. Cores à télécharger (buildbot libretro, zip à extraire)

| | Mega Drive (genesis_plus_gx) | SNES (snes9x) |
|---|---|---|
| Linux / WSL2 | https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip | https://buildbot.libretro.com/nightly/linux/x86_64/latest/snes9x_libretro.so.zip |
| macOS Intel | https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/genesis_plus_gx_libretro.dylib.zip | https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/snes9x_libretro.dylib.zip |
| Windows | https://buildbot.libretro.com/nightly/windows/x86_64/latest/genesis_plus_gx_libretro.dll.zip | https://buildbot.libretro.com/nightly/windows/x86_64/latest/snes9x_libretro.dll.zip |

## 2. ROM de test libres (GPL, le `.zip` se charge directement)

- Mega Drive : https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download
- SNES : https://sourceforge.net/projects/testsuite240p/files/OldFiles/SNES_SFC/240pSuite-SNES-1.02.zip/download

## 3. Où les placer, comment lancer

Pas d'emplacement imposé en 0.1 : le plus simple est de mettre le core (dézippé) et la ROM (`.zip`) **à côté de
l'émulateur**, dans le dossier extrait de la release, puis de passer leurs chemins.

- **Linux (WSL2, affichage et son via WSLg)**
  ```bash
  ./emu-rust --core ./snes9x_libretro.so --rom ./240pSuite-SNES-1.02.zip
  ```
- **macOS 12.7.6** (première fois : clic droit sur `emu-rust.app` → Ouvrir, pour Gatekeeper)
  ```bash
  ./emu-rust.app/Contents/MacOS/emu-rust --core ./snes9x_libretro.dylib --rom ./240pSuite-SNES-1.02.zip
  ```
- **Windows** (PowerShell ; SmartScreen : « Informations complémentaires » → « Exécuter quand même »)
  ```powershell
  .\emu-rust.exe --core .\snes9x_libretro.dll --rom .\240pSuite-SNES-1.02.zip
  ```
- **Docker** : core Linux (`.so`) dans `cores/`, ROM dans `roms/` du volume `/data` (copie : voir `docker/README.md`).
  ```bash
  docker compose -f docker/compose.yaml --profile wslg run --rm emu-wslg \
    --core /data/cores/snes9x_libretro.so --rom /data/roms/240pSuite-SNES-1.02.zip
  ```
  Image : `ghcr.io/jerhome/emu-rust:0.1.0`. **À faire une fois** : rendre le paquet public sur
  https://github.com/users/jerhome/packages/container/emu-rust/settings (Danger Zone → Change visibility → Public).

Mega Drive : pareil avec `genesis_plus_gx_libretro.*` et `240pSuite-GenesisMD-1.21.zip`.

Sauvegardes, savestates et `config.toml` sont créés dans le dossier de données utilisateur ; son chemin est affiché
au démarrage (ligne « racine des données »).

## 4. Touches

Flèches = croix ; Entrée = Start ; Échap = menu.

| Touche | Mega Drive | SNES |
|---|---|---|
| Z | A (valider) | B (retour) |
| X | B | A (valider) |
| C | C | — |
| A / S / D | X / Y / Z | Y / X / — |
| Q / W | — | L / R |
| Maj droite | Mode | Select |

Raccourcis : P pause · H reset · F2 / F4 sauver / charger l'état · F6 / F7 emplacement · Espace (maintenu) avance
rapide · F11 plein écran · F3 statistiques.

## 5. Checklist (par plateforme : Docker, Linux/WSL2, macOS, Windows)

- [ ] La fenêtre s'ouvre, l'image est nette (4:3), y compris sur écran Retina / mise à l'échelle Windows.
- [ ] Le son sort, sans craquements (SNES à 32 kHz, Mega Drive).
- [ ] Clavier : naviguer dans le menu de la 240p Test Suite (valider / retour).
- [ ] Échap : menu en jeu ; navigation au clavier ; « Reprendre ».
- [ ] F2 puis F4 : savestate sauvé puis rechargé.
- [ ] Écran des touches : réassigner une touche, quitter, relancer → la touche est conservée (`config.toml` en 0600).
- [ ] Options du core : changer une option, l'effet est immédiat.
- [ ] (Facultatif) un jeu avec sauvegarde cartouche : SRAM conservée après redémarrage.

Noter les problèmes (plateforme, étapes, message affiché) : ils seront corrigés pour la 0.1.x stable.
