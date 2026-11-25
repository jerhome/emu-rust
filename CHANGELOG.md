# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/). Versionnage : [SemVer](https://semver.org/lang/fr/).

## [7.5.0] - non publiée (tag à valider)

Finition PC : le travail de la ligne PC (7.1.0-rc.1 et 7.2, jamais publiés sur `main`) est réappliqué sur la 7.4
(contrat : `docs/design/v7.5-finition-pc.md`). Guide d'essai : `docs/guide-essai-7.5.md`. Schéma de la base
inchangé (v7) ; aucune migration.

### Ajouté

- **Journal** : page « Journal » ouverte depuis le pied de page (compteur des erreurs et avertissements non lus) ou
  par le lien des Réglages ; filtres, lignes des cores, copie de tout le journal ; fichier `logs/emu-rust.log`
  (session en cours et précédente, nettoyage des données sensibles, taille bornée, trace d'un plantage, verrou entre
  deux instances).
- **Pied de page** sur toutes les pages et dans le menu en jeu : « emu-rust 7.5.0 — dev by claude@jerhome », bouton
  « Journal » à droite.
- **Messages copiables** : texte sélectionnable et bouton « Copier » sur les messages d'erreur, le journal et les
  derniers messages du menu en jeu ; le presse-papiers est réellement actif (il ne l'était pas en 7.4).
- **Windows** : `emu-rust-cli.exe` (ligne de commande, livré à côté d'`emu-rust.exe`) remplace
  `emu-rust-console.exe` ; boîte de message Windows pour une erreur fatale.
- **Réglage « Afficher le terminal (après redémarrage) »** (`[app] show_terminal`) : sous Windows, relance par
  `emu-rust-cli.exe` ; sous macOS, relance dans Terminal.app (inchangé). L'ancienne clé `[interface] console` de la
  7.1.0-rc.1 est lue puis retirée du fichier.
- **Écran Cores** : fichiers d'origine facultatifs par core (BIOS GBA et Game Boy, ROM Roland MT-32 / CM-32L de
  dosbox_pure), états vérifiés par empreinte md5, systèmes à core unique sans menu de choix, recherche.
- **BIOS PlayStation** : les autres révisions (SCPH-1001, 1002, 7002…) sont reconnues à l'import et copiées sous le
  nom attendu pour leur région.
- **CD 32X** : « Ce jeu est un CD 32X » / « Ce n'est pas un CD 32X » au clic droit ou à la touche X (menu unique
  avec « Réinitialiser la jaquette »), mémorisé dans `[library] cd32x` ; invitation dans la ligne d'aide ; étiquettes
  `(Sega CD 32X)`, `(Mega CD 32X)`, `(32X CD)`, `(CD 32X)` reconnues, casse et tiret indifférents.
- Tests : `mkcd.py --variant 32x`, aller-retour de `config.toml` des deux lignes, relecture des étiquettes 32X sur
  une base 7.4 ou v6.

### Modifié

- Bandeau du jeu sélectionné : fichiers vérifiés par leur empreinte (« présent, non vérifié » quand elle est
  inconnue, import forcé possible) ; PlayStation et 3DO « fonctionnent mieux avec » le fichier, le Mega CD l'« exige ».
- Icône (dessin 7.4) produite par une chaîne de génération reproductible (`packaging/icon/`, empreinte du dessin).
- Notice de picodrive : il n'intègre aucun BIOS Sega (démarrage 32X assuré par le core).
- Une étiquette `(32X)` seule n'est plus reconnue comme CD 32X (marque manuelle).
- Un CD 32X ne se lance jamais avec `genesis_plus_gx` : le dialogue propose d'installer picodrive.

### Corrigé

- Message « buildbot en cours de mise à jour » au lieu d'une erreur de CRC quand le serveur des cores se met à jour.
- Disquettes numérotées par un chiffre final (`MANIACD0`, `MANIACD1`) regroupées en un seul jeu, pour tous les
  ordinateurs.

## [7.4.0] - non publiée (tag à valider)

### Ajouté

- **Sega CD 32X** : les CD Mega CD + 32X sont reconnus au scan (signature « 32X Initial Program » de la piste de
  données, ou nom Redump) et lancés avec picodrive ; badge 32X dans la bibliothèque.
- Installation de picodrive proposée si le core manque.

### Corrigé

- Les disques de Fahrenheit (Sega CD) et de Fahrenheit (32X CD) ne sont plus regroupés en un seul jeu ; ceux de
  Night Trap (Disc 1 / Disc 2) le restent quand ils sont dans le même dossier.

## [7.3.0] - non publiée (tag à valider)

Contient aussi les versions 7.1 et 7.2 (voir ci-dessous).

