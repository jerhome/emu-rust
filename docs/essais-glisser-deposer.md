# Glisser-déposer de jeux dans la fenêtre — étude technique pour la version 7.2

Date : 2026-11-21. Périmètre : `docs/specs/v7.1-v8.0-utilisateur.md` §7.2 — déposer des jeux dans la fenêtre
d'emu-rust, rangés automatiquement dans `roms/`, **uniquement depuis le gestionnaire de fichiers du système** ;
toute autre source ⇒ simple message d'erreur. L'utilisateur veut **en discuter avant la conception** : ce document
est une étude, pas une décision (aucune ADR, aucun code). Les questions pour lui sont dans
`docs/questions-ouvertes.md`, section « 7.2 ».

Niveaux de preuve : **lu** = relevé dans la source des dépendances du `Cargo.lock` (winit 0.30.13, egui-winit
0.37.0) ; **non vérifié** = comportement connu des systèmes ou des logiciels tiers, à confirmer par un essai réel
(aucun essai de dépôt n'est possible dans le conteneur de développement, sauf X11 simulé, §8).

## 1. Résumé

- **Techniquement simple pour le cas nominal** : winit donne déjà, sans dépendance ni `unsafe` nouveau, le
  **chemin** de chaque fichier ou dossier déposé depuis l'Explorateur (Windows), le Finder (macOS) ou un
  gestionnaire de fichiers X11 (Linux). emu-rust n'utilise pas eframe : les événements passent par
  `emu-ui/src/egui_layer.rs` et sont aujourd'hui ignorés.
- **On ne peut pas savoir quelle application est à l'origine du dépôt** (aucun système ne le dit à la cible, et
  winit ne transmet que des chemins). « Uniquement depuis le gestionnaire de fichiers » se traduit donc par une
  règle sur **ce qui arrive** : un chemin de fichier local, existant, durable. Le reste se trie en deux familles :
  - ce qui **n'arrive jamais** jusqu'à emu-rust (lien ou image d'un navigateur, pièce jointe d'un e-mail, texte) :
    le système affiche lui-même le curseur « interdit » ; **emu-rust ne peut afficher aucun message**, car il ne
    reçoit rien ;
  - ce qui arrive sous forme de **fichier temporaire** (archive ouverte dans 7-Zip, WinRAR, « dossier compressé »
    de l'Explorateur…) : reconnaissable par son emplacement ⇒ message d'erreur « extrayez d'abord l'archive ».
- **Fiable** : Windows et macOS (les deux machines de l'utilisateur). **Linux** : X11 seulement ; **rien sous
  Wayland** avec winit 0.30 (donc rien non plus dans la fenêtre native sous WSLg, qui passe par Wayland), rien
  d'utile sous Docker.
- Le rangement de la 7.1 (ADR 0023 : classement, unités, aperçu, journal, annulation) fournit l'essentiel ; il
  manque une opération « importer depuis l'extérieur de `roms/` » (copie, pas déplacement interne).

## 2. Ce que fournissent winit et egui

winit 0.30.13 (**lu**, `src/event.rs`) :

- `WindowEvent::HoveredFile(PathBuf)` : des fichiers survolent la fenêtre (un événement **par fichier**) ;
- `WindowEvent::HoveredFileCancelled` : ils sont repartis ;
- `WindowEvent::DroppedFile(PathBuf)` : dépôt (un événement **par fichier**, tous émis à la suite).

Aucune position de dépôt, aucune information sur la source, aucun autre type de donnée (texte, URL, contenu
virtuel). Un dossier déposé arrive comme **un** chemin de dossier (non parcouru).

egui-winit 0.37.0 (**lu**, `src/lib.rs`) recopie ces événements dans `RawInput::hovered_files` et
`RawInput::dropped_files` (`egui::DroppedFile { path: Some(..), .. }`), lisibles par `ctx.input(|i| …)`. Les
écrans hors jeu (bibliothèque, cores) passent déjà leurs événements de fenêtre à `EguiLayer::on_window_event` :
**la donnée y est déjà disponible**, il reste à la lire (en jeu, egui ne reçoit les événements que menu ouvert :
le dépôt y serait lu directement dans la boucle d'événements). Les fichiers d'un même geste doivent être regroupés par l'application (tous
ceux reçus dans la même passe d'événements).

winit 0.31 (bêta, non utilisable : egui 0.37 est sur 0.30) remplace ces événements par `DragEntered` /
`DragMoved` / `DragDropped` / `DragLeft`, avec la liste complète des fichiers et la position ; son journal des
modifications **ne mentionne aucune prise en charge sous Wayland**. À isoler derrière une petite fonction pour que
la migration soit locale.

## 3. Comportement par système

### Windows (**lu**, `platform_impl/windows/drop_handler.rs`)

- winit enregistre une cible OLE (`IDropTarget`, activée par défaut : `with_drag_and_drop(true)`) et ne demande
  que le format **`CF_HDROP`** (liste de chemins). Tout dépôt sans `CF_HDROP` est refusé au survol
  (`DROPEFFECT_NONE` : curseur interdit) et **ne produit aucun événement**.
- L'effet annoncé est toujours **copie** (`DROPEFFECT_COPY`) : la source ne supprime jamais rien ; c'est emu-rust
  qui décide de copier ou de déplacer le fichier dont il a reçu le chemin.
- Chemins reçus : lettre de lecteur, lecteur réseau (`Z:\…`), chemin UNC (`\\serveur\partage\…`), chemins longs.
- **Non vérifié**, à contrôler à l'essai : un programme lancé **en administrateur** ne reçoit aucun dépôt d'un
  Explorateur non élevé (protection de Windows) ; un fichier OneDrive « à la demande » a un chemin valide mais se
  télécharge à la lecture ; un téléphone branché (MTP) ne fournit pas de `CF_HDROP` (rien n'arrive).
- La cible OLE exige `OleInitialize` sur le fil de la fenêtre ; elle cohabite aujourd'hui avec l'audio (cpal /
  WASAPI) sans défaut constaté par l'utilisateur. À ne pas désactiver par mégarde.

### macOS (**lu**, `platform_impl/macos/window_delegate.rs`)

- La fenêtre s'enregistre pour le seul type `NSFilenamesPboardType` (chemins de fichiers). Un dépôt sans ce type
  (fichier « promis » : pièce jointe de Mail, photo de Photos, image de Safari) est refusé dès l'entrée dans la
  fenêtre et **ne produit aucun événement**.
- **Non vérifié** : à la première lecture d'un fichier du Bureau, de Documents ou de Téléchargements, macOS peut
  demander l'autorisation d'accès au dossier (application non signée, hors bac à sable) ; un refus donne une
  erreur de lecture qu'il faudra expliquer.

### Linux X11 (**lu**, `platform_impl/linux/x11/event_processor.rs`, `dnd.rs`)

- Protocole XDND ; winit n'accepte que le type `text/uri-list`, et uniquement des URI `file:///…` **sans nom
  d'hôte** ; chaque chemin est résolu par `canonicalize` (liens suivis, fichier obligatoirement existant). Une
  seule URI d'un autre schéma (`http://`, `sftp://`, `smb://`) ou un fichier absent fait échouer **tout** le
  dépôt, sans événement.
- Conséquences : un lien de navigateur, un partage réseau montré par Nautilus en `sftp://` / `smb://`, une
  extraction directe d'un gestionnaire d'archives (protocole « XDS ») ne donnent rien. Un partage monté dans le
  système de fichiers (`/mnt/…`, `/run/user/<uid>/gvfs/…`) fonctionne.

### Linux Wayland, WSLg, Docker

- **Wayland : aucun glisser-déposer de fichiers dans winit 0.30.13** (**lu** : `DroppedFile` n'est émis que par les
  parties Windows, macOS et X11). winit choisit Wayland dès que `WAYLAND_DISPLAY` est défini ; emu-rust ne force
  aucun des deux aujourd'hui. Sur un bureau Wayland la fonction serait donc absente, sauf à **forcer X11**
  (XWayland) au démarrage — possible (`EventLoopBuilderExtX11::with_x11`), mais c'est un changement d'affichage
  pour tout le reste (mise à l'échelle, icône, comportement des fenêtres) qui demanderait sa propre décision.
- **WSLg** : la fenêtre Linux est servie par Wayland ⇒ rien. Et même en X11, WSLg ne relaie pas un glisser-déposer
  de l'Explorateur Windows vers une fenêtre Linux (**non vérifié**, limite connue de WSLg). Sous Windows,
  l'utilisateur a de toute façon le binaire natif.
- **Docker** : noVNC ne transmet aucun dépôt ; en X11 partagé avec un hôte Linux, le chemin reçu est celui de
  l'hôte, absent du conteneur ⇒ refusé. Fonction sans objet : le bouton « Ouvrir le dossier des ROMs » (3.0) et
  « Ranger les ROMs… » (7.1) restent la voie.

### Tableau de synthèse

| Source du geste | Windows | macOS | Linux X11 |
|---|---|---|---|
| Gestionnaire de fichiers, fichier local | chemin reçu | chemin reçu | chemin reçu |
| Idem, plusieurs fichiers / un dossier | un événement par élément ; dossier = son chemin | idem | idem |
| Lecteur réseau, clé USB | chemin reçu (copie plus lente) | chemin reçu (`/Volumes/…`) | reçu si monté ; rien si `smb://` / `sftp://` |
| Navigateur : lien, image d'une page | rien (curseur interdit) | rien | rien |
| Navigateur : fichier **déjà téléchargé**, tiré depuis la barre de téléchargements | chemin du fichier téléchargé (c'est un vrai fichier) — non vérifié | idem — non vérifié | idem — non vérifié |
| E-mail : pièce jointe (Outlook, Mail, webmail) | rien — non vérifié | rien — non vérifié | rien ou fichier temporaire selon le logiciel |
| Archive ouverte dans 7-Zip / WinRAR | chemin **temporaire** (`%TEMP%\7zE…`, `%TEMP%\Rar$…`), que le logiciel **supprime juste après le dépôt** — non vérifié | sans objet (le Finder extrait d'abord) | Ark : `/tmp/ark-…` ; file-roller : rien (XDS) — non vérifié |
| « Dossier compressé » de l'Explorateur (zip ouvert comme un dossier) | chemin temporaire (`%TEMP%\Temp1_<nom>.zip\…`) ou rien — non vérifié | — | — |
| Texte, sélection, onglet | rien | rien | rien |

« Non vérifié » = à observer pendant l'essai Windows / macOS de l'utilisateur ; la conception ne doit pas dépendre
de ces lignes, seulement de la règle du §4.

## 4. Distinguer « gestionnaire de fichiers » du reste

Impossible au sens strict : la cible d'un dépôt ne connaît pas l'application source. Deux conséquences à
présenter à l'utilisateur.

**4.1 Ce qui n'arrive pas ne peut pas avoir de message.** Pour un lien de navigateur ou une pièce jointe, winit
refuse le survol : le pointeur montre « interdit » et rien ne se passe au lâcher. C'est le comportement standard
des applications, mais ce n'est pas le « simple message d'erreur » de la spécification. Obtenir un vrai message
demanderait de remplacer la cible de dépôt de winit par la nôtre (code natif par système : `IDropTarget` sous
Windows, types supplémentaires de `NSPasteboard` sous macOS, XDND sous X11 — `unsafe` dans `emu-runtime`, revue
`reviewer`, et un risque de conflit avec winit). Coût disproportionné pour un message : **non recommandé**
(question 7.2 n° 3).

**4.2 Ce qui arrive est filtré par une règle, pas par la source.** Proposition :

1. le chemin existe, est lisible, est un fichier ordinaire ou un dossier (pas un périphérique, pas un lien
   symbolique vers ailleurs — même prudence que le rangement 7.1) ;
2. il n'est **pas dans un emplacement temporaire ou un cache** : dossier temporaire du système
   (`std::env::temp_dir()`, `%TEMP%`, `%LOCALAPPDATA%\Temp`, `/tmp`, `/var/tmp`, `/private/var/folders/…`),
   caches d'applications (`%LOCALAPPDATA%\Microsoft\Windows\INetCache`, `~/Library/Caches`, `~/.cache`),
   motifs connus (`Temp1_*.zip`, `7zE*`, `7zO*`, `Rar$*`, `ark-*`, `.fr-*`) ;
3. il n'est pas dans les dossiers de données d'emu-rust autres que `roms/` (`system/`, `saves/`, `cores/`…) ;
4. il n'est pas déjà dans `roms/` — sinon c'est un simple rangement 7.1 de cet élément, pas un import.

Un chemin refusé par 1 à 3 donne le message d'erreur, avec la raison et le remède (« Ce fichier vient d'une
archive ouverte : extrayez-la d'abord dans un dossier, puis déposez le fichier extrait »). La liste des
emplacements temporaires est une **heuristique** : elle peut laisser passer un outil inconnu (le fichier est alors
copié normalement s'il existe encore, sinon erreur « fichier disparu ») ou refuser à tort un utilisateur qui range
ses jeux dans `/tmp` (peu probable).

Sécurité : un chemin déposé est une entrée non fiable. Rien n'est exécuté ; un dossier est parcouru avec les
bornes du scan (profondeur, nombre de fichiers, liens non suivis) ; les noms passent par les mêmes contrôles que le
rangement (pas d'écrasement, comparaison insensible à la casse).

## 5. Copier ou déplacer

- Habitudes des systèmes : entre deux dossiers du **même volume** l'Explorateur et le Finder **déplacent** ; entre
  deux volumes ils **copient** ; vers une application, l'usage est de laisser l'original en place.
- winit annonce toujours « copie » à la source : l'original n'est touché que si emu-rust le fait lui-même.
- **Copier** (recommandé par défaut) : l'original reste où il est, rien d'irréversible, « annuler » = retirer la
  copie. Coût : place disque doublée (un CD = 300 à 700 Mo) et durée de copie (barre de progression, annulable).
- **Déplacer** : pas de doublon, instantané sur le même volume ; mais le fichier disparaît de l'endroit où
  l'utilisateur l'avait mis (Téléchargements, clé USB, disque réseau), ce qui surprend, et l'annulation doit le
  remettre.
- Compromis possible : copier par défaut, case « Déplacer au lieu de copier » dans l'aperçu (mémorisée).

Question 7.2 n° 1.

## 6. Intégration avec le rangement de la 7.1 (ADR 0023)

Ce qui se réutilise tel quel : le classement « comme si aucun dossier ne décidait » (sondes d'en-tête, empreinte
DAT, contenu d'un zip, nom, extension), les unités (fichier seul, jeu à plusieurs disquettes, `.cue` + pistes,
`.m3u`, jeu-dossier DOS), les statuts (sûre / suggérée / ambiguë / inconnue / bloquée), les collisions (doublon,
conflit, jamais d'écrasement ni de renommage), la copie vérifiée entre volumes (`.emu-rust-part`, taille + CRC32),
le journal, le verrou, le scan incrémental final, le signalement d'un BIOS reconnu par son md5 (« Importer un
BIOS… »).

Ce que la 7.2 ajoute (esquisse, à concevoir après la discussion) :

- une analyse sur **une liste de chemins extérieurs** au lieu de `roms_root` (`plan_import(sources)` à côté de
  `plan_tidy`), qui produit le même `TidyPlan` ;
- une opération de journal « copie importée » (ou « déplacement importé ») : l'annulation **retire la copie** si
  elle est inchangée (taille + date), ou remet le fichier à sa place d'origine s'il a été déplacé et que la place
  est libre. C'est la seule entorse à « le rangement ne supprime jamais » : il ne retire que ce qu'il vient de
  créer, et jamais un fichier modifié depuis ;
- les fichiers **compagnons** : déposer un `.cue` seul peut emporter ses pistes (elles sont à côté de l'original,
  le chemin est connu) ; déposer les pistes sans le `.cue` reste bloqué avec une explication ;
- l'interface : voile « Déposez vos jeux ici » pendant le survol (`hovered_files`), puis **aperçu** (le même écran
  que « Ranger les ROMs… », limité aux éléments déposés) ou rangement direct selon la réponse à la question n° 2,
  puis rapport avec « Annuler » ;
- les états où le dépôt est refusé avec un message : partie en cours, scan, téléchargement ou rangement en cours
  (mêmes règles que le rangement 7.1).

Points d'attention pour la 7.1 en cours (aucune modification demandée à ADR 0023, simple compatibilité) : garder le
classeur appelable sur une liste de chemins quelconques ; le journal est déjà versionné (`emu-rust tidy 1`), une
opération nouvelle passera par une version 2 lue par la reprise et l'annulation.

Dépendance : la 7.2 ne peut pas précéder la 7.1 (FB1 : `tidy.rs`).

## 7. Cas particuliers à trancher

| Cas | Proposition |
|---|---|
| Dossier entier déposé | Accepté, parcouru comme le fait le rangement (jeu-dossier DOS déplacé en bloc, sinon fichier par fichier), **toujours avec aperçu** ; bornes de profondeur et de nombre |
| Zip déposé depuis le gestionnaire de fichiers | Traité comme par le rangement : jamais décompressé ; zip d'une ROM de console = jeu ; zip contenant une image CD = refusé avec le conseil habituel |
| Fichier d'une archive ouverte (temporaire) | Refusé, message « extrayez d'abord » |
| Fichier déjà présent dans `roms/` (même nom, même contenu) | « Déjà dans votre bibliothèque », rien n'est copié |
| Même nom, contenu différent | Conflit : laissé, signalé (règle 7.1) |
| Fichier non reconnu / ambigu (`.bin`, `.chd`, `.iso` sans indice) | Aperçu avec choix du système (règle 7.1) ; jamais copié « au hasard » |
| BIOS reconnu par son md5 | Proposer « Importer un BIOS… » (3.0), pas de copie dans `roms/` |
| Core (`.dll` / `.so` / `.dylib`), exécutable, autre | Refusé : « ce n'est pas un jeu reconnu » |
| Très gros fichier, lecteur réseau lent | Copie en tâche de fond, progression, annulable ; place libre vérifiée avant de commencer (nouveau par rapport à la 7.1, où un déplacement ne consomme rien) |
| Dépôt pendant une partie | Refusé avec un message (« quittez le jeu pour ajouter des jeux ») ; « insérer cette disquette dans le jeu en cours » serait une autre fonction, hors 7.2 |
| Dépôt sur l'écran Cores ou Réglages | Même traitement que sur la bibliothèque (toute la fenêtre est la cible) |
| Plusieurs fenêtres | Sans objet : une seule fenêtre active à la fois (à confirmer contre le code à la conception) |

## 8. Tests possibles sans écran

| Niveau | Moyen | Couvre |
|---|---|---|
| Règle d'acceptation (§4.2), regroupement, messages | tests unitaires purs (`emu-ui`), chemins fabriqués dans un `tempdir` — attention : le `tempdir` des tests est lui-même sous `/tmp`, la liste des emplacements temporaires doit donc être **injectable** | toute la logique, sur les trois systèmes en CI |
| Import (plan, copie, journal, annulation, reprise) | tests d'intégration `emu-library`, comme `tidy_*.rs` | copie vérifiée, collisions, annulation qui ne retire que l'inchangé |
| Interface | injection d'`egui::RawInput { dropped_files, hovered_files }` dans la couche egui (aucun serveur d'affichage) ; capture Xvfb du voile et de l'aperçu par un point d'entrée de test | écrans, états refusés |
| Chaîne complète X11 | source XDND minimale en Python `ctypes` (sur le modèle de `tests/tools/xinject.py` ; `xdotool` n'est pas installé et ne sait de toute façon pas simuler un dépôt de fichiers) : devient propriétaire de la sélection `XdndSelection`, envoie `XdndEnter` / `XdndPosition` / `XdndDrop`, répond `text/uri-list` | winit X11 → egui → import, sous Xvfb ; **non réalisé**, estimé à une demi-journée, facultatif |
| Windows, macOS réels | essai de l'utilisateur (guide d'essai : Explorateur, 7-Zip, navigateur, pièce jointe, lecteur réseau, dossier) | les lignes « non vérifié » du §3 |

## 9. Impacts par crate (estimation)

| Crate | Travail |
|---|---|
| `emu-library` | `plan_import` (analyse d'une liste de chemins), opération de journal « import », annulation / reprise correspondantes, contrôle de place libre ; revue `reviewer` (code qui écrit chez l'utilisateur) |
| `emu-ui` | lecture de `dropped_files` / `hovered_files`, règle d'acceptation, voile de survol, aperçu réutilisé, messages, réglage copier / déplacer |
| `emu-runtime` | rien avec l'approche recommandée (pas de cible de dépôt maison) ; un éventuel forçage de X11 s'y ferait |
| `emu-libretro`, `emu-assets`, `emu-app` | rien |
| `tests/tools`, `qa-systems` | source XDND de test (facultatif), guide d'essai |

Aucune dépendance nouvelle, aucun `unsafe` nouveau, aucun code réseau, graphe des crates inchangé — sous réserve
des réponses aux questions n° 3 (message pour les sources non reçues) et n° 8 (Wayland).

## 10. Points ouverts

1. Lignes « non vérifié » du §3 : à observer à l'essai (7-Zip, WinRAR, dossier compressé, Outlook, Mail, barre de
   téléchargements d'un navigateur, OneDrive, autorisation macOS).
2. Liste exacte des emplacements temporaires par système ; comportement si elle se trompe.
3. Annulation d'un import : retirer la copie (proposé) ou la laisser dans `roms/` ?
4. Wayland : accepter l'absence (proposé), ou étudier le forçage de X11.
5. Migration future vers winit 0.31 (événements groupés, position) : isoler la lecture des événements.
