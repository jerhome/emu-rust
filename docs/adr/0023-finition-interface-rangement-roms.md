# ADR 0023 — Version 7.1 : finition de l'interface, console cachée, rangement de `roms/`, fichiers d'origine

Statut : **accepté** (2026-11-21, architecte ; proposé le même jour, amendé après la revue indépendante — reprise
par unité, marqueur d'identités, groupes multi-CD, console à deux exécutables, copie entre volumes retirée ;
**amendé le 2026-11-22 après la revue FV0** du journal : §4.3 `.bin` sans en-tête, §4.8 ; **précisé le même jour
après la revue RV1** : §4.8, « Précisions RV1 »). Choix
de l'utilisateur faits : icône **B** (croix et boutons), avertissement BIOS **une fois par (core, fichier)** ; les
autres défauts sont appliqués et consignés (`docs/questions-ouvertes.md`, lignes 7.1). Périmètre :
`docs/specs/v7.1-v8.0-utilisateur.md` §7.1 (fait autorité ; point 5 = BIOS / firmware d'origine fourni par
l'utilisateur, jamais téléchargé ; rangement avec aperçu et annulation). Précise
ADR 0005 (rien de commercial n'est téléchargé), 0006 (arborescence `roms/<système>/`), 0011 (identité de jeu,
écritures atomiques, `config.toml`), 0012 (paquets), 0014 (bibliothèque), 0013 / 0021 (firmware, repli du core).
Réutilise sans les changer les règles de classement des ADR 0015, 0017 à 0021 (ordinateurs, CD, extensions
partagées). Conception et découpage : `docs/design/v7.1-finition-interface.md`.
**Graphe des crates inchangé. Schéma de `library.sqlite` inchangé (v6). Aucun code réseau nouveau.**

## Contexte

Faits relevés dans le code de `wip-7.1` (= `v7.0.0-rc.1`) :

- `emu-app` est un binaire console : sous Windows une fenêtre de terminal s'ouvre avec l'application. Le journal
  (`tracing`, filtre `warn` + `info` pour nos crates) et les erreurs fatales (`eprintln!`) ne vont que sur `stderr`.
  `tracing-subscriber` n'est une dépendance que d'`emu-app`.
- `unsafe` est interdit hors `emu-libretro` / `emu-runtime` ; `emu-app` ne dépend que d'`emu-ui`. Les appels
  `AttachConsole` / `AllocConsole` de Windows sont des appels FFI. `windows-sys` est déjà dans `Cargo.lock`
  (0.52.0, 0.59.0, 0.61.2).
- Un programme du sous-système graphique lancé depuis `cmd` ou PowerShell n'est **pas attendu** par l'invite :
  la sortie se mêle à l'invite et `$LASTEXITCODE` / `%ERRORLEVEL%` ne sont pas renseignés, même après
  `AttachConsole`. Un programme du sous-système console lancé par un processus sans console reçoit de Windows une
  console neuve. Dans un processus graphique sans redirection, les poignées standard sont nulles (lisible en Rust
  sûr : `AsRawHandle`).
- Le scan ne suit aucun lien de dossier (`scan.rs`, parcours) ; `group_media` ne regroupe que des fichiers du
  **même dossier** ; une ligne nouvelle est insérée par `store_batch` avant la purge des lignes disparues ; les
  lignes d'une racine externe absente sont protégées (`keep`). Le registre d'identité compare des chemins
  **canoniques** (`registry_key`). Le verrou du registre est un verrou de l'OS (`File::lock`).
- La bibliothèque identifie un jeu par son **chemin** (`game.path UNIQUE`, `game_media.path UNIQUE`) : favori,
  historique, identification et jaquettes sont portés par la ligne. Le scan ne reconnaît pas un fichier déplacé :
  il supprime la ligne et en crée une autre (favori et historique perdus).
- Sauvegardes (ADR 0011, amendement 2.1) : `.srm`, états, options par jeu et dossier `.disks` sont nommés par le
  `save_name`, dérivé du **nom du fichier**, pas de son dossier. Le registre `<saves>/<core_id>/.emu-rust-games`
  mémorise le chemin de la source ; « même empreinte et ancien chemin disparu ⇒ même identité » existe déjà.
- Classement au scan : dossier nommé d'après l'`id` du système > sondes d'en-tête > DAT > extension ; les
  extensions partagées (`folder_extensions` : `.chd`, `.cue`, `.iso`, `.m3u`, `.ipf`, `.zip` d'ordinateur…) ne sont
  attribuées **que** sous `roms/<id>/`, sinon le fichier est « non classé ». Les jeux-dossiers DOS ne sont reconnus
  que sous `roms/dos/`. 17 systèmes au catalogue (`systems.toml`).
- Firmware : `cores.toml` porte `required_firmware` (avec `fallback` pour PlayStation, 3DO, Amiga ⇒ état
  `CoreFallback`, avertissement une fois par jeu depuis la 3.0 / 7.0) et `builtin_firmware` ; les `.info` installés
  listent `firmwareN_path` / `firmwareN_opt` (facultatif) ; `firmware.toml` donne les md5 de référence.
- Interface : thème sombre d'egui, accent bleu `#285AA0`, contour de focus `#96C8FF`, avertissement `#FFBE50`.
  Aucune icône (ni fenêtre, ni exécutable, ni `.app`, ni `.desktop`). Pas de presse-papiers (choix d'ADR 0002).
- Outils ici : ImageMagick (rendu SVG interne) et Python 3 sans Pillow ; ni `rsvg-convert` ni Inkscape.

## Décision

### 1. Pied de page

Bandeau bas (`TopBottomPanel::bottom`) sur les écrans hors jeu (bibliothèque, cores, réglages) et en bas du menu en
jeu : `emu-rust <version> — dev by claude@jerhome`, version = `env!("CARGO_PKG_VERSION")` (version du workspace,
exposée par `emu_ui::VERSION`, la même que `--version`). Le bandeau porte aussi le bouton « Journal » (§3). Jamais
dessiné par-dessus l'image du jeu.

### 2. Icône

- **Une seule image**, source SVG `packaging/icon/emu-rust.svg` = **variante B** (croix directionnelle et deux
  boutons ronds sur fond bleu ; choix de l'utilisateur du 2026-11-21 ; formes pleines, sans dégradé, sans texte,
  sans marque ni manette d'un fabricant). Les quatre propositions restent dans `docs/design/icone/`.
- **Fichiers générés et commités** (une compilation n'a besoin d'aucun outil graphique) par `scripts/gen-icons.sh` :
  PNG 16 à 1024 px, `emu-rust.ico` (16, 24, 32, 48, 64, 256), `emu-rust.icns`. Rasterisation et `.ico` par
  **ImageMagick** (son rendu SVG interne suffit à des formes pleines : vérifié sur la planche) ; `.icns` par un
  script Python sans dépendance (conteneur de PNG). Pas d'image Docker d'outillage. Le script écrit
  `packaging/icon/SOURCE.sha256` (empreinte du SVG) ; la CI vérifie seulement que cette empreinte correspond au
  SVG commité (un rendu identique au bit près d'une version d'outil à l'autre n'est pas exigé).
- **Windows** : icône et informations de version dans l'exécutable par une ressource, posée par `build.rs`
  d'`emu-app` avec `winresource` (MIT, `build-dependencies` sous `cfg(windows)`, outil `rc.exe` du runner MSVC).
  Compilation croisée depuis Linux : pas de ressource, pas d'échec. **macOS** : `Contents/Resources/emu-rust.icns`
  + `CFBundleIconFile`. **Linux** : `share/icons/hicolor/<taille>/apps/emu-rust.png`, `scalable/apps/emu-rust.svg`,
  `share/applications/emu-rust.desktop` (`Terminal=false`, `StartupWMClass=emu-rust`) dans le `tar.gz`.
- **Fenêtre** (toutes les fenêtres : jeu, bibliothèque, cores) : `with_window_icon` de `winit` à partir des PNG
  de `packaging/icon/png/` embarqués par `include_bytes!` (32, 64, 256 ; décodés par `png`, déjà au workspace ;
  aucune copie dans `emu-ui`) — barre de titre et barre des tâches Windows, X11
  (`_NET_WM_ICON`, donc noVNC). Wayland n'a pas d'icône de fenêtre : `app_id` = `emu-rust` pour que le `.desktop`
  s'applique. macOS ignore l'icône de fenêtre (Dock = `.icns`). Aucune dépendance nouvelle à l'exécution.

### 3. Console cachée, journal dans l'interface

- **Windows : deux exécutables, le même programme.** `emu-rust.exe` (sous-système graphique : aucun terminal au
  double-clic) et `emu-rust-cli.exe` (sous-système console), deux cibles `[[bin]]` d'`emu-app` de trois lignes
  chacune qui appellent la même fonction de la bibliothèque d'`emu-app`. Le second n'est construit et livré que
  sous Windows (fonctionnalité Cargo `cli-bin`). Solutions écartées : `AttachConsole` / `AllocConsole` dans un
  exécutable unique — l'invite n'attend pas, `$LASTEXITCODE` est faux (scripts et guide d'essai cassés), chaque
  poignée standard doit être reprise à la main (`CONOUT$`, `SetStdHandle`), le tout en `unsafe` et sans test
  automatique possible hors ConPTY. Avec deux exécutables, **Windows fait le travail** et il ne reste aucun appel
  de console :
  1. ligne de commande (`--help`, `--version`, `cores …`, `--check-bios`, scripts, CI) : `emu-rust-cli.exe` —
     attendu par l'invite, code de retour et tubes corrects. `README.txt` et l'aide le disent ;
  2. `emu-rust.exe` avec `[interface] console = true`, en mode fenêtre et sans sortie standard : il **relance**
     `emu-rust-cli.exe` (fichier voisin, mêmes arguments, variable `EMU_RUST_CONSOLE_CHILD=1` contre toute boucle)
     et se termine ; Windows ouvre la console du second. Le réglage est lu au démarrage : **effet après
     redémarrage** (dit dans les Réglages). Voisin absent ⇒ avertissement au journal, démarrage sans console ;
  3. `emu-rust.exe` appelé avec une sous-commande : elle s'exécute (sortie correcte si elle est redirigée : tube,
     fichier) ; sans sortie standard, une boîte de message renvoie vers `emu-rust-cli.exe`.
  Seul appel FFI restant : `MessageBoxW` (erreur fatale avant l'ouverture d'une fenêtre, sans console), dans
  `emu-runtime` (module `dialog`, `// SAFETY:`, revue `reviewer`), derrière une fonction sûre réexportée par
  `emu-ui`. La détection « pas de sortie standard » est en Rust sûr. Le lancement réel avec console (point 2)
  reste un **essai de l'utilisateur** ; la décision (pure) et les en-têtes PE des deux exécutables sont testés.
- **Linux / macOS / Docker** : il n'y a pas de fenêtre de terminal propre à l'application. « Caché » veut dire :
  lancement par l'icône (`.desktop` `Terminal=false`, `.app` par le Finder) sans terminal ; lancé **depuis** un
  terminal, le journal continue d'y être écrit (c'est le terminal de l'utilisateur, `docker logs` en dépend). Le
  réglage « Afficher la console » n'est proposé que sous Windows ; ailleurs une ligne d'explication le remplace.
- **Journal capturé** : une couche `tracing` (`emu_ui::log`) alimente un tampon circulaire en mémoire, installé par
  `emu-app` dès le début de `main` à côté de la sortie `stderr` (conservée). Bornes : 2 000 entrées, message tronqué
  à 2 000 caractères, lignes des cores nettoyées (`sanitize_core_line` : caractères de contrôle et bidi retirés).
  Niveaux capturés : ceux du filtre actuel (`warn` global, `info` pour nos crates et le core ; `RUST_LOG` l'emporte).
  Les secrets ne sont déjà jamais journalisés (`emu_assets::Secret`) : inchangé.
- **Fichier** : `<data>/logs/emu-rust.log`, alimenté par le même tampon dès que la racine est connue ; au démarrage
  l'ancien devient `emu-rust.log.1` ; écriture arrêtée à 2 Mio (ligne « journal tronqué »). Un `panic` y est écrit
  (crochet de panique) : sans console c'est la seule trace d'un plantage. Mode `0600` sous Unix. Rotation ou
  ouverture impossible (seconde instance) ⇒ pas de fichier pour cette instance, un avertissement, rien d'autre.
- **Panneau « Journal »** (pied de page, avec un compteur d'erreurs et d'avertissements non lus ; aussi dans le menu
  en jeu) : liste horodatée, filtre Erreurs / Avertissements / Tout, origine (application ou core) et « Ouvrir le dossier
  des journaux » (pas de presse-papiers ; le fichier suffit, pas d'« Enregistrer sous… »). Ce qui n'existait que sur le terminal y
  figure : racine des données, core et jeu chargés, messages du core, refus d'un core (`core_error`), conseils de
  `diagnostics`. Les messages déjà affichés (bandeaux, OSD) restent où ils sont.

### 4. Rangement de `roms/`

**4.1 Périmètre.** Réglages → « Dossier des ROMs » : (a) « Créer les dossiers des systèmes » crée `roms/<id>/` pour
chaque système du catalogue (17 aujourd'hui), sans rien déplacer ; (b) « Ranger les ROMs… » analyse, **montre un
aperçu**, puis déplace ce que l'utilisateur a laissé coché. Seul `<data>/roms` est rangé (les dossiers externes de
`[library] roots` ne sont jamais touchés). Est « déjà rangé » tout élément dont le chemin relatif commence par le
dossier de **son** système. Le rangement **déplace, ne renomme jamais, ne supprime jamais, n'écrase jamais, ne
décompresse jamais** ; il ne suit aucun lien symbolique (un lien est laissé en place et signalé).

**4.2 Unités.** L'analyse regroupe les fichiers en unités indivisibles, déplacées en bloc :

| Unité | Contenu | Destination |
|---|---|---|
| Fichier seul | ROM de console, `.zip`, image unique (`.chd`, `.iso`, `.pbp`, `.adf`, `.st`, `.d64`…), avec le `.sbi` de même nom d'un `.chd` / `.pbp` / `.iso` PlayStation | `roms/<id>/<nom>` |
| Jeu à plusieurs disquettes | groupe `(Disk N of M)` (`group_media`), mêmes règles que le scan | `roms/<id>/` (à plat, côte à côte) |
| CD à plusieurs fichiers | `.cue` + pistes référencées (`cue_references`) + `.sbi` ; `.m3u` + ses disques et leurs pistes | `roms/<id>/<nom du .cue ou du .m3u>/` |
| **Jeu à plusieurs CD sans `.m3u`** | le groupe `(Disc N)` **entier** : tous ses `.cue` (ou `.chd`) et toutes leurs pistes (`group_media` exige le même dossier, sinon le jeu serait éclaté en un jeu par disque) | `roms/<id>/<nom du groupe>/`, **un seul dossier** ; système sans `group_discs` (3DO) : une unité par disque |
| Pistes `.bin` sans `.cue` | regroupées comme le scan (`hide_disc_tracks`, `audio_track_name`) : piste de données + pistes audio de même radical | comme un CD à plusieurs fichiers ; regroupement impossible ⇒ **bloquée** |
| Jeu-dossier DOS | dossier avec un `.exe` / `.com` / `.bat` à sa racine | `roms/dos/<dossier>/` (le dossier entier) |
| Dossier quelconque | ni CD ni DOS | ses fichiers sont traités un par un ; le dossier vidé est retiré (noté au journal) |

Un dossier source qui contient exactement une unité à dossier (et rien d'autre) est déplacé tel quel, sous son
propre nom. Un jeu-dossier DOS laissé décoché reste **entier** en place : ses fichiers ne redeviennent jamais des
unités séparées. Un fichier appartient à une seule unité (piste citée par deux `.cue` ⇒ les deux unités bloquées).
Les fichiers qui ne sont pas des jeux (textes, images, BIOS déposés par erreur) restent en place ; un fichier dont
le md5 est celui d'un BIOS connu est signalé avec « Importer un BIOS… ».

**Unité bloquée** (expliquée dans l'aperçu, rien n'est déplacé pour elle) : référence de `.cue` / `.m3u` absolue,
hors de son dossier ou manquante ; lien symbolique ; **`roms/<id>` qui n'est pas un vrai dossier** (lien,
jonction) ou qui est sur un **autre volume** (point de montage) — le scan ne suit pas les liens et la 7.1 ne
copie pas entre volumes ; chemin non UTF-8 (la base stocke du texte) ; chemin de destination de plus de 240
caractères sous Windows ; dossier à créer dont le nom est l'`id` d'un système (il fausserait la règle de dossier) ;
système de fichiers sans liens physiques (FAT, exFAT).

**4.3 Système de destination.** Une unité **déjà connue de la bibliothèque** (`known_game`) garde le système de sa
ligne : statut Sûre, raison « bibliothèque » (ses sauvegardes sont sous `<saves>/<core_id>/`, donc liées au
système). Sinon, même ordre que le scan, appliqué « comme si aucun dossier ne décidait » :
(1) sondes d'en-tête (`disc_info`, `probe_bin`, iNES, en-tête Mega Drive / 32X…) ; (2) empreinte dans un DAT importé
(CRC32 / SHA1, série, piste 01) ; (3) contenu de l'archive (`.zip` : extensions des entrées ; une image CD dans un
zip reste refusée, règle 2.1) ; (4) nom présent dans **un seul** DAT ; (5) extension propre à un seul système.

- **Sûre** (bibliothèque, 1, 2, 3 ou 5) : cochée d'office.
- **Suggérée** (4 ; jeu-dossier DOS ; zip pris pour du DOS ; **`.bin` sans en-tête**) : montrée **décochée**,
  avec la raison. Un `.bin` que seule son extension désigne (ni en-tête Mega Drive / 32X reconnu par la sonde, ni
  empreinte dans un DAT) n'est jamais « Sûre » : l'extension est aussi celle de BIOS et de pistes de CD isolées
  (choix de FB1a, validé par la revue FV0).
- **Ambiguë** (plusieurs systèmes possibles : `.chd`, `.m3u`, `.dsk`, `.bin`, `.gb` sans DAT…) : l'aperçu propose
  les systèmes candidats, unité par unité ou **pour toute une extension d'un coup** (« tous les `.chd` ambigus →
  PlayStation ») ; sans choix, l'unité reste en place.
- **Inconnue** : laissée en place.

Choisir pour un jeu connu un système **autre** que celui de sa ligne est possible mais averti : « le core change,
les sauvegardes et états de ce jeu ne suivront pas ». Une fois sous `roms/<id>/`, la règle de dossier du scan fait
foi : c'est ce qui rend lançable un fichier jusque-là « non classé ». Les alias de dossier (`ps1/`) restent refusés.

**4.4 Collisions.** Destination déjà occupée (comparaison insensible à la casse, sur le disque **et** entre unités
du plan) : même taille et même CRC32 ⇒ **doublon**, sinon **conflit** ; dans les deux cas l'unité reste en place,
signalée. Aucun renommage automatique : le `save_name` dépend du nom du fichier.

**4.5 Exécution.** Tâche de fond annulable (entre deux unités), refusée pendant un scan, un téléchargement ou une
partie de cette instance ; **verrou de l'OS** (`File::try_lock` sur `<data>/rangement/lock`, comme le registre
2.1 : libéré par le système si le processus meurt). `roms_root` est **canonisé** avant toute chose : journal, base
et registres voient la même écriture des chemins. Par unité :

1. `B <unité>` et les intentions au **journal** (`<data>/rangement/journal-<unix>.txt`, texte, une opération par
   ligne, chemins relatifs à `roms/` échappés, racine canonique en en-tête, `sync_all` avant tout déplacement) ;
2. déplacement **sans jamais écraser** : fichier seul = `fs::hard_link(source, destination)` (échoue si la
   destination existe) puis `fs::remove_file(source)` ; unité à dossier assemblée = `create_dir` exclusif de
   `<destination>.emu-rust-part/`, liens physiques de chaque fichier, **un seul `rename`** du dossier vers son nom
   final, puis retrait des sources ; dossier déplacé tel quel (jeu DOS, CD déjà dans son dossier) = **un seul
   `rename`** (qui ne peut remplacer qu'un dossier vide sous Unix et échoue sous Windows). Autre volume
   (`ErrorKind::CrossesDevices`) ou liens physiques refusés ⇒ unité bloquée, rien n'a bougé ;
3. **bibliothèque**, dans une transaction : `game.path`, `game_media.path` et `game.pinned` de l'unité sont
   réécrits (`Library::relocate`, clés construites par la même fonction que le scan) — la ligne est conservée,
   donc favori, historique, identification, région et jaquettes aussi ; une ligne périmée déjà enregistrée à la
   destination (fichier absent) est retirée dans la même transaction ;
4. `C <unité>` (unité validée). Un échec en 2 ou 3 défait l'unité ; les autres continuent ; tout figure au rapport.

Après la dernière unité, ligne `E`, puis : (5) `emu_runtime::relocate_identities` réécrit le chemin des sources
dans les registres `<saves>/<core_id>/.emu-rust-games` et les manifestes `.disks` (sous leur verrou, écriture
atomique, comparaison par `registry_key` de chemins canoniques) ; **alors seulement** l'UI fait poser la ligne
`I` (« identités faites ») par `Library::mark_tidy_identities` ; (6) scan incrémental, qui ajoute les jeux
nouvellement classés. **Aucun fichier de `saves/`, `states/`, `game-options/` ni `covers/` n'est déplacé ni
renommé** : ils sont nommés par le `save_name` ou rattachés à la ligne du jeu, qui ne changent pas.

L'étape 5 n'est pas facultative : sans elle le registre dit encore « `roms/a.sfc` → sauvegarde `a` », et un
**autre** `a.sfc` déposé plus tard à la racine hériterait du `.srm` du jeu rangé. Tant qu'un journal validé n'a
pas sa ligne `I`, l'étape 5 est rejouée (idempotente) à l'ouverture de la bibliothèque **et** avant tout lancement
direct par la ligne de commande.

**Reprise** (au démarrage, avant le scan ; refusée si la racine canonique du journal n'est pas la racine courante),
**une seule règle, par unité** : une unité commencée et non validée (`B` sans `C`) est **défaite en entier** —
chaque fichier présent à la destination et absent à la source revient à la source ; **source et destination
présentes et identiques** (même fichier physique, ou même taille et même CRC32 : arrêt entre le lien et le
retrait) ⇒ la destination est retirée, la source reste ; présentes et différentes ⇒ rien n'est touché, signalé ;
un dossier `.emu-rust-part` est retiré après ce contrôle fichier par fichier ; la base est remise sur les chemins
sources (réécriture inverse, idempotente). Une unité validée (`C`) n'est jamais retouchée. Puis `E` est posée et
l'étape 5 rejouée.

Filet de sécurité indépendant : le **scan** reconnaît un fichier déplacé — ligne dont le chemin a disparu + ligne
nouvelle de mêmes système, taille et empreinte (ou mêmes nom, taille et date pour un fichier non haché), un seul
candidat de chaque côté. La ligne nouvelle étant déjà insérée (`store_batch`), ses champs sont **reportés sur
l'ancienne ligne, qui garde son `id`** (les jaquettes y sont rattachées), puis la nouvelle est supprimée. Les
lignes protégées (`keep` : racine externe débranchée) ne sont jamais candidates. Cela protège aussi les
déplacements faits à la main.

**4.6 Annulation.** « Annuler le dernier rangement » (Réglages, et bouton du rapport ; même verrou, même contrôle
de racine) rejoue le dernier journal à l'envers, unité par unité, avec les mêmes primitives : une unité revient si
tous ses fichiers sont encore à leur destination, inchangés (taille + date), et si leur place d'origine est libre ;
sinon elle reste entière où elle est et figure au rapport. Dossiers créés retirés s'ils sont vides, dossiers vidés
recréés ; mêmes mises à jour de la base et des registres en sens inverse (avec leur propre ligne `I`). Le journal
est alors marqué « annulé » ; les 20 derniers sont gardés. Seul le journal **le plus récent** s'annule ; s'il est
illisible, rien ne s'annule (on ne saute jamais à un plus ancien). Précisions : §4.8.

**4.7 Schéma.** Aucune migration : le journal est un fichier, `relocate` n'utilise que les colonnes de la v6.

**4.8 Journal : décisions de l'amendement du 2026-11-22** (revue FV0 du format écrit par FB1a ; grammaire, états
et reprise détaillés : design §3.4.1, qui fait foi pour le format). La version du format reste 1 : aucun journal
n'a encore été écrit chez un utilisateur.

- **Taille et date dans les intentions.** `M <n> <taille> <mtime> <de> <vers>` ; `W <fichiers> <taille> <mtime>
  <de> <vers>` (agrégat récursif du dossier : nombre de fichiers, somme des tailles, date la plus récente). `mtime`
  = secondes entières depuis 1970 UTC, tronquées (la valeur du scan, `game.mtime_unix`). Comparaison du journal au
  disque : taille égale et dates à **2 s** près. C'est le « inchangés (taille + date) » du §4.6 ; à la reprise, la
  date ne prouve jamais que deux fichiers sont identiques (fichier physique ou taille + CRC32, §4.5), elle sert
  seulement à ne pas rapatrier un fichier que le journal ne prouve pas avoir posé.
- **Section d'annulation.** L'annulation écrit **dans le même journal**, après `E`, des blocs `B u` … `C u` aux
  déplacements inversés, puis `U`. La règle de reprise du §4.5 (« `B` sans `C` ») s'y applique telle quelle : une
  unité dont le retour a été interrompu est remise à son emplacement rangé, puis `U` est posé. Une seule section
  d'annulation par journal : après une annulation partielle (unité modifiée, place d'origine occupée, interruption,
  coupure), les unités restées rangées le restent.
- **Identités.** Tout marqueur de clôture (`E`, `U`) rend caduc le `I` antérieur : les identités sont dues tant
  que la dernière ligne du journal n'est pas `I`. La liste rejouée est **nette** — une paire (chemin périmé →
  chemin actuel) par fichier d'une unité validée, dans le sens de l'aller si l'unité est restée rangée, inverse
  si elle est revenue — donc juste que le `I` de l'aller ait été posé ou non.
- **Journal strict.** Toute ligne hors grammaire (déplacement hors d'un bloc, `C` qui ne ferme pas le `B` ouvert,
  numéro dupliqué, assemblage mal nommé, destination hors de l'assemblage, inverse qui n'est pas celui de
  l'aller…) fait refuser le journal **entier** : rien n'est rejoué ni complété. Un journal en cours ou illisible
  bloque tout nouveau rangement et toute annulation ; un fichier vide ou à en-tête coupé vaut « rien n'a bougé ».
  Le journal est créé au premier `B` ; l'aller est clos avant 7 Mio pour que son annulation tienne sous la borne
  de lecture de 16 Mio ; la purge des 20 derniers ne supprime qu'un journal dont la dernière ligne est `I`.
- **Précisions RV1** (revue du contrat, 2026-11-22 ; détail : design §3.4.1).
  1. Le journal refuse tout `M` / `W` dont `de` et `vers` sont égaux après pliage de casse et normalisation
     Unicode ; sous Unix, la reprise exige `nlink ≥ 2` avant de retirer une destination « identique » à sa
     source. Un seul fichier vu sous deux noms n'est jamais retiré.
  2. Si la défaite d'un bloc échoue elle-même (erreur d'E/S), l'exécution s'arrête sans `E` ni `U` : le journal
     reste en cours et est repris au démarrage suivant ; `recover_tidy` rend une erreur sans poser de clôture.
  3. Nettoyage d'un assemblage défait : les `D+` du plus profond au moins profond (cherchés aux deux
     emplacements, comme les `M`), puis `S`, puis `<D>` ; « non vide » est silencieux pour un `D+`.
  4. Un bloc peut écrire `D+` pour les ancêtres de `<D>` qu'il crée réellement ; un `D+` n'est écrit que par le
     bloc qui crée vraiment le dossier à l'exécution, jamais d'après le plan.
  5. Chaque ligne du journal est synchronisée (`m`, `w`, `C`, `E`, `U`, `I` compris) ; le dossier source l'est
     après le retrait, avant `C`. Une annulation interrompue devient **définitive** (l'unité est remise rangée,
     `U` posé) et le rapport de reprise le dit. Un journal plus récent d'une autre racine bloque l'annulation.
- **Limites assumées.** Sous Windows, un fichier plus gros que la borne de hachage, coupé entre le lien et le
  retrait de sa source, ne peut pas être prouvé identique : il reste en double à la destination, signalé, jamais
  retiré d'office. Sur FAT / exFAT (unités `W` seulement, les autres y sont bloquées), un changement d'heure
  peut décaler les dates d'une heure : l'unité n'est alors pas annulée (elle reste rangée, signalée). Deux
  instances qui posent `I` et annulent en même temps ne sont pas départagées au-delà du verrou de chaque appel
  (limite « seconde instance » ci-dessous).
- **Décisions après la revue FV1a** (2026-11-22 ; rapport `.claude/fv1a-7.1.md`).
  1. **Identité physique (I3).** Dans la branche destructrice de la reprise et de la défaite (retirer un
     exemplaire « identique »), l'identité de deux exemplaires est prouvée par le crate `same-file` (déjà
     transitif via `walkdir`, il devient dépendance directe d'`emu-library` ; l'implémenteur vérifie sa licence),
     avec en plus le garde `canonicalize(src) != canonicalize(dst)`. La borne de 512 Mio (taille + CRC32)
     disparaît pour cette branche. Si un fichier ne peut pas être ouvert, le cas est signalé et rien n'est retiré.
  2. **Windows, `sync_dir`** (m4) : sans effet, acceptable sur NTFS. Sur FAT / exFAT, un renommage de dossier
     (unités `W`) n'est ni atomique ni ordonné : limite assumée, à ajouter à celles ci-dessus.
  3. **`fold_path`** : le pliage de casse et d'Unicode est volontairement plus grossier que NFC (`é` == `e`) et
     incomplet pour certains alphabets. Il va dans le sens sûr : refus ou `Conflict`, jamais fusion.
     `Pokémon.gb` et `Pokemon.gb` visant le même dossier restent non rangés, le message montre le fichier existant.
  4. **Fichier ouvert par un autre programme** (Windows) : violation de partage, donc `Fatal` ; le journal reste
     en cours jusqu'à la fermeture du fichier, puis reprise. L'UI doit l'expliquer.
  5. **Sur `Fatal`**, le `TidyReport` partiel est perdu : l'UI enchaîne `recover_tidy` puis
     `pending_tidy_identities`.

