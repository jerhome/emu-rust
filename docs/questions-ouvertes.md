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
