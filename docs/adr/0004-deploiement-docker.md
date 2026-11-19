# ADR 0004 — Docker : environnement de dev et cible de déploiement

Statut : accepté (2026-11-19)

## Contexte

- Claude travaille dans un conteneur sans écran, sans son, sans périphériques d'entrée et sans accès au démon Docker.
- L'utilisateur est sur Windows + WSL2 avec Docker Desktop. Docker est une **cible de déploiement à part entière**,
  au même titre que macOS, Windows ou Linux.

## Décision

Deux images distinctes :

| Image | Fichier | Usage |
|---|---|---|
| dev | `docker/dev/Dockerfile` | Toolchain + bibliothèques système + Xvfb : reproduit l'environnement de Claude (build, tests, captures). |
| runtime | `docker/runtime/Dockerfile` | Build release multi-étapes, image finale minimale, utilisateur non root, données dans le volume `/data`. |

Modes d'affichage de l'image runtime (`docker/compose.yaml`, un service par mode) :

1. **wslg** (défaut sur Windows) : montage de `/tmp/.X11-unix`, `/mnt/wslg`, `/usr/lib/wsl`, périphérique `/dev/dxg`
   (GPU) ; son via `PULSE_SERVER=/mnt/wslg/PulseServer`.
2. **x11** : hôte Linux, socket X11 + PulseAudio/PipeWire de l'hôte.
3. **novnc** : écran virtuel (Xvfb) + `x11vnc` + noVNC sur `http://localhost:6080`. Image et clavier/souris, **pas de son**.
   Pour les tests et le dépannage.

Docker sous macOS n'est pas visé (pas de GPU, XQuartz nécessaire) : build natif à la place.

## Conséquences

- C'est l'utilisateur qui construit et lance l'image runtime ; Claude écrit et maintient les Dockerfiles.
- La manette dans Docker (usbipd-win) n'est pas garantie : le jeu à la manette passe par les builds natifs.
- Données persistantes (cores, BIOS, ROMs, sauvegardes, covers, config) dans le volume `/data`, jamais dans l'image.