### Ajouté

- **Thème sombre** unique, accent bleu de l'icône (`#4aa3ff`), couleurs, typographie et espacements cohérents,
  contrastes vérifiés ; rien de lourd pour les petites machines (pas de flou ni d'animation coûteuse).
- **Bibliothèque en grille de jaquettes** : systèmes en colonne à gauche, jaquettes mises en valeur ; navigation au
  clavier ou à la manette inchangée.
- **Manuel intégré** à l'application : bouton « Aide » de la bibliothèque, touche F1, `?` ou Ctrl+H (pour les claviers
  sans touche F), entrée « Aide » du menu en jeu, et petits « ? » d'aide à côté des réglages.
- **Assistant de premier lancement** : dossier des ROMs avec premier scan, cores à installer, clavier ou manette.
- **États vides utiles** (bibliothèque sans jeu, sans jaquette, etc.) et **messages d'erreur** qui disent quoi faire.
- **Réinitialiser la jaquette d'un jeu** (touche R ou clic droit) et **toutes les jaquettes** (Réglages, avec
  confirmation).

### Corrigé

- Une jaquette restait affichée sur un autre jeu qui reprenait le même identifiant après un nouveau scan.
- Les compteurs de jaquettes sont désormais calculés par jeu : ils ne s'additionnent plus à chaque réinitialisation.

## [7.2.0] - non publiée (tag à valider)

### Ajouté

- **Manettes USB simples de type SNES / Super Famicom** absentes de la base de manettes connue : l'application
  les détecte et propose de les configurer (« Configurer » / « Ignorer »). Un écran d'apprentissage demande
  d'appuyer sur chaque bouton ; la configuration est mémorisée pour cette manette. Sous macOS, l'application essaie
  d'abord de deviner les boutons puis propose d'apprendre le reste (« Deviner + proposer »). Un message s'affiche
  aussi en jeu. Essayé sur deux manettes Super Famicom USB (`081f:e401`).

### Corrigé

- Apprentissage : une croix directionnelle rapportée sur des axes, dont la première valeur est -1, n'était plus
  prise pour une gâchette.
- Identifiant de manette insensible à la casse ; configuration de manette invalide ignorée au lieu de bloquer.

## [7.1.0] - non publiée (tag à valider)

### Ajouté

- **Pied de page** : version de l'application et « dev by claude@jerhome ».
- **Icône d'application** identique sous Windows, macOS et Linux.
- **Terminal caché par défaut** ; un réglage permet de le réafficher après redémarrage (sous macOS, l'application se
  relance alors dans Terminal).
- **Journal** consultable dans l'interface.
- **Bandeau d'avertissement firmware** : indique quand un core est amélioré par un fichier d'origine fourni par
  l'utilisateur (jamais téléchargé) et propose de l'importer.

## [7.0.2] - 2026-11-23

### Corrigé

- **Jaquettes** : aucune jaquette n'arrivait, car `thumbnails.libretro.com` répond désormais « introuvable » pour
  toutes les images. Les jaquettes libretro sont maintenant prises d'abord sur le dépôt GitHub
  `libretro-thumbnails` (`raw.githubusercontent.com`), le site libretro restant en secours ; HTTPS et ces deux hôtes
  seulement, sans redirection, au plus 2 requêtes simultanées.
- Une source en panne (réponses « introuvable » en série pour des jeux reconnus d'au moins 3 systèmes) ou une erreur
  réseau ne marque plus les jeux « sans jaquette » pendant 30 jours ; au premier démarrage de la 7.0.2, les marques
  « sans jaquette » déjà enregistrées sont effacées une fois pour que tout soit redemandé.
- Statut lisible : « Jaquettes : X trouvées, Y introuvables » (et « source injoignable » le cas échéant) ; chaque échec
  est écrit dans le journal.

## [7.0.1] - 2026-11-23

### Corrigé

- **Mega CD à plusieurs disques** (ex. Night Trap en deux `.chd`) : `genesis_plus_gx` refusait le jeu (« could not
  load … from M3U file list ») et `picodrive` répondait « invalid CD image ». La liste `.m3u` générée contenait des
  chemins absolus que ces deux cores préfixent toujours par le dossier de la liste. Pour eux, le disque 1 est
  désormais chargé directement et les disques suivants sont ajoutés par l'interface de changement de disque
  (`multi_disc = "disk_control"` dans `cores.toml`) ; aucune liste n'est plus générée. PlayStation inchangée.

## [7.0.0] - 2026-11-22

Version stable de la 7.0 : contenu de la 7.0.0-rc.1, **validé par l'utilisateur sous Windows** : PlayStation (GTA avec
le BIOS intégré et avec un BIOS d'origine, mode DualShock retenu, cores `pcsx_rearmed`, `swanstation` et
`mednafen_psx`), 3DO (Road Rash avec Opera Boot intégré et avec `panafz10.bin`), zip CPC (B.A.T.). Aucun changement
de code depuis la rc.

