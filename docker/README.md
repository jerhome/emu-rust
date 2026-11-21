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
/data/cache/    copies de jeux DOS en dossier : cache/dosbox_pure/<jeu>.zip (4.0, supprimable)
/data/library.sqlite   base de la bibliothèque (créée par l'application)
/data/config.toml   configuration (touches, options) ; droits 0600, propriétaire `emu`
```

Un seul volume porte tout le persistant ; le dossier de données est `EMU_DATA_DIR=/data` (équivalent de
`--data-dir /data`, voir ADR 0011). L'entrypoint crée `roms/ system/ saves/ states/ assets/ cores/ covers/ database/ game-options/ cache/` s'ils manquent et remet `config.toml`
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

Touche hôte : **Ctrl droit** partout, macOS compris (⌘ droit réglable en repli si le clavier n'a pas de Ctrl droit). Seule, elle ouvre le menu ; combinée :
`S`/`L` sauvegarder/charger l'état, `,`/`.` emplacement -/+, `R` reset, `P` pause, `F` plein écran, `I` stats,
`Espace` avance rapide, `K` clavier virtuel, `M` capture de la souris, `J` mode clavier/joystick, `D` disquette suivante.

Souris : le bureau GEM l'exige. Elle est capturée au premier clic dans la fenêtre ; libération par la touche hôte
(menu) ou Hôte + M. Sous noVNC et WSLg la capture n'est pas disponible (pas de mouvement relatif) : la souris reste
libre, utilisez le clavier virtuel et le joystick. Disquettes : menu Disquettes, ou Hôte + D pour passer à la suivante.

## Mega CD (2.1)

Core `genesis_plus_gx` (le même que la Mega Drive). Le BIOS n'est jamais fourni : déposez `bios_CD_E.bin`,
`bios_CD_U.bin` ou `bios_CD_J.bin` (noms exacts) dans le volume, dossier `/data/system/`. Disques (`.cue` avec toutes
ses pistes, ou `.chd`) dans `/data/roms/megacd/<jeu>/`. La sauvegarde du jeu est dans `/data/saves/`.

```sh
docker compose -f docker/compose.yaml run --rm --no-deps -v "$PWD/bios:/in:ro" --entrypoint bash emu-wslg \
  -c 'mkdir -p /data/system && cp /in/bios_CD_*.bin /data/system/'
```

## Amiga (3.0, en développement)

Cores `puae` (par défaut) et `puae2021` (plus léger), installés depuis l'écran Cores. Disquettes `.adf`, `.adz`, `.dms`,
`.ipf`, `.fdi`, disques `.hdf`, archives WHDLoad `.lha` ou `.m3u` dans `/data/roms/amiga/` ; les écritures se font sur
des copies de travail (`/data/saves/puae/<jeu>.disks/`), l'original n'est pas modifié.

**Kickstart** : jamais fourni ni téléchargé. Sans lui, le core utilise **AROS** (ROM libre intégrée au core, licence
AROS Public License 1.1 ; compatibilité limitée). Pour un Kickstart à vous, copiez-le dans le volume sous le nom exact
attendu, ou utilisez « Importer un BIOS... » (navigateur de fichiers intégré : pas de boîte de dialogue native dans le
conteneur) après l'avoir déposé dans un dossier visible du conteneur :

```sh
docker compose -f docker/compose.yaml run --rm --no-deps -v "$PWD/bios:/in:ro" --entrypoint bash emu-wslg \
  -c 'mkdir -p /data/system && cp /in/kick*.A* /data/system/'
```

**Dossier des ROMs de l'hôte** : dans le conteneur (`EMU_RUST_CONTAINER=1`, posé par l'entrypoint), le bouton « Ouvrir le
dossier des ROMs » affiche le chemin `/data/roms` au lieu d'ouvrir un explorateur. Pour déposer les jeux directement
depuis l'hôte, décommentez dans `compose.yaml`, pour le service utilisé, la ligne `${EMU_HOST_ROMS_DIR}:/data/roms` et la
ligne `EMU_HOST_ROMS_DIR` de `environment`, puis :

```sh
EMU_HOST_ROMS_DIR=/home/moi/roms docker compose -f docker/compose.yaml run --rm emu-wslg
```

Depuis WSL, choisissez un chemin du système de fichiers WSL (`/home/...`), bien plus rapide que `/mnt/c/...`. Le bouton
affiche alors ce chemin d'hôte. Notices : les licences des cores (GPL-2.0) et d'AROS sont celles des cores ; aucune
ROM ni Kickstart n'est dans l'image.

## PC-DOS (4.0, en développement)

Core `dosbox_pure` (GPL-2.0, installé depuis l'écran Cores, buildbot libretro). Aucun jeu, BIOS ni système DOS
commercial n'est fourni. Rangez chaque jeu **en dossier** (`/data/roms/dos/<jeu>/` contenant ses `.exe`) ou **en `.zip`**
dans `/data/roms/dos/` (les `.exe` posés directement dans `roms/dos/` sont ignorés) ; images de disquette `.ima`/`.img`
et CD `.cue`/`.iso` acceptés. Un dossier est d'abord copié dans `/data/cache/dosbox_pure/` (« Préparation du jeu... ») :
ce cache se supprime sans perte (Réglages > Vider le cache DOS). Le dossier d'origine n'est jamais modifié ; les
modifications d'un jeu zip vont dans `/data/saves/dosbox_pure/<jeu>.pure.zip`.

**Musique MIDI** : facultative. Le SoundFont libre **GeneralUser GS 2.0.3** (32 Mo, « GeneralUser GS License v2.0 ») est
téléchargé **uniquement sur votre action** (Réglages DOS > MIDI, ou écran Cores ; HTTPS depuis `raw.githubusercontent.com`,
URL épinglée au commit `684543d` de `mrbumpy409/GeneralUser-GS`, SHA-256 vérifié) dans `/data/system/GeneralUser-GS.sf2`.
Il n'est jamais dans l'image. MT-32 / CM-32L : ROM Roland commerciales, jamais téléchargées ; importez les vôtres
(`/data/system/MT32_CONTROL.ROM`, `MT32_PCM.ROM`, ...). Le DAT `DOS.dat` (libretro-database, CC-BY-SA-4.0) est téléchargé
à la demande (Réglages > Mettre à jour la base) pour identifier les jeux ; il n'est pas redistribué.

```sh
docker compose -f docker/compose.yaml run --rm --no-deps -v "$PWD/dos:/in:ro" --entrypoint bash emu-wslg \
  -c 'mkdir -p /data/roms/dos && cp -r /in/. /data/roms/dos/'
```

## Amstrad CPC et Commodore 64 (5.0, en développement)

Cores `cap32` (CPC, GPL-2.0), `vice_x64` (C64, GPL-2.0, par défaut) et `vice_x64sc` (C64 précis, environ 5 fois plus
lent), installés depuis l'écran Cores. Contenus dans `/data/roms/` (de préférence `cpc/` et `c64/`) : CPC `.dsk`, `.cdt`,
`.voc`, `.sna`, `.cpr`, `.zip`, `.m3u` ; C64 `.d64`, `.d71`, `.d81`, `.g64`, `.nib`, `.t64`, `.tap`, `.prg`, `.p00`,
`.crt`, `.zip`, `.m3u`. Les écritures se font sur des copies de travail (`/data/saves/<core>/`).

**ROM système** : intégrées aux cores, jamais extraites ni copiées dans l'image ou le volume ; rien à fournir.
- CPC : "Amstrad has kindly given its permission for its copyrighted material to be redistributed but Amstrad retains
  its copyright." (partie (c) Locomotive Software, CP/M (c) Digital Research).
- C64 : KERNAL, BASIC, CHARGEN, DOS 1541 (c) Commodore, fournis par VICE ; statut de licence incertain, usage personnel.

JiffyDOS (commercial, facultatif, jamais fourni) : le copier dans le volume sous `/data/system/vice/`, ou utiliser
« Importer un BIOS... » après l'avoir déposé dans un dossier visible du conteneur :

```sh
docker compose -f docker/compose.yaml run --rm --no-deps -v "$PWD/bios:/in:ro" --entrypoint bash emu-wslg \
  -c 'mkdir -p /data/system/vice && cp /in/JiffyDOS_*.bin /data/system/vice/'
```

Mode d'entrée par défaut : **Joystick** ; Ctrl droit + `J` bascule en mode Clavier (60 % AZERTY pris en charge ; clavier
virtuel `K`). Cassettes : menu Cassette (Lecture, Arrêt, Rembobiner). Les DAT (clean-cpc-db MIT, TOSEC, No-Intro) sont
téléchargés à la demande et gardent leur licence. Aucune ROM de jeu n'est dans l'image.

## Manette (2.1)

Le clavier fonctionne toujours. Pour une manette PS4 :

- **WSLg ne transmet pas les manettes USB** à WSL2. Sous Windows, installer usbipd-win et attacher la manette :
  `usbipd attach --wsl --busid <id>`. Vérifier ensuite dans WSL que `/dev/input/event*` existe (`ls /dev/input`).
- Décommenter la ligne `- /dev/input:/dev/input` du service dans `docker/compose.yaml` (commentée, car compose
  échoue si le chemin n'existe pas).
- L'utilisateur `emu` (uid 1000) doit pouvoir lire `/dev/input/event*` : ajouter `group_add: ["<gid>"]` avec le gid
  numérique du groupe `input` de l'hôte (`getent group input`).
- Ajouter `GILRS_DISABLE_UDEV=1` dans `environment` : la base udev est absente du conteneur, sans cette variable
  aucune manette n'est détectée (gilrs lit alors `/dev/input` directement).
- `--no-gamepad` désactive la manette ; réglages dans la section `[input.gamepad]` de `/data/config.toml`.
- Sous noVNC, pas de manette.

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