### 5. Cores améliorés par un fichier d'origine

- **Source de l'information**, sans rien télécharger : (a) *dérivée* — tout `required_firmware."<système>"` de
  `cores.toml` qui déclare un `fallback` (PlayStation ×3, 3DO, Amiga) : le core tourne avec un remplaçant, le
  fichier d'origine l'améliore ; (b) *déclarée* — nouvelle clé `optional_firmware."<système>"` de `cores.toml`
  (liste `{ file, benefit }`), tenue à la main et courte (BIOS GBA de mgba, ROM de démarrage Game Boy, ROM MT-32 de
  dosbox_pure…), **inerte** si le `.info` installé du core ne liste pas ce fichier. Les `.info` seuls ne suffisent
  pas : `firmwareN_opt` ne distingue pas « améliore » de « requis pour certains contenus ». md5 de référence :
  `firmware.toml`, sinon `.info`. Un fichier **requis** sans repli (BIOS Mega CD) garde son dialogue bloquant.
- **Affichage permanent** : écran Cores, ligne du core — « Peut être amélioré par un fichier d'origine », avec le
  fichier, son état (présent et vérifié / absent / md5 inattendu), le bénéfice et « Importer un BIOS… » (3.0).
- **Avertissement, une fois, non bloquant** : au premier lancement d'un jeu concerné, une fois par couple
  (core, fichier) — mémorisé dans `[interface] firmware_hints_seen` de `config.toml`. Type *dérivé* : le dialogue
  de la 7.0 (« Importer un BIOS… » / « Continuer »), qui passe d'« une fois par jeu » à « une fois par core et
  fichier » (**confirmé par l'utilisateur** le 2026-11-21) ; un jeu déjà averti (`GameInput::aros_acknowledged`
  de ses options) ne l'est pas de nouveau et sa clé est ajoutée. Type *déclaré* : simple bandeau d'information
  dans la bibliothèque, refermable. Le texte dit toujours : « emu-rust ne télécharge jamais ce fichier ; il doit
  provenir de votre propre machine. »