## [7.0.0-rc.1] - 2026-11-21

Première pré-release de la 7.0 : **PlayStation** et **3DO** (rendu logiciel). Guide d'essai :
`docs/guide-essai-7.0.md`. Contient aussi les correctifs des zips CPC et de l'ordre des disquettes (voir « Corrigé »).

### Ajouté

- PlayStation : core `pcsx_rearmed` par défaut ; `swanstation` (rendu logiciel seulement) et `mednafen_psx`
  (Beetle PSX, plus précis, plus lent) au choix.
- Jeux PlayStation : `.cue` + `.bin`, `.chd`, `.iso`, `.pbp`, `.m3u`, piste `.bin` seule ; exécutables PS-EXE
  (`--rom programme.exe`) ; jeux à plusieurs CD regroupés en un seul jeu (menu « CD… », PS + L3 à la manette) ;
  identification Redump par numéro de série et région ; fichier `.sbi` (LibCrypt) accepté à côté de l'image.
- BIOS PlayStation par région (`scph5500.bin`, `scph5501.bin`, `scph5502.bin`) : **facultatifs et jamais
  téléchargés** ; sans eux, le core utilise son remplaçant intégré (BIOS HLE, OpenBIOS sous licence MIT) et
  l'application le signale **une seule fois par jeu** (importer un BIOS ou continuer).
- Manette PlayStation : mode **numérique** (par défaut) ou **DualShock** (analogique), réglable globalement
  (Réglages → Manette, `--pad-device`), par jeu et à chaud depuis le menu ; clavier et DS4 mappés.
- Cartes mémoire PlayStation par jeu (`saves/<core>/<jeu>.srm`), communes aux disques d'un même jeu et
  **reprises d'un core PS1 à l'autre** (copie unique, message à l'écran).
- 3DO : core `opera`, jeux `.cue`, `.chd`, `.iso`, `.bin` ; **Opera Boot** intégré au core, `panafz10.bin` préféré
  s'il est présent (importable) ; NVRAM par jeu ; touches clavier et DS4 3DO.
- Bibliothèque : systèmes « PlayStation » et « 3DO » reconnus par le contenu des disques (descripteur, `SYSTEM.CNF`,
  en-tête Opera) ; un `.chd` ambigu reste non classé jusqu'à son rangement dans le bon dossier.
- `--check-bios` pour PlayStation (par région) et 3DO ; réglages de cores PlayStation / 3DO en tête des options.
- Écran Cores : ligne « BIOS HLE intégré — BIOS d'origine recommandé » (information, pas une erreur).
## [6.0.0] - 2026-11-21

Version stable de la 6.0 : contenu de la 6.0.0-rc.1, **validé par l'utilisateur sous Windows** sur les 15 plates-formes
(Mega Drive, Super Nintendo, NES, Master System, Game Gear, Game Boy, Game Boy Color, Game Boy Advance, 32X, Mega CD,
Atari ST, Amiga, PC-DOS, Amstrad CPC, Commodore 64 ; son, manette PS4, sauvegardes, changement de disquette).

### Corrigé

- Un jeu CPC en `.zip` passe par l'extraction : cap32 n'insérait aucune disquette depuis un zip (« disc missing »).
- Un zip de disquettes d'ordinateur devient un jeu multi-disquettes, dans l'ordre logique (« Disk N / Side X »,
  TOSEC « Disk N of M ») ; les disquettes extraites sont listées dans cet ordre dans le menu.
- `dosbox_pure` ne plante plus quand les options du core sont lues sans jeu chargé.
- Une piste audio `.bin` d'une base 6.0 est purgée au rescan ; un gros `.bin` sans en-tête n'est plus haché.

### Limites connues

- Rendu logiciel seulement : pas de haute résolution ni de PGXP (SwanStation compris).
- Pas de vibration de manette.
- 3DO : pas de changement de disque (un jeu par disque, jeux à plusieurs CD non regroupés).
- Compatibilité des BIOS de remplacement (HLE, OpenBIOS, Opera Boot) inférieure à celle des BIOS d'origine : certains
  jeux peuvent mal démarrer sans BIOS.
- `.chd` et `.pbp` reconnus par leur nom ; jamais de CD dans un zip.
- Cartes mémoire copiées une fois d'un core à l'autre, non synchronisées.
- Sega CD 32X (Mega CD + 32X) : prévu en 7.2.
  TOSEC « Disk N of M ») ; les disquettes extraites sont listées dans cet ordre.

