# Questions ouvertes

Questions et décisions **non bloquantes** : une décision par défaut a été prise pour avancer. L'utilisateur peut la
remettre en cause à tout moment. Les questions bloquantes lui sont notifiées directement.

| Date | Phase | Question | Décision par défaut | Statut |
|---|---|---|---|---|
| 2026-11-19 | 1 | Correspondance des boutons Mega Drive dans genesis_plus_gx : A et Start scriptés n'ont pas ouvert de sous-menu dans le test d'entrées. | Vérifié expérimentalement : libretro Y = MD A, B = B, A = C. `KeyMap::genesis_default()` était déjà correct (Z = A, X = B, C = C, Entrée = Start) ; c'est le script du test qui pressait libretro A (= MD C). Test corrigé. Mapping 6 boutons non vérifié (core en 3 boutons par défaut). | résolu (1b) |
| 2026-11-19 | 1b | Outillage perso (Rust, X11/GL, Mega Drive) embarqué dans l'image commune de claude-sandbox. | Noté dans claude-sandbox (PR #3, mergée) ; à traiter plus tard. | reporté |
| 2026-11-19 | 1b | Config ALSA dans l'image runtime : statique (`/etc/asound.conf`) ou conditionnelle ? | Conditionnelle : `entrypoint.sh` écrit `~/.asoundrc` (défaut -> pulse) seulement si `PULSE_SERVER` est défini ; sans serveur (noVNC) l'ouverture ALSA échoue vite (pas de blocage), l'app doit traiter l'absence de périphérique audio comme « pas de son ». | décidé |
| 2026-11-19 | 1b | Rééchantillonneur : crate (`rubato`, sinc) ou maison ? | Linéaire maison (`StereoResampler`), sans dépendance ; `rubato` en phase 2 si la qualité ne suffit pas. | décidé (défaut) |
| 2026-11-19 | 1b | Latence audio cible. | 64 ms (consigne de remplissage, anneau = 2×), DRC ±0,5 % ; réglable via `AudioConfig`. | décidé (défaut) |
| 2026-11-19 | 1b | Son en mode noVNC ? | Non (ADR 0004) : l'entrypoint passe `--no-audio` en noVNC. | décidé (défaut) |
| 2026-11-19 | 1b | Backend Linux de `cpal` : ALSA (+ plugin pulse) ou feature `pulseaudio` native (pur Rust, cpal 0.18) ? | ALSA par défaut, déjà câblé dans l'image (`libasound2-plugins`, `.asoundrc`) ; feature `pulseaudio` à réévaluer si WSLg pose problème. | décidé (défaut) |
| 2026-11-19 | 1b | `audio-cpal` activée par défaut ? | Oui depuis l'ajout de `libasound2-dev` au conteneur de Claude ; `--no-default-features` donne un binaire silencieux. | décidé (défaut) |
| 2026-11-19 | 1b | sound-check et --no-audio | `sound-check` refuse de tourner sans PULSE_SERVER/socket (message clair) ; `--no-audio` ajouté par l'entrypoint en mode noVNC uniquement. | décidé |
| 2026-11-19 | 6 | Licence du dépôt public `jerhome/emu-rust` : aucune pour l'instant (= tous droits réservés). | `MIT OR Apache-2.0`, validé par l'utilisateur. | résolu |
| 2026-11-19 | 2 | Thread d'émulation dédié ? | Non : tout sur le thread principal, sync pilotée par l'audio (ADR 0010) ; compatible HW render phase 7. | décidé (défaut) |
| 2026-11-19 | 2 | `GET_LOG_INTERFACE` : cale C ou `false` ? | Cale C (`cc`) + core factice C pour les tests (ADR 0009) ; un compilateur C devient requis au build. | décidé (défaut) |
| 2026-11-19 | 2 | Emplacement de `config.toml` : racine de données ou dossier de config XDG ? | Racine de données (ADR 0006/0011), un seul volume Docker ; racine `--data-dir` > `EMU_DATA_DIR` > `directories`. Le défaut relatif `./data` disparaît. | décidé (défaut) |
| 2026-11-19 | 2 | Échap : quitter (phase 1) ou menu ? | Menu (pause) ; quitter via le menu ou la fermeture de fenêtre. Raccourcis : F2/F4 état, F6/F7 emplacement, Espace maintenu = avance rapide (4×, muet), P pause, H reset, F11 plein écran, F3 stats ; modifiables dans `config.toml` (pas encore dans l'UI). | décidé (défaut) |
| 2026-11-19 | 2 | Savestates : format RetroArch ou maison ? | Conteneur maison `EMURSTAT` (en-tête + somme), 10 emplacements, pas d'état auto à la sortie ; SRAM `.srm` brute (compatible RetroArch). | décidé (défaut) |
| 2026-11-19 | 2 | Fréquence d'écriture de la SRAM. | Si modifiée : toutes les 10 s, à l'ouverture du menu, à la sortie (écriture atomique). | décidé (défaut) |
| 2026-11-19 | 2 | Vsync par défaut ? | Activée (configurable) ; à confirmer sous WSLg lors de l'essai E1. | décidé (défaut) |
| 2026-11-19 | 2 | Profils de touches : par système ou par core ? | Par `core_id` (+ profil `default`) tant que la bibliothèque (phase 4) ne connaît pas les systèmes. | décidé (défaut) |
| 2026-11-19 | 2 | Zip à plusieurs entrées jouables ; taille maximale. | Première entrée acceptée dans l'ordre de l'archive (+ `warn!`) ; refus au-delà de 512 Mio. | décidé (défaut) |
| 2026-11-19 | 2 | Options de core : `.opt` par core (RetroArch) ou `config.toml` ? | `config.toml`, section `core_options.<core_id>`. | décidé (défaut) |
| 2026-11-19 | 2 | Volumes Docker : un par catégorie (saves/states/config) ou un seul ? | Un seul volume `emu-data` sur `/data` (ADR 0011 : tout sous la racine de données) ; `EMU_DATA_DIR=/data` répété dans chaque service compose ; entrypoint crée les sous-dossiers et force `config.toml` en 0600. | décidé (défaut) |
| 2026-11-19 | 2b | Runner macOS Intel : `macos-13` (demandé) ? | `macos-13` est retiré par GitHub (déc. 2025) ; CI utilise `macos-15-intel` (disponible jusqu'à ~août 2027, ensuite plus de runner Intel : cross-compilation depuis arm64). | décidé (défaut) |
| 2026-11-19 | 2b | Tests CI Linux : sous `xvfb-run` ? | Oui, par prudence (tests touchant à un affichage X) ; coût négligeable. | décidé (défaut) |
| 2026-11-19 | 2b | Artefact macOS : binaire nu ou `.app` ? | Binaire nu pour l'instant (TODO dans `ci.yml`) ; bundle `.app` en phase 2b suite. | décidé (défaut) |
| 2026-11-19 | 2 | Touche « menu » (Échap) avant l'interface egui (U2) ? | Quitte l'application (placeholder) ; devient ouverture du menu en U2. `--screen` sans egui : avertissement, capture de l'image du jeu seule. Depuis U2 : ouvre le menu ; quitter = « Quitter » du menu ou fermeture de la fenêtre (Échap quitte encore si egui n'a pas pu s'initialiser). | décidé (défaut) |
| 2026-11-19 | 2 | `config.toml` invalide : comportement à l'écriture ? | `Config::load_or_default` renvoie des valeurs par défaut « verrouillées » ; `Config::save` refuse (`ConfigError::Locked`) : le fichier de l'utilisateur n'est jamais écrasé. Une touche de raccourci inconnue invalide tout le fichier ; une liaison de jeu inconnue est seulement ignorée. | décidé (défaut) |
| 2026-11-19 | 2 | Pause/avance rapide : effets sur l'audio et le core. | Pause : SRAM flushée, entrées relâchées, audio en pause. Avance rapide (maintenue) : audio en pause, `set_fast_forwarding(true)`, ratio `speed.fast_forward_ratio` ; la pause l'emporte. | décidé (défaut) |
| 2026-11-19 | 6 | Noms réservés Windows (CON, NUL, COM1, LPT1…) dans les noms de jeu / d'entrées zip. | À filtrer **avant la release 0.1** (build Windows avancé, ADR 0012) : assainisseur partagé, suffixe `_` (tâche R3, `docs/design/release-0.1.md`). | planifié (0.1) |
| 2026-11-19 | 7 | Zip contenant plusieurs fichiers liés (`.cue` + `.bin`) : seule la première entrée acceptée est extraite. | Non géré ; à traiter avec les systèmes CD (extraction complète dans le dossier temporaire). | reporté |
| 2026-11-19 | 2 | Options du core et emplacement : `ComboBox` (conception) ou ligne à valeur ? | Ligne à valeur `< valeur >` : Gauche/Droite (ou Entrée) changent la valeur, pas de liste déroulante ; plus simple au clavier et à la manette. | décidé (défaut) |
| 2026-11-19 | 2 | Écran des touches : profil édité par défaut ; plusieurs touches par bouton ? | Profil « ce core » à l'ouverture (créé depuis la disposition effective au premier changement) ; une réassignation remplace les touches du bouton (une touche par bouton depuis l'UI) ; touche déjà liée ailleurs ⇒ déplacée avec message de conflit ; raccourcis refusés ; Retour arrière efface ; config écrite en quittant l'écran. | décidé (défaut) |
| 2026-11-19 | 2 | Taille de fenêtre initiale trop petite pour le menu (ex. core 64×48). | Échelle augmentée par pas entiers jusqu'à 640×480 minimum (bornée à 16). | décidé (défaut) |
| 2026-11-19 | 2 | Date des savestates dans le menu : heure locale ? | UTC (pas de crate de date/fuseau) ; affichée « (UTC) ». | décidé (défaut) |
| 2026-11-19 | 2 | Menu : après « Charger l'état » / « Réinitialiser » ; écran Vidéo/Audio. | Charger (réussi) et Réinitialiser referment le menu ; Sauvegarder le laisse ouvert. Écran Vidéo/Audio (vsync, échelle, synchro) non fait en U2 : réglages via `config.toml`. | décidé (défaut) |
| 2026-11-19 | 2 | `config.toml` invalide : comment lever le verrou ? | Bouton « Remplacer config.toml par les réglages actuels… » avec confirmation, dans les écrans Touches et Options. Clés inconnues conservées à l'écriture (`extra` à chaque niveau). | décidé (défaut) |
| 2026-11-19 | 2 | Captures `--screen` sous Xvfb sans gestionnaire de fenêtres : la fenêtre n'a jamais le focus, egui n'affiche alors pas le focus clavier. | En mode capture, l'UI considère la fenêtre comme focalisée (événement `Focused(true)` injecté dans egui). Animations egui désactivées (rendu immédiat, captures déterministes). | décidé (défaut) |
| 2026-11-19 | 0.1 | Toolchain Windows : MSVC ou GNU (cross) ? | MSVC sur `windows-2025` (toolchain de référence de Rust, `cc` → `cl.exe`, pas de DLL MinGW à livrer) ; GNU cross réévalué si un build local devient nécessaire. | décidé (défaut) |
| 2026-11-19 | 0.1 | Windows : `windows_subsystem = "windows"` ? | Non en 0.1 : sous-système console (lancement CLI, logs stderr visibles) ; à revoir avec un lanceur graphique + fichier de log (phase 3/4). | décidé (défaut) |
| 2026-11-19 | 0.1 | Lancement sans arguments (double-clic sur le `.app` / l'`.exe`) | Affiche l'usage et quitte ; 0.1 = ligne de commande, documenté dans `README.txt`. Sélecteur de fichiers : phase 3/4. | décidé (défaut) |
| 2026-11-19 | 0.1 | macOS : signature / notarisation ? | Signature ad hoc seulement, non notarisé (pas de compte Apple Developer) ; contournement Gatekeeper documenté (`xattr -dr com.apple.quarantine`). | décidé (défaut) |
| 2026-11-19 | 0.1 | macOS : version minimale | `LSMinimumSystemVersion` / `MACOSX_DEPLOYMENT_TARGET` = 10.13 ; à relever si une dépendance l'exige (version du Mac de l'utilisateur à confirmer lors de l'essai). | décidé (défaut) |
| 2026-11-19 | 0.1 | Runner du binaire Linux de release | `ubuntu-22.04` (glibc 2.35) pour tourner sur plus de distributions ; la CI reste sur 24.04. | décidé (défaut) |
| 2026-11-19 | 0.1 | Licences des dépendances dans les archives | `THIRD-PARTY-LICENSES.html` généré par `cargo-about` (version épinglée) en plus de `LICENSE-MIT` / `LICENSE-APACHE`. | décidé (défaut) |
| 2026-11-19 | 0.1 | Release GitHub : brouillon, pré-release ou finale ? | Pré-release publiée tant que la version est `0.x` ; notes extraites de `CHANGELOG.md`. | décidé (défaut) |
| 2026-11-19 | 0.1 | Image Docker : plateformes et visibilité | `linux/amd64` seulement ; tags `0.1.0`, `0.1`, `latest` ; paquet GHCR créé privé au premier push : l'utilisateur le passe en public une fois. | décidé (défaut) |
| 2026-11-19 | 0.1 | Test SRAM de la checklist | La 240p Test Suite n'a pas de SRAM : test manuel facultatif (jeu à pile fourni par l'utilisateur) ; couverture par les tests du core factice. | décidé (défaut) |
| 2026-11-19 | 0.2+ | Contenu entre 0.1 et 1.0 : intégrer des « petites consoles » (NES, Master System/Game Gear, Game Boy/Color, PC Engine…) ? | Décidé par l'utilisateur : Game Boy, NES, Game Gear, Master System (pour le moment), dans la 1.0. | résolu |
| 2026-11-19 | 0.1 (P1) | Cache de build dans `release.yml` ? | Aucun `rust-cache` dans les jobs de release (build propre, pas de cache partagé avec d'autres refs) ; seul le cache GHA de buildx (portée `runtime`, écrit par ce workflow) est gardé pour l'image Docker. | décidé (défaut) |
| 2026-11-19 | 0.1 (P1) | `workflow_dispatch` avec `dry_run: false` ? | Accepté seulement sur une ref de tag (relance d'une publication ratée) ; si la release existe déjà, les assets sont remplacés (`--clobber`), les notes gardées. En dry run, l'image Docker est construite sans push. | décidé (défaut) |
| 2026-11-19 | 0.1 (P1) | Licences tierces acceptées (`about.toml`). | MIT, MIT-0, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0 (`option-ext`, via `directories`), OFL-1.1 + Ubuntu-font-1.0 (polices egui) ; cibles Linux/macOS/Windows, crates du workspace exclus. `cargo-about` 0.9.3 s'installe avec `--features cli`. | décidé (défaut) |
| 2026-11-19 | 0.1 (P1) | macOS : version minimale. | `MACOSX_DEPLOYMENT_TARGET=10.13` au niveau des jobs macOS (CI et release, couvre rustc et `cc`) ; `scripts/check-macos-minos.sh` échoue si `minos` > 12.0 (Mac d'essai : 12.7.6). | décidé |
| 2026-11-19 | 0.1 (P1) | Test de fumée des artefacts. | Chaque job de release exécute `emu-rust --version` et exige la version du workspace dans la sortie. | décidé (défaut) |
| 2026-11-19 | 0.1 (P1) | `xvfb-capture.sh` : `--screen` d'emu-rust en mode xwd. | `--screen` après `--` bascule automatiquement en mode `--dump-window` (l'écran n'existe qu'en fin de `--frames`, l'app quitte aussitôt). | décidé |
| 2026-11-19 | 6 | Cache GHA pour l'image Docker publiée (le design en prévoyait un) ? | Pas de cache en release : `no-cache` et pas de `cache-to` à la publication ; cache GHA conservé en dry run seulement. | décidé |
| 2026-11-19 | 1.0 (bibl.) | Identification : `.rdb` libretro ou DAT clrmamepro ? | DAT No-Intro clrmamepro de `libretro-database/metadat/no-intro/`, téléchargés à l'exécution (jamais redistribués), importés en SQLite (ADR 0014). | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Game Boy Color : système à part ou fusionné avec Game Boy ? | Systèmes distincts `gb` / `gbc` (DAT, jaquettes et filtres séparés). | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Zip contenant plusieurs ROMs. | Un zip = un jeu = première entrée acceptée (même règle que `emu-runtime`). | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Base `library.sqlite` corrompue. | Quarantaine `library.sqlite.corrupt-<unix>` + base neuve + bandeau ; seuls favoris et historique sont perdus. | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | ScreenScraper exige des identifiants **développeur** en plus du compte utilisateur. | Le projet n'en publie pas : l'utilisateur saisit les deux (ou la source reste désactivée). À revoir si l'utilisateur obtient un accès dev. | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Choix de dossiers : sélecteur natif (`rfd`) ou champ texte ? | Champ texte (pas de dépendance GTK/portail) ; `rfd` réévalué après l'essai. | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Windows : double-clic ouvre aussi une console (sous-système console, ADR 0012). | Conservé en 1.0 ; passage à `windows_subsystem = "windows"` + fichier de log à décider après l'essai. | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Mise à jour des DAT. | Au premier scan (proposée) et sur bouton ; GET conditionnel (ETag), jamais en tâche de fond silencieuse. | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Jaquettes : types téléchargés. | Boîtier (`Named_Boxarts` / `box-2D` / `cover_big`) seulement ; captures et écrans titre plus tard. | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Profils de touches par système (prévu par ADR 0011 avec la bibliothèque) ? | Restent par `core_id` en 1.0 ; par système si l'essai le demande. | décidé (défaut) |
| 2026-11-19 | 1.0 (bibl.) | Catalogue des systèmes en double avec `emu-assets/data/cores.toml` (ADR 0013) ? | Jonction par le nom de base libretro ; extensions de scan dans `emu-library`, cores dans `emu-assets` ; test de cohérence dans `emu-ui`. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Intégrité des cores nightly (aucun hash ni signature en amont). | TLS + hôte fixe ; CRC32 de `.index-extended` (CRC du core extrait) ; SHA-256 enregistré et revérifié avant chaque chargement ; `.prev` + épinglage (ADR 0013). Canal `stable/<version>/` à hashes épinglés envisageable plus tard. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Core Game Boy / Color par défaut. | gambatte (léger, éprouvé, SRAM/RTC) ; sameboy (MIT, plus précis) puis mgba en alternatives ; surcharge `[cores.preferred] gb = "sameboy"`. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Core NES par défaut. | mesen (le plus précis) ; nestopia si la machine est lente (Mac Intel ancien), fceumm pour les mappers rares. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Vérifier les mises à jour des cores au démarrage ? | Non : réseau seulement sur action (« Vérifier les mises à jour », CLI) ; aucune mise à jour automatique. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Reprise des téléchargements interrompus (HTTP `Range`) ? | Non : zips de quelques Mio ; 3 tentatives puis recommencer à zéro ; état toujours cohérent (temporaire + renommage). | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Extraction zip partagée entre `emu-runtime` et `emu-assets` (crate feuille) ? | Non en 1.0 : extracteur dédié plus étroit dans `emu-assets` (noms jamais utilisés comme chemins), graphe inchangé ; à factoriser (ADR) si un troisième besoin apparaît. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | BIOS « requis » absent selon le `.info`. | Avertissement avec « lancer quand même » (les `.info` ne sont pas toujours exacts) ; aucun BIOS requis pour les systèmes 1.0. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Plusieurs instances d'emu-rust installant des cores en même temps. | Résolu (2026-11-20) : verrou de dossier inter-processus `<cores>/.lock` (`File::lock`, repli journalisé sans verrou si non géré), manifeste relu et fusionné sous verrou, temporaires orphelins supprimés après 1 h (ADR 0013, amendement). | résolu |
| 2026-11-19 | 1.0 (cores) | `--core` : identifiant ou chemin ? | Les deux : un `CoreId` valide sans séparateur ni point ⇒ core géré (vérifié) ; sinon chemin comme en 0.1. | décidé (défaut) |
| 2026-11-19 | 1.0 (plate-forme) | Licence `CDLA-Permissive-2.0` (webpki-root-certs, via rustls-platform-verifier) absente de `about.toml`. | Ajoutée aux licences acceptées (permissive, données de certificats) ; `cargo about` non installé ici, à confirmer en CI. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Core NES : mesen (ADR 0013, précision, GPL-3.0, moins maintenu) ou fceumm (stable, maintenu) ? | **Mesen** : stable depuis le correctif de l'ordre des setters (set_* après retro_init), 10/10 tests. fceumm en repli. Nestopia écarté : bug du core (mémoire non initialisée dans le filtre NTSC Blargg, actif par défaut) ; si un jour retenu, forcer `nestopia_blargg_ntsc_filter=disabled`. | décidé |
| 2026-11-19 | 1.0 (cores) | SameBoy annonce 2 097 152 Hz, refusé par le rééchantillonneur (MAX_RATE 1 MHz). | Game Boy : gambatte (ADR 0013) ; relever MAX_RATE ou décimer avant d'envisager SameBoy. | décidé (défaut) |
| 2026-11-19 | 1.0 (cores) | Crash pendant l'échange `<core>` ⇄ `.prev` (retour arrière). | Au redémarrage, la version courante est restaurée ; l'ancienne `.prev`, restée dans le temporaire `.<core>.tmp-*`, est supprimée par `CoreManager::open` après 1 h (`ORPHAN_AGE`) : cette version précédente est alors perdue. Accepté (cas rare, réinstallable). | décidé (défaut) |
| 2026-11-20 | 1.0 (bibl.) | Téléchargement des jaquettes actif par défaut ? | Non : `covers.download = false` par défaut (aucune requête réseau sans action de l'utilisateur) ; bouton « Activer le téléchargement des jaquettes » dans la barre d'état, et réglage dans les Réglages. | décidé (défaut) |
| 2026-11-20 | 1.0 (bibl.) | Limite mémoire des JPEG de jaquettes : 3 cycles de revue bloqués (divergences de lecture d'en-tête entre notre analyseur et zune-jpeg). | **Décision utilisateur** : budget calculé au pire cas uniquement à partir des valeurs de zune ; JPEG 4:4:4 au-delà d'~2590² refusé (sans gêne pour des jaquettes). | décidé (utilisateur) |
| 2026-11-20 | 1.0 (bibl.) | Zip dont l'entrée porte un chemin (`../x`, `dir/x`) : lister, refuser ? | Listé sous le nom de base de l'entrée (`entry`), titre tiré du nom de l'archive ; rien n'est extrait par le scan. Fichiers et entrées de 0 octet : ignorés (`ScanReport::ignored_empty`). | décidé (défaut) |
| 2026-11-20 | 1.0 (ui) | `App::build` (chargement du core + ROM, extraction zip) s'exécute sur le thread UI au lancement depuis la bibliothèque. | Figement possible d'1-2 s sur une grosse archive. Accepté (pas de régression de 0.1, jeu lancé après une action explicite). | accepté |
| 2026-11-20 | 1.0 (ui) | Le profil clavier `[input.keyboard.default]` s'applique-t-il à tous les systèmes ? | Non : seulement aux styles MegaDrive/Snes/Generic ; SMS/GG/GB/NES gardent leur disposition intégrée (boutons différents). Un profil propre au core reste prioritaire. | décidé (défaut) |
| 2026-11-20 | 1.0 (bibl.) | Windows : casse (`C:\Roms` / `c:\roms`) et préfixe `\\?\` d'une racine canonisée donnent des clés de chemin différentes en base. | Accepté pour la 1.0 (racine normalisée par composants depuis 1.0.0-rc.1) ; à revoir si les essais Windows le montrent (canonisation commune côté UI). | à surveiller |
| 2026-11-20 | 1.0 (ui) | Clavier 60 % : H (reset) trop proche de J ; raccourcis F/flèches inaccessibles. | Reset par défaut = F5 (aussi Ctrl+R et menu Échap) ; alternatives Ctrl+S/L/,/./F/I/R et Alt+Entrée ; P (pause) conservé (sans gravité, Échap pause aussi). Profils déjà enregistrés inchangés. | décidé (défaut) |
| 2026-11-20 | 1.0.0-rc.2 | Réouverture du son synchrone sur le thread d'émulation pendant une perte (à-coup d'image possible toutes les 2 s sous WASAPI tant que le périphérique est absent). | Accepté pour la rc ; à mesurer lors de l'essai Windows (débrancher le casque sans autre sortie). | à surveiller |
| 2026-11-20 | 1.0.0-rc.2 | `DeviceChanged` (nouvelle sortie par défaut alors que l'ancienne reste valide, ex. casque branché) n'est pas suivi. | Pas de bascule automatique tant que l'ancienne sortie fonctionne ; inchangé depuis la 1.0.0-rc.1. | reporté |
| 2026-11-20 | 2.0 | Ordre 2.0 / manette | **Utilisateur** : 2.0 Atari ST d'abord, manette en parallèle (T1/T2 dans `gamepad/`, T3 après U2). | décidé |
| 2026-11-20 | 2.0 | Touche hôte | **Utilisateur** : Ctrl droit (⌘ droit sur macOS). | décidé |
| 2026-11-20 | 2.0 | EmuTOS par défaut | **Utilisateur** : 192 Ko français (`etos192fr.img`, clavier ST AZERTY). | décidé |
| 2026-11-20 | 2.0 | Mode d'entrée par défaut d'un jeu ST | **Utilisateur** : **Clavier** (ses jeux ST sont surtout des point-and-click à la souris, un peu de clavier) ; Joystick par Hôte+J. ⇒ la souris (capture, précision, confort) est prioritaire dans la 2.0. | décidé |
| 2026-11-20 | 2.0 | Core, disquettes, noVNC, machine (design §9 q.1, 5, 7, 8) | Défauts de l'architecte : `hatari` (hatarib installable), copies de travail, souris sans capture accepté sous noVNC/WSLg, machine ST (STE par jeu). | défaut |
| 2026-11-20 | 6 | Questions manette (design phase6 §6) | Défauts recommandés : gilrs puis SDL3 en repli, pas de vibration, PS = raccourcis (Share en repli), Docker sans garantie, autres manettes sans garantie, manette 1 = joueur 1. | défaut |
| 2026-11-20 | 2.0 B1 | Sémantique de `Game.media` (design §3.4 : « autres disquettes ») | Liste **complète et ordonnée** des disquettes (la 1re incluse, `media[0] == path`), vide pour un jeu à une image ; directement utilisable comme `MediaSet.images`. Zips `Disk 1`/`Disk 2` non regroupés (limite connue). | défaut |
| 2026-11-20 | 3.0 | Bouton « ouvrir le dossier des ROMs » dans le gestionnaire de fichiers du système | **Utilisateur** : en 3.0 (noté dans docs/roadmap.md). | décidé |
| 2026-11-20 | 3.0 | Bouton « Importer un BIOS… » (sélecteur de fichier, vérification du hash, copie sous le nom attendu, jamais d'écrasement silencieux ; BIOS fournis par l'utilisateur uniquement) | **Utilisateur** : oui, en 3.0 (transmis à l'architecte 3.0). | décidé |
| 2026-11-20 | 5.0 | Amstrad CPC + Commodore 64 : une ou deux versions ? | **Utilisateur** : 5.0 = CPC + C64 ; une seule version, CPC d'abord en pré-release puis C64 (repli 5.1). | décidé |
| 2026-11-20 | 2.0/3.0 | Touche hôte sur macOS | **Utilisateur** : **Ctrl droit partout** (y compris macOS, son Razer a un Ctrl droit) ; ⌘ droit seulement en repli réglable pour un clavier Mac sans Ctrl droit. Remplace « ⌘ droit sur macOS ». | décidé |
| 2026-11-20 | 3.0 | Touches Amiga gauche / droite | **Utilisateur** : **Alt gauche = Amiga gauche, Alt droit (AltGr / ⌥ droit) = Amiga droit** (évite la touche Windows) ; les Alt de l'Amiga passent par le clavier virtuel. Remplace la proposition Windows/Menu de l'ADR 0017. | décidé |
| 2026-11-20 | 3.0 | Questions du design `docs/design/v3.0-amiga.md` §9 (core puae, Kickstart absent → AROS avec avertissement, import inconnu « quand même », modèle auto, capsimg plus tard, Docker EMU_HOST_ROMS_DIR, dialogue natif + repli egui) | Défauts recommandés de l'architecte, **à confirmer par l'utilisateur au lancement de la 3.0** ; q.2 (touches) déjà décidée. | défaut |
| 2026-11-20 | 4.0 | Questions du design `docs/design/v4.0-dos.md` §9 (dosbox_pure seul, menu du core pour l'exécutable, instantané des jeux en dossier, GeneralUser GS, pavé numérique via clavier virtuel, disposition fr, mode Clavier, Windows 3.x/9x hors garantie, 70 Hz natif) | Défauts recommandés de l'architecte, **à confirmer au lancement de la 4.0**. | défaut |
| 2026-11-20 | 6.0 | Phase 7 « 32 bits » | **Utilisateur** : 6.0 = GBA + 32X (+ Mega CD ensuite). *Remplacé le 2026-11-21 : PS1 + 3DO en 7.0 ; Saturn, Dreamcast, rendu matériel plus tard.* | décidé |
| 2026-11-20 | 6.0 | Mega CD | **Utilisateur** : ajouté à la 6.0 (genesis_plus_gx ; BIOS bios_CD_*.bin fournis par l'utilisateur). | décidé |
| 2026-11-20 | 6.0 | Questions du design `docs/design/v6.0-gba-32x-megacd.md` §9 (cores, RAM Mega CD par jeu, test 32X par l'utilisateur, CHD conseillé, régions BIOS, touches GBA, livraison groupée) | Défauts recommandés, **à confirmer au lancement de la 6.0**. | défaut |
| 2026-11-20 | 5.0 | Questions du design `docs/design/v5.0-cpc-c64.md` §9 (ROMs C64 intégrées à VICE — **bloque le jalon C64 sans accord** ; x64 par défaut ; mode Joystick par défaut CPC/C64 ; chargement accéléré ; touches F1-F10/Alt=COPY/AltGr=RESTORE/<=£ ; CPC français ; cartouches CPC+ ; JiffyDOS) | Défauts recommandés, **à confirmer au lancement de la 5.0**. | défaut |
| 2026-11-20 | 7.0 | Consoles 32 bits restantes | **Utilisateur** : 7.0 = PS1 + 3DO (rendu logiciel) ; Saturn, Dreamcast et rendu matériel laissés de côté pour le moment. | décidé |
| 2026-11-20 | Rendu matériel (futur) | API graphiques à cibler | **Utilisateur** : OpenGL, DirectX **et Vulkan** ; Vulkan ne sera **pas essayé par l'utilisateur** (sa configuration le supporte mal) → les tests automatiques (lavapipe) suffisent à valider les étapes ; **rappeler à l'utilisateur** de demander à un camarade l'essai Vulkan réel. | décidé |
| 2026-11-20 | 2.0 | Disquettes ajoutées en cours de jeu : chaque session crée une nouvelle copie (`02 X.st`, `03 X.st`…) jamais nettoyée | Limite connue (revue V1bis, mineur) : à signaler dans le guide d'essai ; nettoyage / persistance des disques ajoutés à prévoir (2.1). | à surveiller |
| 2026-11-21 | CI | Portabilité Windows des tests | **Utilisateur** : OK pour un `.gitattributes` (fins de ligne) et `--no-fail-fast` dans la CI (ci.yml). Appliqué après l'audit, dans un même commit. | décidé |
| 2026-11-21 | 5.0 | ROMs système C64 intégrées à VICE | **Utilisateur** : **acceptées**, « il faut que ça marche de suite » (usage personnel ; rien n'est téléchargé à part). Le jalon C64 est débloqué. | décidé |
| 2026-11-21 | 4.0 | SoundFont GeneralUser GS (téléchargement sur action) | **Utilisateur** : **oui** ; ajoutée à la liste autorisée (CLAUDE.md, amendement ADR 0005). | décidé |
| 2026-11-21 | 2.1 | Version de la manette PS4 | **Utilisateur** : **2.1**, juste après la 2.0 (T2-T7), avec les disquettes ajoutées mémorisées. | décidé |
| 2026-11-21 | 5.0 | Mode d'entrée par défaut CPC / C64 | **Utilisateur** : **Joystick** pour CPC et C64 (ST, Amiga, DOS restent en Clavier) → champ `default_input` par système. | décidé |
| 2026-11-21 | 2.1-6.0 | Choix d'architecte (corrections de la revue croisée) : PS + R3 = clavier virtuel et PS + L3 = disquette suivante (2.1) ; md5 absents de System.dat lus du .info installé ; folder_extensions Amiga = hdf/hdz/lha ; .ipf hors dossier et hors DAT = « non classé » ; liste blanche des lignes # d'un .m3u utilisateur ; Mega CD : cue/chd/iso par dossier + sonde SEGADISCSYSTEM ; SC-55 hors import guidé | Défauts retenus, **à confirmer au lancement des versions concernées**. | défaut |
| 2026-11-21 | 2.0 | Essai rc.1 Windows : « core hatari : CRC32 x au lieu de y » | Cause : catalogue en cache plus vieux que le nightly reconstruit. Corrigé (emu-assets) : catalogue > 12 h rafraîchi avant installation, et sur écart de CRC un rafraîchissement + retéléchargement une fois ; jamais d'installation non conforme. Contournement rc.1 : « Vérifier les mises à jour » puis installer. | corrigé (rc.2) |
| 2026-11-21 | 2.1 | Sauvegardes (.srm) et états indexés par nom de jeu seul : deux jeux homonymes ont des copies de travail séparées mais partagent .srm et états (charger l'état de l'un dans l'autre peut corriger la copie) ; et core inconnu : un .zip de ROM console passe en copie de travail (choix prudent) | À traiter en 2.1 (identité de jeu commune aux copies, sauvegardes et états). | à faire (2.1) |
| 2026-11-21 | nuit | Tags pré-release posés en autonomie (nuit du 21/11) | **Utilisateur** : un tag `vX.Y.0-rc.N` posé quand une version est prête à l'essai **publie la pré-release** GitHub (comme rc.1/rc.2 ; jamais `latest`). Pas de release stable. | décidé |
| 2026-11-21 | 3.0 | Design §9 q.3 : Kickstart absent | **Utilisateur** : **AROS + avertissement unique par jeu**. | décidé |
| 2026-11-21 | 3.0 | Design §9 q.1 : core Amiga | **Utilisateur** : **puae** par défaut ; **puae2021 sélectionnable facilement**, l'interface indique qu'il est **recommandé sur Mac** (Intel). | décidé |
| 2026-11-21 | 4.0 | Design §9 q.3 : forme des jeux DOS | **Utilisateur** : **mélange zip + dossiers** (zip lus tels quels ; dossiers en instantané dans le cache, jamais modifiés). | décidé |
| 2026-11-21 | nuit | Disque : cache de build Docker partagé (dind) | **Utilisateur** : `docker builder prune` global autorisé **si besoin** (espace libre < ~10 Go). | décidé |
| 2026-11-21 | 7.0 | Core PlayStation par défaut | **Utilisateur** : **pcsx_rearmed** par défaut ; swanstation et beetle_psx **sélectionnables facilement**. | décidé |
| 2026-11-21 | toutes | Ordre de travail en autonomie | **Utilisateur** : **en largeur** (plusieurs versions avancées en parallèle), avec **un agent chargé de l'harmonie** (intégration, fichiers partagés, ordre des migrations) pour éviter les conflits. | décidé |
| 2026-11-21 | essais | Version à essayer | **Utilisateur** : essaiera **la plus récente disponible**, sur le plus d'hôtes possible (demain ou sous peu). | décidé |
| 2026-11-21 | 3.0 (X0) | « puae2021 sélectionnable facilement, recommandé sur Mac » : sous quelle forme ? | Sélecteur « Core utilisé » par système dans l'écran Cores (écrit `[cores.preferred]`) ; libellé « recommandé sur Mac » tiré de `recommended_on` (`cores.toml`), affiché sur tous les OS ; aucune bascule automatique sur macOS. Réutilisé en 7.0. | défaut |
| 2026-11-21 | 3.0 (X0) | Écart 2.0 : les DAT sont tous téléchargés depuis la base No-Intro (l'Atari ST déclaré TOSEC reçoit le DAT No-Intro) | Corrigé en 3.0 (U4 : téléchargement par source, dossier local par source ; migration v4 : ETag du DAT ST effacé pour forcer le bon fichier). Pas de correctif 2.0.x : l'identification ST fonctionne, avec moins de titres reconnus. | défaut |
| 2026-11-21 | 3.0 (X0) | API de la bibliothèque à plusieurs DAT : remplacer `dat_info(system)` ou ajouter ? | Ajout (`dat_info_for`, `dat_infos`, `import_dat_source`) ; les anciennes fonctions désignent la première source. `SCHEMA_VERSION` passe à 4 avec la migration (B3), pas avant. | défaut |
| 2026-11-21 | toutes | Intégration en largeur : branches, fichiers partagés, vagues | `docs/design/integration-largeur.md` : ligne linéaire `wip-2.1` → … → `wip-7.0`, fusions vers l'aval seulement, 3e worktree pour la 4.0 à partir du point M3 si > 10 Go libres, `firmware.toml` rempli en une passe par A3, `merge=union` proposé pour `questions-ouvertes.md` et `CHANGELOG.md`. | défaut |
| 2026-11-21 | versions | 2.0.0 stable séparée ? | **Utilisateur** : **non** ; la prochaine stable = **la dernière rc validée** par son essai ; `main` suit la dernière rc taggée. | décidé |
| 2026-11-21 | essais | Kickstart, BIOS Mega CD / PS1 / 3DO, MT-32 | **Utilisateur** : présents sur son **Mac** (pas sur le PC Windows) ; il les copiera sur le PC si les essais Windows continuent. Essais à BIOS : plutôt sur macOS. | info |
| 2026-11-21 | toutes | Validation des versions | **Utilisateur** : tout jusqu'à la **6.0** est validé par Claude (vérifications et revues automatiques) sans attendre ses essais entre les versions ; **la 7.0 n'est pas abordée** sans son feu vert. | décidé |
| 2026-11-21 | 2.1 / 6.0 | Mega CD prioritaire | **Utilisateur** : veut essayer **Dune (Mega CD)** dès le 21/11 (BIOS sur son Mac) → **prise en charge Mega CD express dans la 2.1** (`docs/design/v2.1-megacd-express.md`), reprise et complétée en 6.0. Ordre de ses essais ensuite : Amiga, DOS, CPC, C64, GBA, 32X. | décidé |
| 2026-11-21 | 2.1 | Questions du design `docs/design/v2.1-identite-disques.md` §9 (homonymes déjà joués en 2.0 : `.srm` et options copiés au second jeu, états non copiés mais signalés ; disquettes ajoutées remises dans la liste à chaque lancement sans insertion ; copies non référencées nettoyées à la main seulement ; état d'un autre fichier du même nom : confirmation, défaut Non ; core inconnu + `.zip` de ROM console : zip inspecté, `Direct` si toutes les entrées sont des ROM console) | Défauts recommandés de l'architecte, **à confirmer au lancement des tâches I0-I5**. | défaut |
| 2026-11-20 | 4.0-6.0 | Données de test libres : inventaire `docs/design/donnees-test-4-7.md` (disque Mega CD libre 240p Test Suite GPL-2.0, 32X hexgl32x MIT, GBA save tests, CPC rpg-adventure MIT, C64 Unlicense) ; nécessite un essai réel sur chaque core avant de figer les empreintes d image | Lignes à transformer en `fetch-test-assets.sh` par `platform-build` ; aucun contenu exécuté lors de l inventaire ; PS1/3DO hors périmètre | à faire |
| 2026-11-21 | 2.1 | Mega CD express (`docs/design/v2.1-megacd-express.md` §9) : dépôt du BIOS, multi-CD, RAM cartouche | Défauts appliqués : BIOS déposé à la main dans `system/` (dialogue avec chemin exact, « Copier le chemin », « Réessayer » ; pas d'import avant la 3.0 ; reconnaissance par md5 d'un BIOS mal nommé seulement si le temps le permet) ; jeux à plusieurs CD hors rc.1 (`.m3u` sous `megacd/` en 2.1.x ou 6.0) ; `cart.brm` partagée, RAM interne par jeu ; région lue dans l'en-tête du disque (comme le core), nom du fichier en repli pour les `.chd` ; homonymes séparés seulement avec l'identité de jeu (I1). | à confirmer |
| 2026-11-21 | 4.0-6.0 | Re-validation des designs 4.0, 5.0, 6.0 contre le code (3.0 : X0, R3, B3, A3, F3 ; 2.1 : identité de jeu, Mega CD express) | Section « Re-validation » ajoutée à chaque design (§10) ; **ADR 0018, 0019, 0020 acceptées** ; aucun point bloquant. | décidé (architecte) |
| 2026-11-21 | 4.0 | `.chd` Mega CD hors d'un dossier `megacd/` : listé en 2.1, mais « non classé » dès que le DOS déclare `cue` / `chd` / `iso` (contenu d'un CHD non lu) | `folder_extensions` sur `megacd` posé par la 4.0 ; `.cue` / `.iso` hors dossier restent reconnus par leur en-tête ; à annoncer dans le guide 4.0 (« rangez vos `.chd` dans `roms/megacd/` »). | défaut |
| 2026-11-21 | 4.0 | Zip DOS contenant une image CD : refusé par la règle « CD dans un zip » de la 2.1 | Le refus ne s'applique pas aux zips sous `dos/` ni à ceux qui contiennent un `.exe` / `.com` / `.bat`. | défaut |
| 2026-11-21 | 4.0 | Métadonnées `genre` / `releaseyear` / `developer` de `DOS.dat` | Non importées en 4.0 : le schéma v5 n'ajoute que `game.is_dir`. | défaut |
| 2026-11-21 | 4.0 | Deux jeux-dossiers DOS de même nom ; dossier de jeu déplacé | Instantané et sauvegardes nommés par le `save_name` de l'identité 2.1 ; empreinte d'un dossier = taille totale + CRC de la liste des fichiers (sans dates). | défaut |
| 2026-11-21 | 5.0 | `[computer] default_keyboard_mode = "keyboard"` déjà écrit dans une config 2.0-4.0 (écrit d'office dès qu'un autre réglage `[computer]` change) aurait forcé le mode Clavier sur le CPC et le C64 | Nouvelle clé `[computer] keyboard_mode = "auto" / "keyboard" / "joystick"` (défaut auto) ; ancienne clé relue une fois : `"joystick"` repris, `"keyboard"` ignoré. Effet : qui voulait vraiment « Clavier partout » devra le rechoisir dans les Réglages. | défaut, **à confirmer au lancement de la 5.0** |
| 2026-11-21 | 5.0 | `save_scope = "game"` et identité de jeu 2.1 | Un seul mécanisme : `saves/<core>/<save_name>/` pour tous les jeux d'un core `save_scope = "game"` ; motif `game_saves` sans `{game}` interdit ailleurs. | défaut |
| 2026-11-21 | 6.0 | Mega CD : région inconnue (`.chd` sans étiquette) avec le contrôle de firmware unique | Règle de la 2.1 conservée : lancement dès qu'un BIOS Mega CD est présent ; `megacd.rs` (2.1) remplacé par le contrôle générique sans changement visible. | défaut |
| 2026-11-21 | 6.0 | Cartouche RAM Mega CD (`cart.brm`, partagée) et « Réinitialiser les sauvegardes du jeu » | Jamais effacée ; seul `<jeu>.brm` l'est. Reste la question 4 du design express (une cartouche RAM par jeu ?). | défaut |
| 2026-11-21 | 5.0 / 6.0 | Données de test libres (`donnees-test-4-7.md`) : `hexgl32x` (32X, MIT), homebrews CPC / C64, AttoChess (DOS) jamais lancés sur les cores | Intégrés aux designs comme tests de fumée, sautés proprement si le core les rejette ou si l'URL disparaît ; empreintes figées après un premier essai réel par `qa-systems`. | à faire |
| 2026-11-21 | 5.0 | Un `.m3u` utilisateur est transmis tel quel en politique `Direct` (sans liste blanche) : un `#COMMAND:` atteindrait VICE | CR1 applique la liste blanche (relecture + régénération) en `Direct` dès qu'un core déclare `m3u_launch` ; VICE reste en `working_copy`. | défaut |
| 2026-11-21 | 4.0 | Disque : un 3e worktree (≈ 13 Go de `target/`) ne tient pas (12 Go libres) | Défaut : le code 4.0 démarre dans le dossier de la 2.1 dès que la 2.1 est terminée (plan d'harmonie : A reprend le rôle de C). Libérer de la place sur le disque WSL (hors emu-rust) permettrait de démarrer plus tôt. | défaut |
| 2026-11-21 | 4.0 | dosbox_pure plante au 2e chargement dans le même processus (« Redémarrer le jeu », jeu DOS suivant depuis la bibliothèque) : la bibliothèque reste en mémoire après `dlclose` | ADR 0022 : copie privée du core sous un nom unique (`<data>/cache/core-copies/`) dès que le core est détecté « resté en mémoire » ; quelques Mo non libérés par rechargement ; pas de processus enfant ni de clé `cores.toml`. Test générique « deux chargements successifs » pour tous les cores. | défaut |
| 2026-11-21 | 5.0 | `.m3u` utilisateur déjà au format VICE (`jeu.d64:PRG`) | Défaut : non pris en charge (l'entrée est lue comme un nom de fichier) ; le lancement d'un programme se règle par `[media] launch` du jeu. | défaut |
| 2026-11-21 | 6.0 | Schéma v6 : seulement `game.region` (design §10 n° 8) ? | Défaut : v6 ajoute aussi `dat_entry.serial` et `dat_entry.region` (additif, nécessaire pour l'identification par numéro de série et la région Redump). | défaut |
| 2026-11-21 | 6.0 | CRC de la piste 01 d'un CD (identification Redump) | Défaut (design §3.4) : calculé seulement si la taille de la piste figure au DAT, ≤ 512 Mio, interruptible ; sinon série puis nom. | défaut |
| 2026-11-21 | 6.0 | `(CD N)` dans les noms multi-disques | Non reconnu (seulement `(Disc N)`) ; à ajouter dans `emu-library/src/media.rs` si l'essai le demande. | défaut |
| 2026-11-21 | 7.0 | Feu vert de l'utilisateur ; conception | ADR 0021 **acceptée**, `docs/design/v7.0-ps1-3do.md` ; faits vérifiés sur les nightly (buildbot, `.info`, exécution des quatre cores sans BIOS). | décidé (architecte) |
| 2026-11-21 | 7.0 | BIOS PlayStation / 3DO absents : les quatre cores démarrent avec un remplaçant intégré (HLE de pcsx_rearmed, OpenBIOS de swanstation et beetle_psx, Opera Boot d'opera) | Défaut : **lancer avec un avertissement unique par jeu** qui propose l'import ; le BIOS de l'utilisateur (md5 vérifié) est préféré dès qu'il est présent ; rien n'est jamais téléchargé. La feuille de route passe de « requis » à « recommandés » (design §9 q.1, q.2). | défaut, **à confirmer** |
| 2026-11-21 | 7.0 | swanstation déclare `hw_render = true` | Rendu logiciel constaté (`swanstation_GPU_Renderer = Software`) : livré ainsi, sans mise à l'échelle ni PGXP (design §9 q.7). | défaut |
| 2026-11-21 | 7.0 | Manette PlayStation : mode par défaut | **Numérique** (stick gauche = croix, GTA est numérique) ; **DualShock analogique** réglable globalement et par jeu, à chaud (design §9 q.3). Vibration : plus tard (q.9). | défaut, **à confirmer** |
| 2026-11-21 | 7.0 | Cartes mémoire | Emplacement 1 = `.srm` par jeu ; emplacement 2 désactivé par défaut ; en changeant de core, la carte du jeu est copiée une fois depuis l'autre core (design §9 q.4, q.5). | défaut |
| 2026-11-21 | 7.0 | 3DO à plusieurs CD (opera sans interface disque ni `.m3u`) | Un jeu par disque ; NVRAM par jeu, « partagée » réglable (design §9 q.8). | défaut |
| 2026-11-21 | 7.0 | CD hors dossier dont le titre existe sur plusieurs machines (« Road Rash (USA) » : 3DO, PlayStation, Mega CD) ; `.chd` / `.pbp` non lus | `.chd` ambigu = non classé (ranger dans `roms/psx/`, `roms/3do/`) ; `.cue` / `.iso` reconnus par leur en-tête ; alias de dossier `ps1/` non acceptés (design §9 q.6). | défaut |
| 2026-11-21 | 7.0 | Identification Redump | PlayStation par série de `SYSTEM.CNF` (jamais de CRC) ; 3DO par taille + CRC de la piste 01 jusqu'à 800 Mio, une fois, interruptible (design §9 q.11). | défaut |
| 2026-11-21 | 7.0 | Données de test | ps1-tests (MIT) et 240p Test Suite PS1 (GPL-2.0, dont le secteur de licence porte le texte Sony : à voir par `reviewer`, repli sur un disque généré) ; **aucun disque 3DO libre** (l'OS Portfolio est sur chaque disque) : jeu 3DO réel = essai utilisateur. Licence d'OpenBIOS à sourcer par `reviewer`. | à faire |
| 2026-11-21 | 7.0 | Questions q.1-q.3 du design 7.0 §9 | **Utilisateur** : q.1 sans BIOS = lancement + avertissement unique (pas de refus) ; q.2 `panafz10.bin` préféré à Opera Boot quand présent ; q.3 manette PS1 numérique par défaut (DualShock réglable). Défauts confirmés. | décidé |
| 2026-11-21 | 7.1 / 7.2 | Suite après la 7.0 | **Utilisateur** : la 7.1 aura un contenu qu'il précisera (spécifications à venir) ; le Mega CD + 32X (Night Trap 32X) ira en **7.2**, pas urgent. | décidé |
| 2026-11-21 | 7.0 | Feu vert pour la 7.0 (PlayStation + 3DO) | **Utilisateur** : feu vert donné le 2026-11-21 après ses essais de la 6.0 ; jeux d'essai prévus : GTA (PS1), Road Rash (3DO). | décidé |
| 2026-11-21 | 3.0+ | Jeux Amiga en `.ipf` (essai utilisateur : Hunter) | `.ipf` reconnu seulement sous `roms/amiga/` ; lecture par puae seulement avec la bibliothèque SPS `capsimg` (non redistribuable) déposée à la main dans `system/`. Défaut : conseiller l'ADF ; **proposer d'avancer l'import guidé de capsimg** (design 3.0 §9 q.7, reporté). **Essai Windows 2026-11-21** : `capsimg.dll` dans `system/` + relance → toujours l'écran « disquette Workbench » (IPF non lu ; cause non établie : DLL 32 bits ? nom/chemin attendu par puae sous Windows ?) ; l'utilisateur passe à l'ADF (Another World OK) et **met ce point de côté** (Hunter retiré de son plan d'essais). À diagnostiquer avant tout import guidé (journal puae, nom attendu). | à diagnostiquer |
| 2026-11-21 | assets | Installation de `vice_x64` refusée (« CRC ») sous Windows malgré « Vérifier les mises à jour » | **Cause : buildbot** — `.index-extended` daté du 2026-11-20 (CRC f0c3c374) alors que le zip a été reconstruit le 21/11 07h30 (DLL CRC 484b598c) ; idem `vice_x64sc`. Contournement donné : attendre, ou DLL manuelle dans `%APPDATA%\emu-rust\data\cores\` (core « manuel »). Proposition : si le `Last-Modified` du zip est postérieur à la date de l'index, message « buildbot en cours de mise à jour, réessayez plus tard » au lieu d'une erreur d'empreinte (2e cas après hatari en 2.0). | à décider |
| 2026-11-21 | après 6.0 | Jeux « Sega CD 32X » (Mega CD + 32X, ex. Night Trap 32X) demandés par l'utilisateur | Hors 6.0 (ADR 0020 §7). Faisable avec picodrive (BIOS Mega CD de la région requis) : reconnaître un CD 32X au scan, l'envoyer à picodrive au lieu de genesis_plus_gx, contrôle de BIOS Mega CD. Défaut proposé : petite version après la 7.0 (ou 7.x). | à décider |
| 2026-11-21 | 7.1 | Conception de la 7.1 | **ADR 0023 acceptée** (amendée après revue indépendante : reprise par unité, marqueur d'identités, groupes multi-CD, console à deux exécutables), `docs/design/v7.1-finition-interface.md`. | décidé (architecte) |
| 2026-11-21 | 7.1 | **Icône** : quatre variantes (`docs/design/icone/planche.png` : A cartouche, B croix et boutons, C écran, D grille de bibliothèque) | **Choix de l'utilisateur.** Défaut de travail : **B** (la plus lisible en 16 px) ; changer de variante = régénérer les fichiers (tâche FP1, quelques minutes). | **à décider (utilisateur)** |
| 2026-11-21 | 7.1 | Avertissement « BIOS de remplacement » (7.0 : une fois par jeu, confirmé q.1) | Défaut 7.1 : **une fois par core et fichier** (`[interface] firmware_hints_seen`), les jeux déjà avertis ne le sont plus ; fichiers facultatifs (BIOS GBA, ROM MT-32…) : simple bandeau refermable + écran Cores. | **confirmé par l’utilisateur le 2026-11-21** |
| 2026-11-21 | 7.1 | Rangement : jeux-dossiers DOS et zips DOS trouvés en vrac ; nom présent dans un seul DAT | Proposés **décochés** (« suggérés ») dans l'aperçu ; seules les identifications sûres (en-tête, empreinte DAT, extension propre) sont cochées d'office. | défaut |
| 2026-11-21 | 7.1 | Rangement : destination déjà occupée (doublon ou autre fichier du même nom) | L'élément reste en place, signalé ; **aucun renommage ni suppression automatique** (le nom du fichier porte les sauvegardes). | défaut |
| 2026-11-21 | 7.1 | Rangement : CD à plusieurs fichiers (`.cue` + pistes, `.m3u`) | Un **sous-dossier par jeu** `roms/<id>/<nom>/` (les pistes de jeux différents portent souvent le même nom) ; images uniques (`.chd`, `.iso`) à plat. | défaut |
| 2026-11-21 | 7.1 | Rangement : dossiers externes déclarés (`[library] roots`) | **Jamais rangés** : seul `<data>/roms` l'est. | défaut |
| 2026-11-21 | 7.1 | Rangement : BIOS déposé par erreur dans `roms/` (md5 connu) | Signalé dans l'aperçu avec « Importer un BIOS… », pas déplacé par le rangement. | défaut |
| 2026-11-21 | 7.1 | Rangement : annulation | Seul le **dernier** rangement s'annule (20 journaux gardés pour la trace) ; un fichier modifié ou supprimé depuis reste où il est et figure au rapport. | défaut |
| 2026-11-21 | 7.1 | Console Windows : ligne de commande et réglage « Afficher la console » | **Deux exécutables, même programme** : `emu-rust.exe` (graphique, sans terminal) et `emu-rust-cli.exe` (console : attendu par l'invite, `$LASTEXITCODE` correct). Avec `console = true`, `emu-rust.exe` relance `emu-rust-cli.exe`. `AttachConsole` / `AllocConsole` écartés (invite qui n'attend pas, `unsafe`, non testable). La relance avec console reste un essai utilisateur. | décidé (architecte) |
| 2026-11-21 | 7.1 | Console sous Linux / macOS | Pas de réglage (aucune fenêtre de terminal propre à l'application) : `.desktop` `Terminal=false`, `.app` ; lancé depuis un terminal, le journal continue d'y être écrit. | défaut |
| 2026-11-21 | 7.1 | Journal : niveau de détail, fichier | Niveau actuel (`info` pour l'application et le core), `RUST_LOG` pour plus ; fichier `<data>/logs/emu-rust.log` (session en cours + précédente, 2 Mio) ; pas de presse-papiers ni d'« Enregistrer sous… » : « Ouvrir le dossier des journaux ». | défaut |
| 2026-11-21 | 7.1 | Erreur fatale avant l'ouverture de la fenêtre, sans console | Windows : boîte de message native ; macOS / Linux : journal + `stderr` seulement (boîte native macOS : plus tard si l'essai le demande). | défaut |
| 2026-11-21 | 7.1 | États sauvegardés après rangement, pour les cores qui mémorisent le chemin du média (CD, `media_direct`) | À mesurer par `qa-systems` (FQ3) ; si un core le fait : le dire dans le guide d'essai (réinsérer le disque par le menu « CD ») ; l'annulation remet les chemins. | à faire |
| 2026-11-21 | 7.1 | Rangement : autre volume, exFAT / FAT, `roms/<id>` lié ou monté ailleurs ; seconde instance en jeu | **Pas de copie entre volumes en 7.1** : l'unité est bloquée et expliquée (à ranger à la main) ; idem sans liens physiques ou si `roms/<id>` n'est pas un vrai dossier. Une seconde fenêtre d'emu-rust en train de jouer n'est pas exclue par le verrou : ne pas ranger pendant ce temps (dit dans l'aperçu). À rouvrir si l'essai le demande. | défaut |
| 2026-11-21 | 7.2 | CD 32X : où stocker la marque « CD 32X » ? | **Aucune migration** (schéma v6 gardé : retour à la 7.1 / 6.0 possible sur le même dossier). Marque automatique dérivée du titre et des noms de fichiers ; marque manuelle dans `config.toml` (`[library] cd32x`, clé = nom du fichier, minuscules Unicode). Renommer le fichier perd la marque manuelle ; plusieurs jeux Mega CD de même nom de fichier (`image.cue`) sont marqués ensemble, et le message le dit. ADR 0024 §2. | décidé (architecte) |
| 2026-11-21 | 7.2 | CD 32X : où placer l'action « Ce jeu est un CD 32X », et comment y inviter ? | **Clic droit** sur la vignette (validé par l'utilisateur le 2026-11-21) + une touche choisie par `ui`. **Invitation** : quand le jeu sélectionné est un jeu Mega CD non marqué et non identifié par son contenu (tout `.chd`), la ligne d'aide de la bibliothèque ajoute « Jeu Sega CD 32X ? Clic droit → Ce jeu est un CD 32X ». Écartés : bandeau « une fois » (état à mémoriser, vu par ceux qui n'ont pas de CD 32X), message après un arrêt rapide (heuristique, échec déjà subi). ADR 0024 §4. | décidé (architecte) |
| 2026-11-21 | 7.2 | CD 32X : étiquettes reconnues dans le nom | `(Sega CD 32X)`, `(Mega-CD 32X)`, `(Mega CD 32X)`, `(32X CD)`, `(CD 32X)` et `(32X)` seul, pour un jeu rangé sous Mega CD (casse et tiret indifférents). Non reconnus, à marquer à la main : `[32X]` entre crochets (TOSEC), nom sans étiquette. | défaut |
| 2026-11-21 | 7.2 | CD 32X : numéro de série (`T-1620xF`) et en-tête « SEGA 32X » | Non utilisés en 7.2 (en-tête réel jamais relevé ; trois jeux échappent au motif ; les deux disques de Night Trap partagent le numéro, que l'identification Redump ne départage pas). À rouvrir après relevé de l'en-tête d'un vrai disque. | reporté |
| 2026-11-21 | 7.2 | CD 32X à plusieurs disques non regroupés : Fahrenheit (disque 1 Mega CD « Key Disc », disque 2 CD 32X), Slam City (4 disques), Supreme Warrior USA (noms Redump trop différents) | Une entrée par disque, pas de regroupement automatique ; chaque disque étiqueté part sur picodrive ; changement de disque par le menu « CD » ou un `.m3u`. Fahrenheit : marquer à la main le disque 1, ou `.m3u`. Listés dans le guide d'essai ; à valider par un essai réel. | défaut |
| 2026-11-21 | 7.2 | CD 32X et préférence « Core utilisé » du Mega CD | La préférence ne s'applique pas aux jeux marqués : picodrive est imposé (`cd32x_cores` de `cores.toml`), jamais de repli sur genesis_plus_gx (dialogue « Installer picodrive » s'il manque). Marquer un jeu déjà joué avec genesis_plus_gx le fait repartir sans ses sauvegardes `.brm` (dit dans le message). | décidé (architecte) |
| 2026-11-21 | 7.2 | CD 32X en `.cue` : pistes 01 de plus de 512 Mio (Night Trap disque 1, Corpse Killer, Slam City, Fahrenheit disque 2) jamais comparées au DAT | Borne de CRC du Mega CD portée à **800 Mio** (comme la 3DO). Un `.cue` déjà scanné n'est pas ré-analysé : rescan complet ou marquage manuel (guide d'essai). | décidé (architecte) |
| 2026-11-21 | 7.2 | **À relever par l'utilisateur** : nom exact des deux fichiers de son Night Trap 32X (réponse du 2026-11-21 : **deux disques, en `.chd`**) | Un `.chd` n'est jamais lu : reconnaissance par l'étiquette du nom, sinon marquage manuel (invitation dans la ligne d'aide). Nommés `… (Disc 1)` / `… (Disc 2)`, les deux disques forment un seul jeu ; sinon chacun est à marquer. À ranger sous `roms/megacd/`. | en attente (nom) |

## 7.2 — à discuter avec l'utilisateur

Ajouté le 2026-11-21 (architecte). La 7.2 est **à l'étude** : `docs/essais-glisser-deposer.md` et
`docs/essais-sega-cd-32x.md`. Rien n'est conçu tant que ces questions n'ont pas de réponse (demande de
l'utilisateur : discuter du glisser-déposer avant la conception). Chaque question propose un **défaut recommandé** ;
« ok pour les défauts » suffit.

### Glisser-déposer de jeux dans la fenêtre

À savoir d'abord : emu-rust reçoit seulement **l'emplacement du fichier** déposé, jamais le nom du programme d'où
il vient. « Uniquement depuis le gestionnaire de fichiers » devient donc : « uniquement un vrai fichier, rangé à un
endroit durable de l'ordinateur ».

1. **Copier ou déplacer le fichier déposé ?**
   - a) **Copier** : l'original reste où il était (Téléchargements, clé USB…) ; prend de la place en double.
   - b) Déplacer : pas de doublon, mais le fichier disparaît de son dossier d'origine.
   - c) Copier, avec une case « Déplacer plutôt que copier » dans la fenêtre de confirmation (choix mémorisé).
   - **Défaut recommandé : c** (copie tant que la case n'est pas cochée).

2. **Faut-il toujours montrer un aperçu avant de ranger ?**
   - a) Toujours : liste « tel fichier → tel dossier », bouton « Ranger ».
   - b) **Rangement direct quand tout est sûr** (jeu reconnu sans hésitation, pas de doublon), avec un message
     « 3 jeux ajoutés — Annuler » ; aperçu seulement s'il y a un doute, un choix à faire ou un dossier entier.
   - c) Jamais d'aperçu : ce qui est douteux est simplement refusé.
   - **Défaut recommandé : b.**

3. **Ce qui vient d'un navigateur ou d'un e-mail (lien, image, pièce jointe).** Le système refuse lui-même ces
   dépôts : le pointeur affiche le symbole « interdit » et emu-rust n'est pas prévenu ; il ne peut donc **pas**
   afficher de message.
   - a) **Accepter ce comportement** (pointeur « interdit », rien ne se passe) ; un vrai message d'erreur reste
     affiché pour tout ce qu'emu-rust reçoit et refuse (questions 4 et 6).
   - b) Exiger un message dans tous les cas : demande du code propre à chaque système (Windows, macOS, Linux),
     nettement plus long et plus risqué.
   - **Défaut recommandé : a.**

4. **Fichier tiré depuis une archive ouverte** (zip ouvert dans 7-Zip, WinRAR ou l'Explorateur, sans l'avoir
   extraite). Le logiciel d'archive fabrique un fichier temporaire qu'il efface aussitôt.
   - a) **Refuser avec un message** : « Extrayez d'abord l'archive dans un dossier, puis déposez le fichier. »
   - b) Tenter la copie quand même (peut échouer une fois sur deux selon le logiciel).
   - **Défaut recommandé : a.** (Le zip lui-même, déposé depuis le gestionnaire de fichiers, est accepté : n° 5.)

5. **Un fichier `.zip` déposé depuis le gestionnaire de fichiers.**
   - a) **Rangé tel quel, sans le décompresser**, comme le fait le bouton « Ranger les ROMs… » de la 7.1 (un zip
     contenant un jeu de console est un jeu ; un zip contenant une image de CD est refusé avec le conseil habituel).
   - b) Décompressé par emu-rust avant rangement.
   - **Défaut recommandé : a** (emu-rust ne décompresse jamais vos fichiers).

6. **Un fichier téléchargé, déposé depuis la barre de téléchargements du navigateur ou depuis le dossier
   Téléchargements.** C'est un vrai fichier sur le disque : emu-rust ne peut pas le distinguer d'un fichier déposé
   depuis l'Explorateur.
   - a) **Accepté** (c'est le cas le plus courant : on télécharge un homebrew, on le dépose).
   - b) Refuser tout ce qui se trouve dans le dossier Téléchargements (gênant : il faudrait d'abord le déplacer).
   - **Défaut recommandé : a.**

7. **Un dossier entier déposé.**
   - a) **Accepté, toujours avec aperçu** : emu-rust cherche les jeux dedans (un jeu DOS en dossier est pris en
     bloc) ; les fichiers qui ne sont pas des jeux sont laissés.
   - b) Refusé : seuls des fichiers peuvent être déposés.
   - **Défaut recommandé : a.**

8. **Linux.** Le dépôt fonctionne sous Windows et macOS, et sous Linux « X11 ». Il **ne fonctionne pas** sous
   Linux « Wayland » (donc pas non plus pour la version Linux lancée dans WSL), ni dans Docker.
   - a) **Accepter cette limite** ; à ces endroits, on continue d'utiliser « Ouvrir le dossier des ROMs » puis
     « Ranger les ROMs… ».
   - b) Étudier un contournement pour Wayland (change la façon dont la fenêtre s'affiche sous Linux ; chantier à
     part).
   - **Défaut recommandé : a.**

9. **Quand peut-on déposer, et que fait « Annuler » ?**
   - Dépôt accepté sur tous les écrans hors jeu ; **pendant une partie : refusé avec un message** (« quittez le jeu
     pour ajouter des jeux »). Déposer une disquette pour l'insérer dans le jeu en cours serait une autre fonction.
   - « Annuler » après un dépôt : **retire les copies qu'emu-rust vient de faire** (seulement si elles n'ont pas
     été modifiées) ; si le fichier avait été déplacé, il retourne à sa place d'origine.
   - Autres défauts : un fichier déjà présent dans `roms/` n'est pas copié une seconde fois ; un BIOS reconnu
     ouvre « Importer un BIOS… » ; un fichier qui n'est pas un jeu est refusé avec un message.
   - **Défaut recommandé : tout ce qui précède.** Un point à changer ?

### Sega CD 32X (Mega CD + 32X)

À savoir : six jeux seulement (Night Trap, Corpse Killer, Fahrenheit, Slam City, Supreme Warrior, Surgical
Strike). Ils fonctionnent avec picodrive (déjà installé pour la 32X) et le BIOS Mega CD que vous fournissez déjà.

10. **Où ranger ces jeux dans la bibliothèque ?**
    - a) **Sous « Mega CD », avec une pastille « 32X »** : vous les mettez avec vos autres CD Mega CD
      (`roms/megacd/`), emu-rust choisit tout seul le bon moteur.
    - b) Un système à part « Mega CD 32X » avec son propre dossier.
    - **Défaut recommandé : a** (c'est aussi ainsi que les bases de données de jeux les classent).

11. **Reconnaissance.** emu-rust reconnaîtra ces disques par leur empreinte (fichiers `.cue`) ou par la mention
    `(Sega CD 32X)` / `(Mega-CD 32X)` / `(32X CD)` dans le nom du fichier (seul moyen pour un `.chd`).
    - Votre Night Trap 32X : est-ce un `.cue` avec ses pistes, ou un `.chd` ? Quel est le nom exact du fichier ?
    - a) **Si le nom ne porte pas la mention, une action « Ce jeu est un CD 32X »** dans le menu du jeu permet de
      le marquer à la main.
    - b) Pas de marquage manuel : il faut renommer le fichier.
    - **Défaut recommandé : a.**

12. **Essai.** Aucun CD 32X libre n'existe : les tests automatiques vérifieront seulement qu'emu-rust choisit
    picodrive et que la 32X s'allume (disque et BIOS factices). Le vrai jeu ne peut être validé que par vous.
    - a) **Publier la 7.2 en pré-release sur cette base**, votre essai de Night Trap 32X décidant de la suite.
    - b) Attendre votre essai d'une version de travail avant toute pré-release.
    - **Défaut recommandé : a.**

13. **Ordre et contenu de la 7.2.** Le glisser-déposer dépend du rangement de la 7.1 ; le CD 32X est indépendant
    et petit.
    - a) **Une seule 7.2 avec les deux**, CD 32X fait en premier.
    - b) CD 32X seul en 7.2, glisser-déposer en 7.3.
    - **Défaut recommandé : a.**

### Réponses de l'utilisateur (2026-11-21, questionnaire)

Toutes les options recommandées sont retenues, sauf :

- **Q4, archive ouverte** : l'utilisateur veut **garder le choix**. emu-rust affiche un avertissement : le fichier vient
  d'une archive ouverte et peut disparaître. L'avertissement propose « Extraire d'abord » (conseil, rien n'est copié)
  ou « Tenter la copie quand même ».
- **Q13, découpage** : version **7.2 = Sega CD 32X** ; **glisser-déposer en 7.2.1**.
- **Q11, Night Trap 32X** : format (`.cue` ou `.chd`) et nom exact **à relever par l'utilisateur**. Le marquage
  manuel « Ce jeu est un CD 32X » est retenu (défaut a).

Les autres questions prennent le défaut recommandé :

| Question | Réponse retenue |
|---|---|
| Q1 | Copier, avec la case « Déplacer plutôt que copier » mémorisée |
| Q2 | Rangement direct si tout est sûr, avec « Annuler » ; aperçu en cas de doute ou pour un dossier |
| Q3 | Accepter le curseur « interdit » pour les dépôts depuis un navigateur ou un e-mail |
| Q5 | Zip rangé tel quel, jamais décompressé |
| Q6 | Fichier des Téléchargements accepté |
| Q7 | Dossier entier accepté, toujours avec aperçu |
| Q8 | Wayland, WSL et Docker : limite acceptée, « Ouvrir le dossier des ROMs » puis « Ranger » |
| Q9 | Comportement du dépôt et de « Annuler » : défauts retenus |
| Q10 | Sous « Mega CD », avec une pastille « 32X » |
| Q12 | Pré-release sur la base des tests factices ; l'essai de l'utilisateur décide de la suite |
