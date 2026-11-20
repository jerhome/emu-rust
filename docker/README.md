# Docker

Images : `dev` (toolchain, Xvfb) et `runtime` (build release, utilisateur `emu`, données dans `/data`). Voir ADR 0004.
Commandes à lancer depuis la racine du dépôt.

## Image publiée (GHCR)

Chaque tag `v*` publie l'image runtime (`linux/amd64`) sur `ghcr.io/jerhome/emu-rust` : `:<version>` (ex. `1.0.0`),
`:<majeur>.<mineur>` (`1.0`) et `:latest` (ADR 0012). `EMU_IMAGE` choisit l'image utilisée par `compose.yaml`
(défaut : `emu-rust:dev`, construite localement) :

```sh
export EMU_IMAGE=ghcr.io/jerhome/emu-rust:1.0.0
docker compose -f docker/compose.yaml --profile wslg pull
docker compose -f docker/compose.yaml run --rm --no-build emu-wslg --core /data/cores/genesis_plus_gx_libretro.so --rom /data/roms/240pSuite-GenesisMD-1.21.zip
```

Sans `EMU_IMAGE`, `--build` reconstruit l'image depuis les sources comme avant. Avec `EMU_IMAGE` et `--build`, l'image
locale construite porte le nom GHCR : préférer `--no-build`.

Premier push : GHCR crée le paquet en **privé**. Une seule fois, sur GitHub : profil → *Packages* → `emu-rust` →
*Package settings* → *Change visibility* → *Public* (sinon `docker login ghcr.io` avec un jeton `read:packages`).

## Cores depuis l'application

Sans argument, l'application ouvre la bibliothèque de jeux (l'entrypoint n'impose ni `--core` ni `--rom`). Les cores
s'installent depuis l'écran Cores ou en ligne de commande, dans `/data/cores` (HTTPS : l'image contient
`ca-certificates`) :

```sh
docker compose -f docker/compose.yaml run --rm --no-deps emu-wslg --install-core recommended
```

Dossier des ROMs à indiquer dans les Réglages de la bibliothèque : `/data/roms`. Les liens manuels de
`packaging/README.txt` et la copie dans le volume (ci-dessous) restent valables.

## Données dans `/data` (volume `emu-rust_emu-data`)

```text
/data/cores/    cores libretro (*.so)
/data/roms/     ROMs (jamais fournies par l'application ni l'image)
/data/system/   BIOS et fichiers système des cores
/data/saves/    SRAM : saves/<core_id>/<jeu>.srm (compatible RetroArch)
/data/states/   savestates à emplacements, par core puis par jeu
/data/assets/   réservé (non utilisé en 1.0 ; jaquettes dans covers/, DAT dans database/)
/data/covers/   jaquettes    /data/database/   DAT No-Intro / TOSEC
/data/game-options/   options par jeu : game-options/<core_id>/<jeu>.toml (2.0)
/data/library.sqlite   base de la bibliothèque (créée par l'application)
/data/config.toml   configuration (touches, options) ; droits 0600, propriétaire `emu`
```

Un seul volume porte tout le persistant ; le dossier de données est `EMU_DATA_DIR=/data` (équivalent de
`--data-dir /data`, voir ADR 0011). L'entrypoint crée `roms/ system/ saves/ states/ assets/ cores/ covers/ database/` s'ils manquent et remet `config.toml`
en `0600` (il pourra contenir des identifiants). Sauvegarde du volume :

```sh
docker run --rm -v emu-rust_emu-data:/data:ro -v "$PWD:/out" debian:bookworm-slim tar czf /out/emu-data.tgz -C /data saves states config.toml
```

Le compilateur C (`build-essential`) n'est présent que dans les étages de build (dev et build du runtime) :
`emu-libretro` compile une cale C via `cc` (ADR 0009) ; l'image finale n'en contient pas.

## Atari ST (2.0, en développement)

Core `hatari` (installé depuis l'écran Cores). Le BIOS de l'Atari ST est **EmuTOS 1.4** (GPL-2.0-or-later, libre) :
l'application le propose et le télécharge elle-même (HTTPS, SHA-256 vérifié) dans `/data/system/tos.img`. Aucun TOS
commercial n'est téléchargé ; si vous placez le vôtre dans `/data/system/tos.img`, il n'est jamais écrasé.
Disquettes : `.st`, `.msa`, `.stx`, `.ipf`, `.dim` ou `.m3u` (plusieurs disquettes) dans `/data/roms/` ; les écritures
se font sur des copies de travail (l'original n'est pas modifié).

Touche hôte : **Ctrl droit** (⌘ droit sur macOS). Seule, elle ouvre le menu ; combinée :
`S`/`L` sauvegarder/charger l'état, `,`/`.` emplacement -/+, `R` reset, `P` pause, `F` plein écran, `I` stats,
`Espace` avance rapide, `K` clavier virtuel, `M` capture de la souris, `J` mode clavier/joystick, `D` disquette suivante.

Souris : le bureau GEM l'exige. Elle est capturée au premier clic dans la fenêtre ; libération par la touche hôte
(menu) ou Hôte + M. Sous noVNC et WSLg la capture n'est pas disponible (pas de mouvement relatif) : la souris reste
libre, utilisez le clavier virtuel et le joystick. Disquettes : menu Disquettes, ou Hôte + D pour passer à la suivante.

## Raccourcis (à venir, phase 2)

Sauvegarder/charger un savestate, changer d'emplacement, avance rapide, menu et écran des touches : la liste exacte
et les touches par défaut seront documentées ici quand l'interface sera livrée (voir `docs/design/phase2-frontend.md`).
Les touches sont éditables dans l'interface et persistées dans `config.toml`.

Copier des fichiers dans le volume (ex. depuis WSL) :

```sh
docker compose -f docker/compose.yaml run --rm --no-deps -v "$PWD/mes-fichiers:/in:ro" --entrypoint bash emu-wslg \
  -c 'mkdir -p /data/cores /data/roms && cp /in/*_libretro.so /data/cores/ && cp /in/*.bin /data/roms/'
```

Alternative : monter un dossier de ROMs en lecture seule (`-v /chemin/roms:/data/roms:ro`).

## Tests utilisateur

```sh
docker compose -f docker/compose.yaml --profile wslg build
# diagnostic son seul (bip gauche/droite sous WSLg) :
docker compose -f docker/compose.yaml run --rm --entrypoint /usr/local/bin/sound-check emu-wslg
# jeu :
docker compose -f docker/compose.yaml run --rm emu-wslg --core /data/cores/genesis_plus_gx_libretro.so --rom /data/roms/240pSuite-1.21.bin
# noVNC (sans son ; --no-audio ajouté par l'entrypoint) puis http://localhost:6080/vnc.html :
docker compose -f docker/compose.yaml --profile novnc up --build emu-novnc
```

Si le bip est absent : vérifier que `/mnt/wslg/PulseServer` existe dans WSL (`ls /mnt/wslg`), puis `wsl --shutdown`.