## [6.0.0-rc.1] - 2026-11-21

Première pré-release de la 6.0 : **Game Boy Advance**, **32X** et **Mega CD** complété. Guide d'essai :
`docs/guide-essai-6.0.md`.

### Ajouté

- Game Boy Advance : core `mgba` (`gpsp` et `vbam` au choix), jeux `.gba` (zip accepté), BIOS facultatif
  (`gba_bios.bin`), réglages GBA en tête des options du core.
- Sauvegardes GBA (`saves/mgba/<jeu>.srm`) et états.
- Touches et manette GBA : croix, A / B, L / R, Start / Select (DS4 : Rond = A, Croix = B, L1 / R1 = L / R).
- 32X : core `picodrive`, BIOS intégrés au core (rien à fournir), jeux `.32x` (zip accepté).
- Mega CD complété :
  - BIOS par région (`bios_CD_E.bin`, `bios_CD_U.bin`, `bios_CD_J.bin`) : dialogue générique « BIOS manquant »
    (nom exact, région, chemin du dossier `system/`, importer, réessayer, lancer quand même) ; un BIOS dont le md5
    est différent est accepté avec un avertissement ;
  - jeux à plusieurs CD : `.m3u` ou `(Disc N)`, un seul jeu dans la bibliothèque, menu « CD… » pour changer de
    disque ;
  - identification Redump (numéro de série, région) ;
  - `.chd` conseillé (un fichier par disque) ;
  - RAM de sauvegarde par jeu (`saves/genesis_plus_gx/<jeu>.brm`).
- Le message d'erreur du core est affiché quand un jeu ne se charge pas (cause d'origine conservée).
- Ligne de commande : `--check-bios` par région ; un jeu Mega Drive ou 32X ne demande plus de BIOS CD.

### Modifié

- Bibliothèque : schéma v6 (région du disque), migration automatique sans perte.
- Les `.bin` 32X sont reclassés « 32X » par leur en-tête (ils étaient classés Mega Drive).

### Limites connues

- Le suffixe `(CD N)` n'est pas reconnu pour regrouper les disques (utiliser `(Disc N)` ou un `.m3u`).
- La région d'un `.chd` rangé hors de `roms/megacd/` n'est pas lue.
- Une bibliothèque ouverte par la 6.0 n'est plus lisible par une 5.x (base plus récente refusée, jamais modifiée).
## [5.0.0-rc.1] - 2026-11-21

Première pré-release de la 5.0 : **Amstrad CPC** (cap32) et **Commodore 64** (vice_x64, vice_x64sc). Guide d'essai :
`docs/guide-essai-5.0.md`.

### Ajouté

- Amstrad CPC (cap32) et Commodore 64 (vice_x64 par défaut, vice_x64sc en alternative plus exacte).
- ROM système intégrées aux cores (CPC : permission d'Amstrad ; C64 : ROM VICE) : l'écran Cores affiche « ROM intégrées
  au core » et la notice de licence ; rien à importer.
- Mode d'entrée **Joystick** par défaut pour le CPC et le C64 (ZQSD / flèches + J / K), Ctrl droit + J pour passer au
  clavier ; les entrées « BASIC » démarrent en Clavier.
- Chargement accéléré automatique, piloté par le core (OSD « Chargement accéléré », son coupé) ; désactivable.
- Menus **Cassette** (lecture, arrêt, rembobinage) et **Port joystick** (C64, retenu par jeu).
- Commande de démarrage : `RUN"…` (CPC), programme d'une disquette C64 (« Programme à lancer… »), mémorisée par jeu.
- Sauvegardes CPC par jeu (`saves/cap32/<jeu>/`) ; les disquettes d'origine ne sont jamais modifiées.
- Claviers virtuels CPC (AZERTY) et C64.
- JiffyDOS facultatif : importable par « Importer un BIOS… » et « Activer JiffyDOS pour tous les jeux C64 ».
- Ligne de commande : `--launch <commande>`, `--no-auto-fast-load`.

### Modifié

- Réglage `[computer] keyboard_mode` (Automatique / Clavier / Joystick) ; ancienne valeur migrée automatiquement.
- Un zip mélangeant des disquettes CPC et C64 est ignoré (compté dans le résumé du scan).

### Limites connues

- vice_x64sc est environ 5 fois plus lent que vice_x64 (peut manquer de fluidité sur un Mac Intel modeste).
- Un `.m3u` écrit par l'utilisateur au format VICE (`image:PRG`) n'est pas pris en charge.
- Aucune migration de bibliothèque : la 5.0 ajoute seulement deux systèmes.

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
