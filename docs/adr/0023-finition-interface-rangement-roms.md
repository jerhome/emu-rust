# ADR 0023 — Version 7.1 : finition de l'interface, console cachée, rangement de `roms/`, fichiers d'origine

Statut : **proposé** (2026-11-21, architecte). Périmètre : `docs/specs/v7.1-v8.0-utilisateur.md` §7.1 (fait
autorité ; interprétations validées par l'utilisateur le 2026-11-21 : point 5 = BIOS / firmware d'origine fourni par
l'utilisateur, jamais téléchargé ; rangement avec aperçu et annulation ; icône : variantes à choisir). Précise
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
  `AttachConsole` / `AllocConsole` de Windows sont des appels FFI. `windows-sys` est déjà dans `Cargo.lock`.
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

- **Une seule image**, source SVG `packaging/icon/emu-rust.svg` (formes pleines, sans dégradé, sans texte, sans
  marque ni manette d'un fabricant). Quatre variantes proposées dans `docs/design/icone/` : **le choix revient à
  l'utilisateur** ; défaut de travail = variante B.
- **Fichiers générés et commités** (une compilation n'a besoin d'aucun outil graphique) par `scripts/gen-icons.sh` :
  PNG 16 à 1024 px, `emu-rust.ico` (16, 24, 32, 48, 64, 256), `emu-rust.icns`, copies pour `emu-ui`. Rasterisation
  par `rsvg-convert` (image Docker d'outillage, version épinglée) ; `.ico` par ImageMagick ; `.icns` par un script
  Python sans dépendance (conteneur de PNG). Le script écrit `packaging/icon/SOURCE.sha256` (empreinte du SVG) ; la
  CI vérifie seulement que cette empreinte correspond au SVG commité (un rendu identique au bit près d'une version
  d'outil à l'autre n'est pas exigé).
- **Windows** : icône et informations de version dans l'exécutable par une ressource, posée par `build.rs`
  d'`emu-app` avec `winresource` (MIT, `build-dependencies` sous `cfg(windows)`, outil `rc.exe` du runner MSVC).
  Compilation croisée depuis Linux : pas de ressource, pas d'échec. **macOS** : `Contents/Resources/emu-rust.icns`
  + `CFBundleIconFile`. **Linux** : `share/icons/hicolor/<taille>/apps/emu-rust.png`, `scalable/apps/emu-rust.svg`,
  `share/applications/emu-rust.desktop` (`Terminal=false`, `StartupWMClass=emu-rust`) dans le `tar.gz`.
- **Fenêtre** (toutes les fenêtres : jeu, bibliothèque, cores) : `with_window_icon` de `winit` à partir des PNG
  embarqués (32, 64, 256 ; décodés par `png`, déjà au workspace) — barre de titre et barre des tâches Windows, X11
  (`_NET_WM_ICON`, donc noVNC). Wayland n'a pas d'icône de fenêtre : `app_id` = `emu-rust` pour que le `.desktop`
  s'applique. macOS ignore l'icône de fenêtre (Dock = `.icns`). Aucune dépendance nouvelle à l'exécution.

### 3. Console cachée, journal dans l'interface

- **Windows** : `#![cfg_attr(windows, windows_subsystem = "windows")]` sur `emu-app` (aussi en debug : un seul
  comportement à tester). Au démarrage, dans cet ordre :
  1. sorties standard déjà valides (redirection vers un fichier ou un tube : CI, `emu-rust --version > f`) ⇒ rien ;
  2. sinon `AttachConsole(ATTACH_PARENT_PROCESS)` : lancé depuis `cmd` / PowerShell, les sous-commandes (`--help`,
     `--version`, `cores …`, `--check-bios`) écrivent dans le terminal appelant. Limite connue de Windows : l'invite
     n'attend pas un programme graphique (`start /wait emu-rust …` pour un script) ;
  3. sinon, **modes fenêtre seulement**, si `[interface] console = true` : `AllocConsole()` (nouvelle fenêtre de
     console). Le réglage est lu au démarrage : **il prend effet après redémarrage** (dit dans les Réglages).
  Ces appels FFI vivent dans `emu-runtime` (module `console`, `// SAFETY:`, revue `reviewer`), derrière une API
  sûre réexportée par `emu-ui` ; `emu-app` reste sans `unsafe`. Erreur fatale avant l'ouverture d'une fenêtre, sans
  console : boîte `MessageBoxW` (même module) en plus du journal.
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
  (crochet de panique) : sans console c'est la seule trace d'un plantage. Mode `0600` sous Unix.
- **Panneau « Journal »** (pied de page, avec un compteur d'erreurs et d'avertissements non lus ; aussi dans le menu
  en jeu) : liste horodatée, filtre Erreurs / Avertissements / Tout, origine (application ou core), « Enregistrer
  sous… » et « Ouvrir le dossier des journaux » (pas de presse-papiers). Ce qui n'existait que sur le terminal y
  figure : racine des données, core et jeu chargés, messages du core, refus d'un core (`core_error`), conseils de
  `diagnostics`. Les messages déjà affichés (bandeaux, OSD) restent où ils sont.

### 4. Rangement de `roms/`

**4.1 Périmètre.** Réglages → « Dossier des ROMs » : (a) « Créer les dossiers des systèmes » crée `roms/<id>/` pour
chaque système du catalogue (17 aujourd'hui), sans rien déplacer ; (b) « Ranger les ROMs… » analyse, **montre un
aperçu**, puis déplace ce que l'utilisateur a laissé coché. Seul `<data>/roms` est rangé (les dossiers externes de
`[library] roots` ne sont jamais touchés). Est « déjà rangé » tout élément dont le chemin relatif commence par le
dossier de **son** système. Le rangement **déplace, ne renomme jamais, ne supprime jamais, n'écrase jamais, ne
décompresse jamais** ; il ne suit aucun lien symbolique (un lien est laissé en place et signalé).

**4.2 Unités.** L'analyse regroupe les fichiers en unités déplacées en bloc :

| Unité | Contenu | Destination |
|---|---|---|
| Fichier seul | ROM de console, `.zip`, image unique (`.chd`, `.iso`, `.pbp`, `.adf`, `.st`, `.d64`…) | `roms/<id>/<nom>` |
| Jeu à plusieurs disquettes | groupe `(Disk N of M)` (`group_media`), mêmes règles que le scan | `roms/<id>/` (à plat, côte à côte) |
| CD à plusieurs fichiers | `.cue` + pistes référencées (`cue_references`) + `.sbi` de même nom ; `.m3u` + ses disques et leurs pistes | `roms/<id>/<nom du .cue ou du .m3u>/` (un dossier par jeu : les pistes de jeux différents portent souvent le même nom) |
| Jeu-dossier DOS | dossier avec un `.exe` / `.com` / `.bat` à sa racine | `roms/dos/<dossier>/` (le dossier entier) |
| Dossier quelconque | ni CD ni DOS | ses fichiers sont traités un par un ; le dossier vidé est retiré (noté au journal) |

Un `.cue` ou un `.m3u` dont une référence est absolue, sort de son dossier, ou manque ⇒ unité **bloquée**
(expliquée dans l'aperçu), rien n'est déplacé pour elle. Les fichiers qui ne sont pas des jeux (textes, images,
BIOS déposés par erreur) restent en place ; un fichier dont le md5 est celui d'un BIOS connu est signalé avec
« Importer un BIOS… ».

**4.3 Système de destination.** Même ordre que le scan, appliqué « comme si aucun dossier ne décidait » :
(1) sondes d'en-tête (`disc_info`, `probe_bin`, iNES, en-tête Mega Drive / 32X…) ; (2) empreinte dans un DAT importé
(CRC32 / SHA1, série, piste 01) ; (3) contenu de l'archive (`.zip` : extensions des entrées ; une image CD dans un
zip reste refusée, règle 2.1) ; (4) nom présent dans **un seul** DAT ; (5) extension propre à un seul système.
Résultat par unité :

- **Sûre** (1, 2, 3 ou 5) : cochée d'office.
- **Suggérée** (4 ; jeu-dossier DOS ; zip pris pour du DOS) : montrée **décochée**, avec la raison.
- **Ambiguë** (plusieurs systèmes possibles : `.chd`, `.m3u`, `.dsk`, `.bin`, `.gb` sans DAT…) : l'aperçu propose
  la liste des systèmes candidats ; tant que l'utilisateur ne choisit pas, l'unité reste en place.
- **Inconnue** : laissée en place.

Une fois sous `roms/<id>/`, la règle de dossier du scan fait foi : c'est ce qui rend lançable un fichier jusque-là
« non classé ». Les alias de dossier (`ps1/`) restent refusés.

**4.4 Collisions.** Destination déjà occupée (comparaison insensible à la casse, sur le disque **et** entre unités
du plan) : même taille et même CRC32 ⇒ **doublon**, sinon **conflit** ; dans les deux cas l'unité reste en place,
signalée. Aucun renommage automatique : le `save_name` dépend du nom du fichier.

**4.5 Exécution.** Tâche de fond annulable (entre deux unités), refusée pendant un scan, un téléchargement ou une
partie ; verrou inter-processus `<data>/rangement/.lock`. Par unité :

1. ligne d'intention au **journal** (`<data>/rangement/journal-<unix>.txt`, texte, une opération par ligne, chemins
   relatifs à `roms/` échappés comme le registre 2.1, `sync_all` avant tout déplacement) ;
2. déplacement : la destination est revérifiée absente (`symlink_metadata`), puis `fs::rename` (même volume :
   atomique). `ErrorKind::CrossesDevices` (`roms/<id>` monté ou lié ailleurs) ⇒ copie vers
   `<destination>.emu-rust-part`, `sync_all`, vérification taille + CRC32, `rename`, puis suppression de la source ;
   une copie ratée (disque plein) est retirée, la source reste intacte. Un jeu-dossier DOS sur un autre volume est
   copié fichier par fichier de la même façon ;
3. **bibliothèque**, dans une transaction : `game.path`, `game_media.path` et `game.pinned` de l'unité sont
   réécrits (`Library::relocate`) — la ligne est conservée, donc favori, historique, identification, région et
   jaquettes aussi ;
4. ligne « fait ». Un échec en 2 ou 3 remet en place les fichiers déjà déplacés de l'unité ; les autres unités
   continuent ; tout figure au rapport.

Après la dernière unité : (5) `emu_runtime::relocate_identities` réécrit le chemin des sources dans les registres
`<saves>/<core_id>/.emu-rust-games` et les manifestes `.disks` (sous leur verrou, écriture atomique ; un registre
illisible n'est pas touché : la règle d'empreinte de la 2.1 reste le filet) ; (6) scan incrémental, qui ajoute les
jeux nouvellement classés. **Aucun fichier de `saves/`, `states/`, `game-options/` ni `covers/` n'est déplacé ni
renommé** : ils sont nommés par le `save_name` ou rattachés à la ligne du jeu, qui ne changent pas.

Arrêt brutal : au prochain démarrage, avant le scan, un journal sans ligne de fin est **repris**
(`Library::recover_tidy`) : pour chaque intention, l'emplacement réel du fichier décide ; la base puis les registres
sont remis en accord (opérations idempotentes) ; un `.emu-rust-part` orphelin est supprimé.

Filet de sécurité indépendant : le **scan** reconnaît désormais un fichier déplacé (ligne dont le chemin a disparu
+ fichier nouveau de mêmes système, taille et empreinte, ou mêmes nom, taille et date pour un fichier non haché) et
réécrit la ligne au lieu de la remplacer ; cela protège aussi les déplacements faits à la main.

**4.6 Annulation.** « Annuler le dernier rangement » (Réglages, et bouton du rapport) rejoue le dernier journal à
l'envers : chaque fichier encore présent à sa destination, inchangé (taille + date), revient à son emplacement
d'origine si celui-ci est libre ; dossiers créés retirés s'ils sont vides, dossiers vidés recréés ; mêmes mises à
jour de la base et des registres en sens inverse. Ce qui ne peut pas revenir (fichier modifié, supprimé, place
prise) reste où il est et figure au rapport. Le journal est alors marqué « annulé » ; les 20 derniers sont gardés.

**4.7 Schéma.** Aucune migration : le journal est un fichier, `relocate` n'utilise que les colonnes de la v6.

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
  fichier » ; les jeux déjà avertis en 7.0 ne le sont pas de nouveau. Type *déclaré* : simple bandeau d'information
  dans la bibliothèque, refermable. Le texte dit toujours : « emu-rust ne télécharge jamais ce fichier ; il doit
  provenir de votre propre machine. »

## Conséquences

- `unsafe` nouveau dans `emu-runtime` (console et boîte de message Windows) ⇒ revue `reviewer` ; `windows-sys`
  devient une dépendance directe d'`emu-runtime` sous `cfg(windows)` (déjà au `Cargo.lock`). `tracing-subscriber`
  devient une dépendance d'`emu-ui`. `winresource` : dépendance de build d'`emu-app` sous Windows. Rien d'autre.
- Le rangement écrit dans le dossier de l'utilisateur : première fonction d'emu-rust qui déplace ses ROMs. Les
  garde-fous (aperçu, jamais d'écrasement ni de suppression, journal écrit avant d'agir, annulation, reprise) sont
  le contrat ; `reviewer` relit `emu-library/src/tidy*.rs` comme du code sensible.
- La fenêtre entre « destination vérifiée absente » et `rename` n'est pas atomique (pas de `RENAME_NOREPLACE` en
  Rust sûr) : acceptée, le verrou exclut un second emu-rust.
- Limites assumées : pas de contrôle d'espace libre avant une copie entre volumes (l'échec est propre) ; un état
  sauvegardé d'un core qui mémorise le chemin absolu du média (cores en `media_direct`) peut demander de réinsérer
  le disque après rangement — à mesurer par `qa-systems`, à écrire dans le guide d'essai ; sous Windows un fichier
  ouvert par un autre programme fait échouer son unité ; les sous-commandes lancées depuis `cmd` n'attendent pas.
- Les jeux à la racine de `roms/` de l'utilisateur (ses consoles) gardent favoris, historique, jaquettes,
  sauvegardes, états et options après rangement, et après annulation.
- Feuille de route : ligne 7.1 et tableau des schémas mis à jour (« 7.1 : aucune migration »).
