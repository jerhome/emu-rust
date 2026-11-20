# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/). Versionnage : [SemVer](https://semver.org/lang/fr/).

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
