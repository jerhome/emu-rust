# Questions ouvertes

Questions et décisions **non bloquantes** : une décision par défaut a été prise pour avancer. L'utilisateur peut la
remettre en cause à tout moment. Les questions bloquantes lui sont notifiées directement.

| Date | Phase | Question | Décision par défaut | Statut |
|---|---|---|---|---|
| 2026-11-19 | 1 | Correspondance des boutons Mega Drive dans genesis_plus_gx : A et Start scriptés n'ont pas ouvert de sous-menu dans le test d'entrées. | Vérifié expérimentalement : libretro Y = MD A, B = B, A = C. `KeyMap::genesis_default()` était déjà correct (Z = A, X = B, C = C, Entrée = Start) ; c'est le script du test qui pressait libretro A (= MD C). Test corrigé. Mapping 6 boutons non vérifié (core en 3 boutons par défaut). | résolu (1b) |
| 2026-11-19 | 1b | Outillage perso (Rust, X11/GL, Mega Drive) embarqué dans l'image commune de claude-sandbox. | Noté dans claude-sandbox (PR #3, mergée) ; à traiter plus tard. | reporté |
| 2026-11-19 | 1b | Config ALSA dans l'image runtime : statique (`/etc/asound.conf`) ou conditionnelle ? | Conditionnelle : `entrypoint.sh` écrit `~/.asoundrc` (défaut -> pulse) seulement si `PULSE_SERVER` est défini ; sans serveur (noVNC) l'ouverture ALSA échoue vite (pas de blocage), l'app doit traiter l'absence de périphérique audio comme « pas de son ». | décidé |
