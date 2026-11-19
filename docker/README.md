# Docker

Images : `dev` (toolchain, Xvfb) et `runtime` (build release, utilisateur `emu`, données dans `/data`). Voir ADR 0004.
Commandes à lancer depuis la racine du dépôt.

## Données dans `/data` (volume `emu-rust_emu-data`)

```text
/data/cores/    cores libretro (*.so)
/data/roms/     ROMs (jamais fournies par l'application ni l'image)
/data/bios/     BIOS
/data/saves/    SRAM : saves/<core_id>/<jeu>.srm (compatible RetroArch)
/data/states/   savestates à emplacements, par core puis par jeu
/data/assets/   covers et métadonnées téléchargées
/data/config.toml   configuration (touches, options) ; droits 0600, propriétaire `emu`
```

Un seul volume porte tout le persistant ; le dossier de données est `EMU_DATA_DIR=/data` (équivalent de
`--data-dir /data`, voir ADR 0011). L'entrypoint crée `saves/ states/ assets/` s'ils manquent et remet `config.toml`
en `0600` (il pourra contenir des identifiants). Sauvegarde du volume :

```sh
docker run --rm -v emu-rust_emu-data:/data:ro -v "$PWD:/out" debian:bookworm-slim tar czf /out/emu-data.tgz -C /data saves states config.toml
```

Le compilateur C (`build-essential`) n'est présent que dans les étages de build (dev et build du runtime) :
`emu-libretro` compile une cale C via `cc` (ADR 0009) ; l'image finale n'en contient pas.

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
