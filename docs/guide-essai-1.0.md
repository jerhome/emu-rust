# Guide d'essai de la 1.0

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v1.0.0-rc.2`). Image Docker :
`ghcr.io/jerhome/emu-rust:1.0.0-rc.2` (paquet à rendre public une fois : voir `docker/README.md`).

## 1. Principe

Plus besoin de télécharger les cores à la main : l'application les installe. Il suffit de fournir des ROMs.

## 2. ROM de test libres (une par système)

- Mega Drive : https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-GenesisMD-1.21.zip/download
- SNES : https://sourceforge.net/projects/testsuite240p/files/OldFiles/SNES_SFC/240pSuite-SNES-1.02.zip/download
- Game Boy : https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/gb240p.gb
- NES : https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/240pee.nes
- Master System : https://github.com/haroldo-ok/sub-rescue/releases/download/v0.3/sub_rescue-0.3.zip
- Game Gear : https://github.com/joyrider3774/waternet/releases/download/v1.0/Waternet.Sega.Game.Gear.zip

Mettre tous les fichiers (les `.zip` se chargent directement) dans un même dossier `roms`.

## 3. Lancer, par plateforme

Dans chaque cas, premier lancement : Réglages → dossier des ROMs → scan ; écran Cores → installer les cores
recommandés (ou `--install-core recommended` en ligne de commande).

- **Linux (WSL2)** : `./emu-rust` (dépendances et son : `packaging/README.txt`).
- **macOS 12.7.6** : `xattr -dr com.apple.quarantine emu-rust.app`, puis double-clic ou
  `./emu-rust.app/Contents/MacOS/emu-rust`.
- **Windows** (SmartScreen : « Informations complémentaires » → « Exécuter quand même ») : `.\emu-rust.exe`.
- **Docker** : copier les ROMs dans `/data/roms` du volume (`docker/README.md`), puis
  ```bash
  export EMU_IMAGE=ghcr.io/jerhome/emu-rust:1.0.0-rc.2
  docker compose -f docker/compose.yaml run --rm --no-build emu-wslg --install-core recommended
  docker compose -f docker/compose.yaml run --rm --no-build emu-wslg
  ```
  Dans les Réglages, dossier des ROMs : `/data/roms`.

Les données (config, base, cores, sauvegardes) sont dans le dossier de données ; son chemin est affiché au démarrage.

## 4. Touches

Directions : **W A S D** (touches physiques : Z Q S D sur un clavier AZERTY) ; les flèches marchent aussi.
Entrée = Start ; Échap = menu. Boutons : rangée du bas J K L, rangée du haut U I O.

| Touche | Mega Drive | SNES | Game Boy / NES | SMS / Game Gear |
|---|---|---|---|---|
| J | A | B | B | bouton 1 |
| K | B | A (valider) | A (valider) | bouton 2 |
| L | C | R | — | — |
| U | X | Y | — | — |
| I | Y | X | — | — |
| O | Z | L | — | — |
| Entrée | Start | Start | Start | Pause (SMS) / Start (GG) |
| Maj droite | Mode | Select | Select | — |

Raccourcis : P pause · F5 reset · F2 / F4 état · F6 / F7 emplacement · Espace avance rapide · F11 plein écran ·
F3 statistiques. Sans touches F (portable où elles exigent Fn) : Ctrl+S / Ctrl+L état · Ctrl+, / Ctrl+. emplacement ·
Ctrl+F ou Alt+Entrée plein écran · Ctrl+R reset · Ctrl+I statistiques. Clavier 60 % : aucune touche F ni flèche
n'est nécessaire (Tab / Entrée / Échap dans les menus). Les touches déjà enregistrées dans `config.toml` restent prioritaires ; pour
adopter la nouvelle disposition, supprimer la section `[input.keyboard.…]` concernée.

## 5. Checklist (par plateforme : Docker, Linux/WSL2, macOS, Windows)

- [ ] `--version` = 1.0.0-rc.2 ; `emu-rust` sans argument ouvre la bibliothèque.
- [ ] Scan du dossier de ROMs : les 6 jeux apparaissent.
- [ ] Écran Cores : installation des cores recommandés ; BIOS manquants signalés (jamais téléchargés).
- [ ] Chaque système : le jeu se lance, fenêtre à la bonne taille, image nette (HiDPI / Retina), son propre.
- [ ] Clavier : bonne disposition par système (tableau ci-dessus) ; réassigner une touche, relancer : conservée.
- [ ] F2 / F4 : savestate (Game Gear : restauré sans image, défaut connu du core).
- [ ] Échap → retour à la bibliothèque ; favori ; « récemment joués ».
- [ ] `--list-cores`, mise à jour, retour arrière d'un core.
- [ ] (Facultatif) jaquettes : les activer dans les Réglages ; test hors ligne.
- [ ] `config.toml` en `600` (Unix).

Limites connues : pas de manette ; retour jeu → bibliothèque non testé automatiquement ; JPEG > ~2700² refusés ;
macOS non notarisé, SmartScreen. Noter les problèmes (plateforme, étapes, message) pour la 1.0.x.
