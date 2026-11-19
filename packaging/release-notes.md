## Télécharger

| Plateforme | Archive |
|---|---|
| Linux x86_64 (glibc ≥ 2.35) | `emu-rust-@VERSION@-linux-x86_64.tar.gz` |
| macOS Intel (≥ 10.13, Mac d'essai : 12.7.6 Monterey ; non notarisé) | `emu-rust-@VERSION@-macos-x86_64.zip` |
| Windows x86_64 | `emu-rust-@VERSION@-windows-x86_64.zip` |
| Docker (amd64) | `docker pull ghcr.io/jerhome/emu-rust:@VERSION@` |

Vérifier : `sha256sum -c SHA256SUMS.txt --ignore-missing`. Aucun core, BIOS ni ROM n'est fourni.

## Core et ROM de test

- ROM libre : [240p Test Suite 1.21 (Mega Drive, GPL-2.0)](https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download)
  — le `.zip` se charge directement.
- ROM libre : [240p Test Suite SNES 1.02 (Super Nintendo, GPL-2.0)](https://sourceforge.net/projects/testsuite240p/files/OldFiles/SNES_SFC/240pSuite-SNES-1.02.zip/download) — avec le core `snes9x` ci-dessous.
- Core `genesis_plus_gx` (buildbot libretro, à dézipper) :
  [Linux](https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip) ·
  [macOS Intel](https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/genesis_plus_gx_libretro.dylib.zip) ·
  [Windows](https://buildbot.libretro.com/nightly/windows/x86_64/latest/genesis_plus_gx_libretro.dll.zip).
  SNES : même chemin avec `snes9x_libretro.{so,dylib,dll}.zip`.

## Avant de tester

| Plateforme | |
|---|---|
| Docker | `EMU_IMAGE=ghcr.io/jerhome/emu-rust:@VERSION@ docker compose -f docker/compose.yaml up --no-build emu-wslg` (noVNC : `emu-novnc`, sans son, <http://localhost:6080/vnc.html>) ; core + ROM copiés dans le volume (`docker/README.md`) |
| Linux (WSL2) | `tar xzf`, puis `sudo apt install libasound2t64 libasound2-plugins libxkbcommon-x11-0 libgl1 libegl1 libwayland-client0 libxcursor1 libxrandr2 libxi6` (22.04 : `libasound2`) ; son WSLg : `~/.asoundrc` = `pcm.!default pulse` / `ctl.!default pulse` |
| macOS Intel | dézipper, `xattr -dr com.apple.quarantine emu-rust.app` (ou clic droit → Ouvrir, une fois), idem pour la `.dylib` ; `./emu-rust.app/Contents/MacOS/emu-rust --core … --rom …` |
| Windows | dézipper ; PowerShell : `.\emu-rust.exe --core .\genesis_plus_gx_libretro.dll --rom .\240pSuite-GenesisMD-1.21.zip` ; SmartScreen : « Informations complémentaires → Exécuter quand même » |

## Checklist de test (chaque plateforme)

- [ ] `emu-rust --version` affiche `@VERSION@`.
- [ ] Lancement avec core + `240pSuite-GenesisMD-1.21.zip` : la fenêtre s'ouvre, pas d'erreur dans le terminal.
- [ ] Image : menu net, mise à l'échelle entière ; « Color Bars » sans couleurs inversées ; redimensionnement, F11 plein écran (HiDPI : image pas au quart de la fenêtre).
- [ ] Son : « Sound Test » audible, sans craquements ; Espace maintenu = avance rapide muette.
- [ ] Clavier : flèches, Entrée = Start, Z / X / C = A / B / C.
- [ ] Menu Échap : pause (son coupé), Reprendre, Touches (changer une touche, relancer : conservée), Quitter.
- [ ] Savestate : F2 sauvegarde, changer d'écran, F4 recharge ; F6/F7 change d'emplacement ; message OSD.
- [ ] SRAM (facultatif, jeu à pile) : sauvegarder en jeu, quitter, relancer → `saves/<core>/<jeu>.srm` relu.
- [ ] Relancer : `config.toml` présent (Unix : mode `600`).

Limites connues : lancement en ligne de commande uniquement, pas de manette, binaires non signés.
