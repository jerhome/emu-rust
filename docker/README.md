# Docker

Images : `dev` (toolchain, Xvfb) et `runtime` (build release, utilisateur `emu`, données dans `/data`). Voir ADR 0004.
Commandes à lancer depuis la racine du dépôt.

## Données dans `/data` (volume `emu-rust_emu-data`)

```text
/data/cores/   cores libretro (*.so)
/data/roms/    ROMs (jamais fournies par l'application ni l'image)
/data/bios/    BIOS
```

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
