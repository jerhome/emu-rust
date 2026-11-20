# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/). Versionnage : [SemVer](https://semver.org/lang/fr/).

## [2.0.0-rc.2] - 2026-11-21

Corrections issues de l'essai de la rc.1 (Windows) et de deux revues indépendantes.

### Corrigé

- Installation d'un core : un catalogue en cache plus vieux que le build nightly provoquait « CRC32 x au lieu de y ».
  Le catalogue de plus de 12 h est rafraîchi avant l'installation, et un écart de CRC déclenche un rafraîchissement
  puis, si besoin, un seul retéléchargement ; un fichier non conforme n'est jamais installé.
- Copies de travail des disquettes : chaque dossier retient l'identité de ses disquettes sources (taille + CRC32).
  Deux jeux homonymes (`disk1.st`) ou une image source remplacée obtiennent des copies séparées (`<jeu> (2).disks`),
  l'ancienne progression n'est jamais écrasée ; « Réinitialiser les disquettes » vise le dossier de la session ;
  les disquettes ajoutées par le menu sont retrouvées d'une session à l'autre.
- Un core inconnu (`--core`) ne travaille directement sur les fichiers que pour un contenu 100 % console.
- `.m3u` lus avec les mêmes règles par la bibliothèque et au lancement ; casse ignorée sous Windows et macOS.
- Touches tapées pendant la pause ignorées ; noms réservés Windows (CON, NUL…) dans les options par jeu ;
  lecture plus sûre des valeurs renvoyées par le core.

## [2.0.0-rc.1] - 2026-11-20

Première pré-release de la 2.0 : **Atari ST** et l'infrastructure « ordinateur » (ADR 0015).

### Ajouté

- Atari ST (core hatari, hatarib en alternative) : lancement depuis la bibliothèque (jeux sur plusieurs disquettes
  regroupés, `.m3u`, zip) ou sans jeu (« Bureau Atari ST »), et en ligne de commande (`--rom` répétable, `--no-game`).
- EmuTOS 1.4 (TOS libre, GPL) téléchargé et vérifié par hash (192 Ko, langue du système, français par défaut) ;
  un `tos.img` fourni par l'utilisateur n'est jamais écrasé sans confirmation (sauvegarde `.user-<date>`).
- Touche hôte **Ctrl droit** (tous systèmes) : menu et raccourcis (Hôte+K clavier virtuel, M souris, J joystick /
  clavier, D disquette suivante) ; clavier ST complet par position de touche ; clavier virtuel (F1-F10, Help, Undo,
  pavé numérique).
- Souris capturée au clic (repli sans capture sous noVNC/WSLg), menu Disquettes, options par jeu
  (`game-options/`), copies de travail : les disquettes de l'utilisateur ne sont jamais modifiées.
- Bibliothèque : système Atari ST, regroupement multi-disquettes (schéma de base v3, migration automatique).

## [1.0.0] - 2026-11-20

Première version stable. Contenu identique à la 1.0.0-rc.2, validé par l'utilisateur (`docs/essais-1.0.md`) :
Windows (SNES, Mega Drive, NES, Master System, Game Boy, Game Gear) et macOS Intel 12.7.6 (Mega Drive).

### Modifié

- Release : builds, licences et image Docker en parallèle ; image poussée seulement si les 3 archives sont prêtes ;
  label OCI de licence « MIT OR Apache-2.0 ».

## [1.0.0-rc.2] - 2026-11-20

Corrections issues du premier essai sous Windows (`docs/essais-1.0.md`).

### Modifié

- Disposition clavier par défaut sans flèches (claviers 60 %) : directions sur Z Q S D en AZERTY (W A S D en QWERTY),
  les flèches restant actives ; boutons sur J K L / U I O selon le système.
- Raccourcis utilisables sans touches F : Ctrl+S / Ctrl+L (état), Ctrl+, / Ctrl+. (emplacement), Ctrl+I (statistiques),
  Ctrl+F ou Alt+Entrée (plein écran), Ctrl+R (reset). Reset déplacé de H vers F5.
- Ctrl, Alt et la touche Windows ne peuvent plus être assignés à un bouton de jeu.

### Corrigé

- Le son reprend automatiquement après la perte du périphérique audio (session verrouillée, casque débranché,
  changement de sortie) au lieu de rester coupé jusqu'au redémarrage ; état de la sortie audio dans les statistiques.

## [1.0.0-rc.1] - 2026-11-20

