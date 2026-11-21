# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/). Versionnage : [SemVer](https://semver.org/lang/fr/).

## [4.0.0-rc.1] - 2026-11-21

Première pré-release de la 4.0 : **PC-DOS** (dosbox_pure). Guide d'essai : `docs/guide-essai-4.0.md`.

### Ajouté

- PC-DOS : jeux en **zip** (lus tels quels) ou en **dossier** (copie « instantané » préparée dans le cache, avec
  progression et annulation ; vos dossiers ne sont jamais modifiés), CD (`.iso`, `.cue`) et disquettes (`.img`).
- Menu DOS de dosbox_pure (programme mémorisé), « Changer de programme de démarrage », « Lancer un autre
  programme… » (touche P), « Réinitialiser les modifications du jeu… ».
- Réglages DOS en tête des options du core ; disposition du clavier DOS selon la langue (AZERTY en français) ;
  clavier virtuel PC 102 touches ; sticks analogiques de la manette pour les jeux DOS.
- Musique MIDI : SoundFont libre **GeneralUser GS** (32 Mo, téléchargée seulement à votre demande depuis l'écran
  Cores, empreinte vérifiée), applicable à tous les jeux DOS ; ROM MT-32 / CM-32L importables.
- « Vider le cache DOS » ; ligne de commande : `--rom <dossier>`, `--rom JEU/START.EXE`, `--clear-dos-cache`.

### Modifié

- Bibliothèque : schéma v5 (jeux-dossiers), migration automatique sans perte.
- Un core qui ne se décharge pas proprement (dosbox_pure) est rechargé depuis une copie privée : relancer ou changer
  de jeu DOS ne plante plus (ADR 0022).
- Un `.chd`, `.cue` ou `.iso` hors des dossiers `roms/dos/` et `roms/megacd/` est « non classé ».

### Limites connues

- Une bibliothèque ouverte par la 4.0 n'est plus lisible par une 3.x.
- Windows 3.x / 95 installés dans dosbox_pure : non testés.

## [3.0.0-rc.1] - 2026-11-21

Première pré-release de la 3.0 : **Amiga** (puae ; puae2021 recommandé sur Mac) et les règles communes des versions
suivantes. Guide d'essai : `docs/guide-essai-3.0.md`.

### Ajouté

- Amiga : disquettes ADF (copies de travail, jamais modifiées), IPF, disques durs HDF / WHDLoad (LHA), jeux à plusieurs
  disquettes (DF0 + DF1), modèle choisi automatiquement (A500 / A1200) ou forcé par jeu.
- Sans Kickstart : démarrage avec AROS (intégré au core) après un avertissement unique par jeu ; « Importer un
  Kickstart… » / « Importer un BIOS… » (fichier vérifié par son empreinte, copié sous le nom attendu, jamais
  d'écrasement d'un fichier existant sans confirmation ; fichier inconnu importable seulement « non vérifié »).
- Écran Cores : « Core utilisé par système » (puae2021 marqué « recommandé sur Mac »).
- Clavier Amiga : Alt gauche / Alt droit = Amiga gauche / droite (Alt + F4, Alt + Tab, Alt + Espace restent au
  système) ; clavier virtuel Amiga 500 ; correspondances réglables (`[computer.remap.amiga]`).
- Bibliothèque : bouton « Ouvrir le dossier des ROMs » (sous Docker : chemin affiché) ; fichiers non classés listés
  après un scan avec le dossier conseillé ; DAT téléchargés par source (TOSEC, WHDLoad…).
- « Réinitialiser » efface aussi les disquettes de sauvegarde créées par le core pour ce jeu.
- Ligne de commande : `--import-bios`, `--list-bios` par modèle.

### Modifié

- Bibliothèque : schéma v4 (plusieurs DAT par système), migration automatique sans perte depuis la 2.x.
- Atari ST : le DAT TOSEC est désormais bien utilisé (la 2.x téléchargeait le DAT No-Intro).

### Limites connues

- Une bibliothèque ouverte par la 3.0 n'est plus lisible par une 2.x (base plus récente refusée, jamais modifiée).
- Fichiers `.ipf` hors du dossier `amiga/` et hors DAT : « non classés ».

## [2.1.0-rc.2] - 2026-11-21

Deuxième pré-release de la 2.1 : **identité de jeu**, disquettes ajoutées mémorisées, réglages de la manette.

