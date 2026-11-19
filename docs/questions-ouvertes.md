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
- P1 : `sound-check` refuse de tourner sans PULSE_SERVER/socket (message clair) ; `--no-audio` ajouté par l'entrypoint en mode noVNC uniquement. Non bloquant.
| 2026-11-19 | 6 | Licence du dépôt public `jerhome/emu-rust` : aucune pour l'instant (= tous droits réservés). | `MIT OR Apache-2.0`, validé par l'utilisateur. | résolu |
