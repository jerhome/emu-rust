## Télécharger

| Plateforme | Archive |
|---|---|
| Linux x86_64 (glibc ≥ 2.35) | `emu-rust-@VERSION@-linux-x86_64.tar.gz` |
| macOS Intel (≥ 10.13, Mac d'essai : 12.7.6 Monterey ; non notarisé) | `emu-rust-@VERSION@-macos-x86_64.zip` |
| Windows x86_64 | `emu-rust-@VERSION@-windows-x86_64.zip` |
| Docker (amd64) | `docker pull ghcr.io/jerhome/emu-rust:@VERSION@` |

Vérifier : `sha256sum -c SHA256SUMS.txt --ignore-missing`. Aucun core, BIOS ni ROM n'est fourni.

## Premier lancement

1. Lancer `emu-rust` sans argument : la bibliothèque s'ouvre.
2. Réglages : indiquer le dossier de vos ROMs (Docker : `/data/roms`), puis lancer le scan.
3. Écran Cores (ou `emu-rust --install-core recommended`) : installer les cores recommandés (vérifiés, jamais de BIOS téléchargé).
4. Jaquettes : désactivées par défaut ; à activer dans les Réglages si souhaité.

## ROM de test libres (une par système)

| Système | ROM | Licence |
|---|---|---|
| Mega Drive | [240p Test Suite 1.21](https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download) (`.zip`) | GPL-2.0 |
| Super Nintendo | [240p Test Suite SNES 1.02](https://sourceforge.net/projects/testsuite240p/files/OldFiles/SNES_SFC/240pSuite-SNES-1.02.zip/download) (`.zip`) | GPL-2.0 |
| Game Boy | [gb240p.gb (240p Test Suite mini 0.23)](https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/gb240p.gb) | GPL-2.0 |
| NES | [240pee.nes (240p Test Suite mini 0.23)](https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/240pee.nes) | GPL-2.0 |
| Master System | [Sub Rescue 0.3](https://github.com/haroldo-ok/sub-rescue/releases/download/v0.3/sub_rescue-0.3.zip) (`.zip`) | MIT |
| Game Gear | [Waternet 1.0](https://github.com/joyrider3774/waternet/releases/download/v1.0/Waternet.Sega.Game.Gear.zip) (`.zip`) | MIT |

## Cores à la main (alternative)

Buildbot libretro, à dézipper : `genesis_plus_gx` (Mega Drive, Master System, Game Gear)
[Linux](https://buildbot.libretro.com/nightly/linux/x86_64/latest/genesis_plus_gx_libretro.so.zip) ·
[macOS Intel](https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/genesis_plus_gx_libretro.dylib.zip) ·
[Windows](https://buildbot.libretro.com/nightly/windows/x86_64/latest/genesis_plus_gx_libretro.dll.zip).
Autres : même chemin avec `snes9x_libretro`, `gambatte_libretro` (Game Boy / Color), `mesen_libretro` (NES) et l'extension `.so.zip`, `.dylib.zip` ou `.dll.zip`.

## Avant de tester

| Plateforme | |
|---|---|
| Docker | `EMU_IMAGE=ghcr.io/jerhome/emu-rust:@VERSION@ docker compose -f docker/compose.yaml up --no-build emu-wslg` (noVNC : `emu-novnc`, sans son, <http://localhost:6080/vnc.html>) ; ROMs copiées dans `/data/roms` (`docker/README.md`) |
| Linux (WSL2) | `tar xzf`, puis `sudo apt install libasound2t64 libasound2-plugins libxkbcommon-x11-0 libgl1 libegl1 libwayland-client0 libxcursor1 libxrandr2 libxi6` (22.04 : `libasound2`) ; son WSLg : `~/.asoundrc` = `pcm.!default pulse` / `ctl.!default pulse` |
| macOS Intel | dézipper, `xattr -dr com.apple.quarantine emu-rust.app` (ou clic droit → Ouvrir, une fois) ; les cores installés par l'application ne sont pas en quarantaine |
| Windows | dézipper ; `.\emu-rust.exe` ; SmartScreen : « Informations complémentaires → Exécuter quand même » |

## Checklist de test (chaque plateforme)

- [ ] `emu-rust --version` affiche `@VERSION@` ; `emu-rust` sans argument ouvre la bibliothèque.
- [ ] Réglages : dossier des ROMs choisi, scan terminé, les jeux apparaissent (noms identifiés par le DAT si présent).
- [ ] Écran Cores : installation des cores recommandés réussie ; BIOS manquants signalés, non téléchargés.
- [ ] Lancer une ROM de test pour chacun des 6 systèmes : fenêtre à la bonne taille, image nette, son sans craquements.
- [ ] Clavier : flèches, X / Z, Entrée ; menu Échap (Reprendre, Touches, Quitter) ; profil adapté au système.
- [ ] Savestate F2 / F4 (Game Gear : restauré sans image, défaut connu du core).
- [ ] Retour du jeu vers la bibliothèque (Échap) ; favori, « récemment joués » mis à jour.
- [ ] Mises à jour des cores : `--list-cores`, retour arrière possible.
- [ ] (Facultatif) Jaquettes activées : elles s'affichent, la bibliothèque reste fluide ; hors ligne sans erreur bloquante.
- [ ] `config.toml` présent (Unix : mode `600`).

Limites connues : pas de manette, binaires non signés (voir le CHANGELOG).