Première version « complète » : petites consoles, bibliothèque de jeux, jaquettes et gestionnaire de cores.
Systèmes : Mega Drive, Super Nintendo, **Game Boy / Game Boy Color**, **NES**, **Master System** et **Game Gear**.

### Ajouté

- Gestionnaire de cores intégré (écran Cores ou `--install-core`, `--list-cores`, `--update-cores`) : installation
  vérifiée depuis le buildbot libretro (HTTPS, tailles bornées, SHA-256), mises à jour, retour arrière vers la version
  précédente. Les BIOS sont vérifiés par empreinte (md5) mais jamais téléchargés.
- Bibliothèque de jeux : scan des dossiers de ROMs, hash CRC32/SHA-1, identification par les DAT No-Intro, base
  SQLite, favoris et récemment joués. `emu-rust` sans argument ouvre la bibliothèque.
- Jaquettes : libretro-thumbnails, puis ScreenScraper, puis IGDB ; désactivées par défaut (réglage explicite,
  identifiants jamais loggés, fichier de configuration en `600`).
- Nouveaux systèmes : Game Boy / Color (gambatte), NES (mesen), Master System et Game Gear (genesis_plus_gx).
- Profils clavier par défaut pour chaque système (Master System / Game Gear, Game Boy / NES).
- Docker : volume `/data` étendu (`cores`, `covers`, `database`, `library.sqlite`).
- Données embarquées (empreintes de BIOS, libretro-database, CC-BY-SA-4.0) : `THIRD-PARTY-DATA.txt` dans les archives.

### Corrigé

- Lancement depuis la bibliothèque : taille de fenêtre selon le format d'image du jeu.
- Scan : les fichiers vides sont ignorés.
- Ouverture de la base SQLite sans récupération automatique ; échéance globale pour le téléchargement des jaquettes.
- Divers points relevés par les revues (réseau, extraction d'archives, écran Cores).

### Limites connues

- Pas de manette (clavier uniquement) ; la manette PS4 est prévue plus tard.
- Le retour d'un jeu vers la bibliothèque n'est pas testé automatiquement (essai manuel).
- Jaquettes JPEG de plus de ~2700 x 2700 pixels refusées.
- Game Gear : un savestate est restauré sans image avec la nightly actuelle de genesis_plus_gx (défaut du core).
- Aucune ROM ni BIOS commercial n'est fourni ni téléchargé.
- macOS : application non notarisée (validation manuelle de Gatekeeper requise).
- Windows : avertissement SmartScreen attendu (binaire non signé).
- Profil Docker `x11` non testé (pas d'hôte Linux disponible).

## [0.1.0] - 2026-11-20

Première pré-release. Systèmes cibles : **Mega Drive** (genesis_plus_gx) et **Super Nintendo** (snes9x).

### Ajouté

- Frontend libretro en Rust : chargement dynamique des cores (`.so` / `.dylib` / `.dll`), callbacks d'environnement
  complets, formats de pixels, rendu vidéo OpenGL.
- Sortie audio via cpal, synchronisation A/V, pause et avance rapide.
- Sauvegardes : SRAM persistante et savestates.
- Chargement des ROM depuis une archive zip (traitée comme non fiable).
- Interface egui : menu, configuration des touches, options des cores, prise en charge HiDPI.
- Profils clavier par défaut selon le système (Mega Drive, Super Nintendo, générique).
- Configuration au format TOML ; option `--version` ; lancement sans argument.
- Builds : image Docker (WSLg, X11, noVNC), binaire Linux x86_64, bundle `.app` macOS Intel, zip Windows (MSVC) ;
  CI sur Ubuntu, macOS Intel et Windows ; workflow de release.

### Corrigé

- Ordre d'initialisation libretro conforme à libretro.h et à RetroArch : les callbacks vidéo, audio et entrées
  sont posés après `retro_init` (certains cores plantaient sinon).

### Limites connues

- Pas de manette (clavier uniquement) ; la manette PS4 est prévue plus tard.
- Les cores libretro ne sont pas fournis : à placer par l'utilisateur (voir `packaging/README.txt`).
  Aucune ROM ni BIOS commercial n'est fourni ni téléchargé.
- macOS : application non notarisée (validation manuelle de Gatekeeper requise).
- Windows : avertissement SmartScreen attendu (binaire non signé).
- Profil Docker `x11` non testé (pas d'hôte Linux disponible) ; WSLg et noVNC sont les profils essayés.
