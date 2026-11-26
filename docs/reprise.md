# Reprise — état et prochaines tâches

À lire en début de session (surtout en changeant de machine : la mémoire de Claude est locale à chaque poste).
Mis à jour à chaque fin de session. **En début de session : `git fetch` puis lire ce fichier** (règle commune
d'agents-home, ajoutée après la divergence du 2026-11-25).

## État au 2026-11-26, 21 h 20 (PC du bureau)

- **7.6.0 taguée** (v7.6.0 → 8476e54) après l'essai Windows de l'utilisateur (6/6 puis re-test Maniac Mansion OK ;
  fiche <https://claude.ai/artifact/W6aEFUVhcKgz4D5h1nNSA5>). Défaut trouvé à l'essai et corrigé : sauvegarde d'un jeu
  multi-disquette d'une même archive (`plan_moves`, emu-runtime), chemins complets dans le rapport sous Windows.
- `main` après le tag : dettes de jaquettes fusionnées (`clear_cover` / `clear_all_covers` / `CoverCache::remove`,
  réponses périmées ignorées), CHANGELOG « Non publié ».
- **Numérotation** : 8.0 systèmes personnalisés (Atari Lynx en premier, marque de scan en base PB0 → schéma v8 : une 7.x
  refusera la base), 8.1 7z, 8.2 anglais, 8.3 jgenesis, 8.4 CD, 8.5 ordinateurs, 8.6 arcade, 9.0 rendu matériel après
  la 8.6. 7z : conception (ADR 0027) commitée sur la branche locale `wip-7.7` ; créer `wip-8.1` depuis `main` après
  la 8.0 et y reprendre ADR + design (proposition dans le même commit).

## Prochaine session — dans cet ordre

1. `git fetch`, lire ce fichier, créer le cron de suivi.
2. Rétro n° 4 si pas faite le 2026-11-26.
3. 8.0 : branche `wip-8.0` depuis `main`, lot PX0 (design 8.0 §0.6). Essais 8.0 : sauvegarder le dossier de données
   avant (migration v8).
4. Dettes notées à l'essai 7.6 :
   - manette « Super Famicom Controller » (Windows, GUID 030000001f08000001e4000000000000) : gauche / droite inopérants
     avec le mappage par défaut → impossible d'atteindre la configuration de la manette (déjà vu avant) ;
   - filtre de plate-forme (colonne de gauche) appliqué seulement au clic sur « Tri : » ;
   - jaquettes libretro : nom complet demandé (« Dune (USA) (En,Fr,De,Es,It) ») alors que « Dune (USA) » existe → nom
     de secours ; la règle « 8 × 404 = source en panne » se trompe sur de vrais absents → panne seulement sur erreur
     réseau / 5xx ;
   - avertissement « l'état sauvegardé ne contient pas les disquettes » affiché aussi pour un CD (Night Trap) ;
   - message d'échec du marqueur de scan qui écrit `<base>.scanning` au lieu du vrai chemin ; `relocate` supprime une
     ligne fantôme sans le signaler ; dossiers `target/tmp/mktidy-*` laissés par les tests ;
   - test instable non identifié (≈ 1 passage local sur 3 le 2026-11-26) ;
   - mineurs de relecture : test de casse Linux dans `identity/tests.rs`, commentaire `identity.rs` (chemin absolu
     encore dans `skipped.0`) ; casse sous macOS à trancher (architecte).
5. Dettes plus anciennes : réponse `Loaded` (fait), avertissement cargo `main.rs` dans deux binaires d'emu-app ; page
   Réglages (défilement), barre d'outils sur deux lignes à 1280 px, focus initial de la bibliothèque ; idée : ligne
   « Infos » au clic droit.
6. Demander les caractéristiques du PC et les inscrire dans CLAUDE.md (« Cibles »).
7. Branches distantes supprimables avec l'accord de l'utilisateur : `feat/7.2-manette`, `feat/7.4-sega-cd-32x`,
   `feat/7.5-7z`.

## Budget

Arrêt fixé par l'utilisateur à 20 % d'usage hebdomadaire le 2026-11-26 (17 % à 17 h 36). Demander `/usage` aux
points de suivi.
