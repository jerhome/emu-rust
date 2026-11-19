# ADR 0010 — Synchronisation A/V pilotée par l'audio, sur le thread de l'interface

Statut : accepté (2026-11-19)

## Contexte

- Phase 1b : boucle cadencée par l'horloge murale (`FramePacer`), dérive absorbée par le DRC (ADR 0003, amendement).
  La phase 2 doit synchroniser sur l'audio, offrir vsync, pause et avance rapide.
- ADR 0008 laissait ouverte la question d'un thread d'émulation dédié (le `Core` serait alors créé sur ce thread).
- Phase 7 : les cores en rendu matériel (`SET_HW_RENDER`) dessinent dans un FBO du frontend et doivent tourner sur le
  thread où le contexte GL est courant.
- Cores 16 bits : `retro_run` coûte quelques millisecondes, même en debug (`opt-level = 2` sur les dépendances).

## Décision

1. **Pas de thread d'émulation** : `Emulator`, `GlWindow`, egui et la boucle `winit` restent sur le thread principal.
   Seul le thread audio de `cpal` est séparé (anneau SPSC, inchangé).
2. **Ordonnanceur `emu_runtime::Pacer`** (remplace l'usage de `FramePacer` dans l'UI ; `FramePacer` reste comme
   brique interne) :
   - `SyncMode::Audio` (défaut) : une image est émulée tant que l'audio en file est sous la consigne (`latency`,
     64 ms) ; sinon attente jusqu'à `now + (en file − consigne)`, bornée à une période d'image.
   - Repli automatique sur l'horloge murale (`SyncMode::Clock`) sans périphérique audio, après une erreur de flux
     ou pendant la pause audio.
   - Au plus `MAX_BURST` = 3 images d'affilée avant de rendre la main à la boucle d'événements.
   - Le DRC (±0,5 %) reste actif : il corrige la dérive fine ; le pacer fixe le rythme.
3. **Vsync** : option (`GlWindow::set_vsync`), activée par défaut. Le pacer ne dépend pas d'elle ; elle évite le
   déchirement. Si l'échange bloque trop (WSLg, écran ≠ 60 Hz), l'utilisateur la coupe.
4. **Pause** (menu ouvert, touche pause) : plus d'appel à `run_frame`, `AudioOutput::set_paused(true)` (silence sans
   compter de sous-alimentation, pré-remplissage réarmé à la reprise).
5. **Avance rapide** (touche maintenue) : horloge murale à `fps × ratio` (4× par défaut), audio en pause,
   `Core::set_fast_forwarding(true)`.

## Conséquences

- Simple et déterministe ; compatible avec le rendu matériel de la phase 7 sans contexte partagé.
- Une image lente de l'UI (egui) retarde l'émulation : acceptable (egui ne dessine que le menu et l'OSD). Si un
  core lourd (phase 7) ne tient pas la cadence, une ADR pourra introduire un thread d'émulation (rendu logiciel
  seulement) ; l'API `Emulator` n'expose rien qui l'empêche.
- Remplace le paragraphe « Pas de core sur un thread dédié en phase 1 … » d'ADR 0008 (Conséquences) et précise
  la section Timing d'ADR 0003.
