# Reprise — état et prochaines tâches

À lire en début de session (surtout en changeant de machine : la mémoire de Claude est locale à chaque poste).
Mis à jour à chaque fin de session. **En début de session : `git fetch` puis lire ce fichier** (règle commune
d'agents-home, ajoutée après la divergence du 2026-11-25).

## État au 2026-11-26, 18 h 20 (PC du bureau)

- **7.5.0 taguée** (v7.3.0 → 496f8ce, v7.4.0 → d46f66f, v7.5.0 → 28121aa, le 2026-11-26 à 00:19) après l'essai
  Windows de l'utilisateur (6/6). Releases GitHub publiées par le workflow de tag.
- **7.6 = rangement des ROMs, poussée sur `main`** (3628a42, fusion de `wip-7.6-merge`) : rangement branché sur la
  7.5, scan annulé sans perte de favoris, **marqueur « scan en cours »** (`<base>.scanning`, décision B : après un arrêt
  brutal pendant un scan, le scan suivant est prudent — aucune purge — et le suivant retire les jeux vraiment supprimés ;
  messages dans le statut et le Journal), tests `TidyBusy` stabilisés (flock hérité par fork : arbre mktidy généré une
  fois). qa-systems OK (1629 tests, clippy Linux + Windows) ; reviewer OK au 2e tour. **Pas de tag** : essai d'abord
  (`docs/guide-essai-7.6.md`).
- **⚠ Boucler les essais de la 7.6 (et le tag v7.6.0) AVANT tout essai de la 8.0** (demande de l'utilisateur) : la
  8.0 migre le schéma v7 → v8 (marque de scan en base, lot PB0) ; une 7.x refuse ensuite la base.
- **Numérotation (décisions du 2026-11-26)** : 8.0 systèmes personnalisés (Atari Lynx essayé en premier), 8.1 archives
  7z (cache `archive_cache_mb`, défaut 2000, minimum 1000), 8.2 interface en anglais, 8.3 jgenesis, 8.4 CD,
  8.5 ordinateurs, 8.6 arcade, **9.0 rendu matériel après la 8.6**. ADR 0025 / 0026 amendées, design 8.0 recalé sur le
  code de la 7.6 (chemin critique de 14 étapes, PX0 → PB0 → … → PP2).
- **7z** : conception (ADR 0027, non commitée) dans le worktree `../emu-rust-wt77` (branche locale `wip-7.7`, base
  ancienne) ; proposition : `.claude/proposition-numerotation-8x.md` de ce worktree. Créer `wip-8.1` depuis `main`
  après la 8.0 et y copier ADR 0027 + design (pas de rebase).

## Prochaine session — dans cet ordre

1. `git fetch`, lire ce fichier, créer le cron de suivi.
2. Essai de la 7.6 par l'utilisateur (`docs/guide-essai-7.6.md`, sur une copie de la bibliothèque) → tag v7.6.0 sur
   accord.
3. 8.0 : branche `wip-8.0` depuis `main`, lot PX0 (design 8.0 §0.6 et lots).
4. Mineurs reportés : `relocate` supprime une ligne « fantôme » sans le signaler dans `TidyReport` ; message
   d'échec du marqueur qui écrit `<base>.scanning` au lieu du vrai chemin ; dossiers `target/tmp/mktidy-*` laissés
   par les tests (balayés au run suivant).
5. Dettes à déléguer (inchangées) : `Library::clear_cover` / `clear_all_covers` + `CoverCache::remove` ; réponse
   `Loaded` périmée dans `covers.rs` ; avertissement cargo `main.rs` dans deux binaires d'emu-app ; page Réglages
   (défilement), barre d'outils sur deux lignes à 1280 px, focus initial de la bibliothèque ; idée : ligne « Infos »
   au clic droit (core, système, fichiers, marque 32X).
6. Demander les caractéristiques du PC et les inscrire dans CLAUDE.md (« Cibles »).
7. Sur le Mac (écran Retina seul) : HiDPI ×2, 16:10, performances à ~4 Mpx, fenêtre entre écrans.
8. Branches distantes supprimables avec l'accord de l'utilisateur : `feat/7.2-manette`, `feat/7.4-sega-cd-32x`,
   `feat/7.5-7z`.

## Budget

Arrêt fixé par l'utilisateur à 20 % d'usage hebdomadaire le 2026-11-26 (17 % à 17 h 36). Demander `/usage` aux
points de suivi.