### Ajouté

- Identité de jeu : deux jeux homonymes (`disk1.st` dans deux dossiers, deux `Game.sfc`) ont des sauvegardes séparées,
  le second devient `Nom (2)`. Le choix est mémorisé dans le registre `.emu-rust-games` de `saves/<core>/`. Un
  bandeau l'annonce une fois. Aucun fichier existant n'est renommé ni supprimé.
- Héritage du `.srm` et des options par jeu : un homonyme déjà joué en 2.0 reçoit une copie (cible absente seulement) ;
  les états restent attachés au premier jeu, et le bandeau le dit.
- Confirmation avant de charger un état d'un autre fichier du même nom : la réponse par défaut est **Non**.
- Disquettes ajoutées (Atari ST) restaurées à chaque lancement, dans le même ordre, sans être insérées. Menu
  Disquettes : retrait de la liste (la copie est gardée) ou retrait avec suppression de la copie (confirmation),
  « Nettoyer les copies… » (rien n'est coché d'office), limite de 32 disquettes avec le message
  « Liste pleine : retirez d'abord une disquette ajoutée. ».
- Réglages de la manette (Réglages → Manette) : état, zone morte, seuil des gâchettes, raccourci, remappage des boutons
  par système (« appuyez sur un bouton »).
- Navigation de la bibliothèque à la manette, avec répétition quand la direction est maintenue.
- Mega CD en ligne de commande : le message de BIOS manquant s'affiche aussi sans l'interface.
- Core inconnu + `.zip` : un zip qui ne contient que des fichiers de console est lu directement (comportement 1.0) ;
  une disquette, une extension inconnue ou un zip illisible ⇒ copie de travail.

### Limites connues

- Pas de jeu Mega CD à plusieurs disques, pas d'import de BIOS par bouton, pas d'identification automatique des
  disques.
- Verrou en repli : si le système ne gère pas le verrou de fichier (certains partages réseau), deux instances lancées
  au même instant peuvent attribuer le même nom. Rien n'est supprimé. Message de journal à guetter :
  `lock not available (…); concurrent launches are not protected` (fichier `.emu-rust-games.lock`).

## [2.1.0-rc.1] - 2026-11-21

Pré-release de la 2.1 : **Mega CD**, **manette PS4** et arrêt propre.

### Ajouté

- Mega CD / Sega CD (core genesis_plus_gx, picodrive en alternative) : images `.cue`/`.bin`, `.chd` et `.iso` lues
  sans copie (les fichiers du jeu ne sont jamais modifiés), pistes du `.cue` masquées dans la bibliothèque, région lue
  dans l'en-tête du disque (étiquette du nom pour un `.chd`).
- BIOS Mega CD par région (`bios_CD_E.bin`, `bios_CD_U.bin`, `bios_CD_J.bin`) déposé à la main dans `system/` :
  jamais téléchargé. Dialogue « BIOS manquant » (nom attendu, dossier, « Copier le chemin », « Réessayer »),
  avertissement si la révision diffère de la référence.
- Sauvegarde interne du Mega CD par jeu (`saves/<core>/<jeu>.brm`).
- Manette PS4 (DualShock 4, USB et Bluetooth) via gilrs : mapping par système, navigation dans l'interface,
  plusieurs manettes (joueur 1, 2…), branchement et débranchement à chaud.
- Raccourcis PS + : R1 sauver l'état, L1 le charger, Options menu, R2 avance rapide ; Atari ST : R3 clavier virtuel,
  L3 disquette suivante.
- Section `[input.gamepad]` de `config.toml` (zone morte, seuil des gâchettes, surcharge de boutons par profil ou par
  manette) ; option `--no-gamepad`. Une configuration 2.0 reste valable.
- Docker : `libudev` dans l'image, montage de `/dev/input` et `GILRS_DISABLE_UDEV=1` documentés.

### Modifié

- Arrêt propre : Ctrl+C, SIGTERM, SIGHUP et ⌘Q (macOS) écrivent les sauvegardes avant de quitter, comme la fermeture
  de la fenêtre.

### Corrigé

- Retentative d'ouverture d'une manette, réveil de la boucle toutes les 100 ms sans manette, nom de manette échappé
  dans le journal.

### Limites connues

- Pas de jeu Mega CD à plusieurs disques, pas d'import de BIOS par bouton, pas d'identification automatique des
  disques.

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