## Conséquences

- `unsafe` nouveau : **un seul appel** (`MessageBoxW`, `emu-runtime::dialog`) ⇒ revue `reviewer`. `windows-sys`
  **0.61.2** (version déjà au `Cargo.lock`, aucune autre) devient une dépendance directe d'`emu-runtime` sous
  `cfg(windows)`. `tracing-subscriber` et `png` : dépendances d'`emu-ui`. `winresource` : dépendance de build
  d'`emu-app` sous Windows. `emu-app` devient une bibliothèque + deux binaires minces. Toutes ces lignes de
  `Cargo.toml` / `Cargo.lock` sont posées par la tâche de contrat (FX0), jamais par les tâches parallèles.
- Le paquet Windows contient deux exécutables ; `release.yml` vérifie `emu-rust-cli.exe --version` (code de retour
  compris) et le sous-système de chacun.
- Le rangement écrit dans le dossier de l'utilisateur : première fonction d'emu-rust qui déplace ses ROMs. Les
  garde-fous (aperçu, jamais d'écrasement ni de suppression de données, journal écrit avant d'agir, unité
  indivisible, annulation, reprise) sont le contrat ; `reviewer` relit le **format du journal avant** que
  l'exécution soit écrite, puis `emu-library/src/tidy/` comme du code sensible.
- Limites assumées : pas de rangement vers un autre volume ni sur un système de fichiers sans liens physiques
  (unité bloquée, à ranger à la main — à revoir si l'essai le demande) ; `roms/<id>` lié ou monté ailleurs :
  bloqué ; une **seconde instance** d'emu-rust en train de jouer n'est pas exclue (le verrou n'exclut qu'un autre
  rangement) : ne pas ranger pendant qu'une autre fenêtre joue ; un état sauvegardé d'un core qui mémorise le
  chemin absolu du média (cores en `media_direct`) peut demander de réinsérer le disque après rangement — à
  mesurer par `qa-systems`, à écrire dans le guide d'essai ; sous Windows un fichier ouvert par un autre programme
  fait échouer son unité ; le lancement avec console n'est vérifié que par l'essai de l'utilisateur.
- Les jeux à la racine de `roms/` de l'utilisateur (ses consoles) gardent favoris, historique, jaquettes,
  sauvegardes, états et options après rangement, et après annulation.
- Feuille de route : ligne 7.1 et tableau des schémas mis à jour (« 7.1 : aucune migration »).
