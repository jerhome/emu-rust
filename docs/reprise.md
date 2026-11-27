# Reprise — état et prochaines tâches

À lire en début de session (surtout en changeant de machine : la mémoire de Claude est locale à chaque poste).
Mis à jour à chaque fin de session. **En début de session : `git fetch` puis lire ce fichier** (règle commune
d'agents-home, ajoutée après la divergence du 2026-11-25).

## État au 2026-11-27 21:20 (PC du bureau)

- **8.0** (`wip-8.0`, worktree `../emu-rust-wt80`) : 21 lots sur 25. Commits du soir, chacun relu par `reviewer`
  (opus pour les données) et contrôlé par l'architecte contre le contrat : 93ffd82 (PB2 corrigé), 21bfaba (PB3 bascule,
  journal v2, PV4), a331dbd (PC1 table de 184 ids, PU1 enfant d'essai et témoin, PV2), 4efb855 (PU2 lancement d'un
  système personnalisé), de69b77 (PB4 `database` privé), puis PU3 (ajout, Réglages, textes ; commit en fin de session).
  Décisions tranchées et écrites au design §0.9 à §0.13 et dans l'ADR 0025 (lignes d'un id en conflit gelées, jamais
  purgées automatiquement ; retrait d'un id officiel = en conflit ; « Oublier » ciblé ; dossier des définitions passé
  par `TidyOptions::with_systems_dir`).
  **Reste** : PU4 (bascule en trois cas, `with_systems_dir` dans emu-ui, noms de boutons du core, entrées `--screen`
  des vues PU3/PU4), PQ2 (intégration, captures, scan et DAT sur buildbot simulé), PV1, PP2, PE1 (essai réel : copier
  le dossier de données avant, migration v8 ; guide `docs/guide-essai-8.0.md` commencé). Racines de bibliothèque
  supplémentaires non lues par `custom_games_on_disk` (à trancher en PU4 ou PQ2). Mineurs de la relecture PU3
  (tour 2) pour PU4 : « Supprimer » actif pendant une recherche de base (l'enregistrement d'office recrée le système) ;
  aucun « Oublier » pour un retrait d'id officiel alors que le manuel le cite (piste : portée `Frozen`) ;
  `can_forget_retired` lit une vue peut-être périmée (relire le disque avant un oubli `All`).
  Mineurs de la relecture PU4 (tour 2), pour une 8.0.x : chemins absolus encore possibles dans des messages
  d'erreur (`retire_custom` sous `systems/`, extrait toml de `config.toml`, `no_roms_folder` déjà en 7.6,
  `custom_systems.rs` e.to_string()) ; tests non écrits : bascule incomplète de bout en bout, « Non » du cas incertain.
  Écarts PQ2 pour une 8.0.x : capture `custom-settings` qui n'atteint pas la section (défilement animé) ; pas de
  `--screen` pour l'avertissement de contenu, le rapport de bascule incomplète, le refus « prévu en 9.0 », le message
  au redémarrage ; harnais sans `start_dat_download` ni jaquettes activables (« zéro requête » couvert par le filtre).
- **8.1** (`wip-8.1`) : **ADR 0027 acceptée** (f310967, trois tours de relecture) ; faits sur `sevenz-rust2`
  (`docs/design/v8.1-t1-verifications.md`, b4b6680) : limites mémoire des décodeurs et de l'en-tête à porter au design
  au lot T1. **`/emu-rust:implement` lançable par l'utilisateur** dès que la 8.0 est sur `main` et `wip-8.1` recalée
  (alerte mobile prévue). À demander pour T5 : archives `.7z` faites par 7-Zip sous Windows, dont une « ultra ».
- Notes pour la rétro n° 5 : le « rouge d'abord » (tdd) a été sauté deux fois par les agents (PB3, PU2), puis vérifié
  par mutation ; test instable vu une fois sur quatre passages, toujours sans nom.

## État au 2026-11-27 17:54 (PC du bureau)

- **8.0** (`wip-8.0`, worktree `../emu-rust-wt80`) : vague 2 livrée et relue — PB0 (marque de scan en base, schéma
  v8, fusion des jumelles décidée par l'utilisateur), PA1, PQ1, PF1, PR1, PP1, PD1 + PV3 (ADR 0023 amendée), PB1
  (catalogue dynamique). Commits 567af6a, fa39c77 (poussés), a0bc806 (PB1, local) et **WIP PB2 non relu** (local).
  Prochain : relecture de PB2 (reviewer opus) + code-review + qa-systems, puis PB3 (bascule, journal v2), PU1…
- **8.1** (`wip-8.1`, worktree `../emu-rust-wt81`, partie de `main`) : première version en flux mattpocock —
  grill (`docs/specs/v8.1-7z-grill.md`), spec (`docs/specs/v8.1-7z-spec.md`), ADR 0027 et design
  (`docs/design/v8.1-archives-7z.md`) recalés ; CD archivés en 8.1.1. À faire : relecture de l'ADR 0027, puis
  implement **après la livraison de la 8.0** (recaler `wip-8.1` sur `main`).
- Skills mattpocock locaux dans `.claude/skills/` (commit 7cd489b sur `main`, local), chargés par `cc` sous le nom
  `emu-rust:<skill>` ; seul code-review est lancé par Claude, les autres par l'utilisateur.
- Push de `wip-8.0` refusé par le contrôle des permissions (l'utilisateur l'a lancé avec `!`) : règle de permission
  à écrire en rétro.

## État au 2026-11-26, 21 h 20 (PC du bureau)

- **8.0 démarrée** : branche `wip-8.0` (poussée, ea52451) dans le worktree `../emu-rust-wt80` : lot **PX0 fait**
  (contrat : types et signatures, corps `todo!("lot …")`, design §0.7) + commit court dans la version (rétro n° 4,
  pied de page et `--version` : `7.6.0+<commit>`). Prochain : vague 2 — PB0 (marque de scan en base, schéma v8,
  library-assets), PA1 (`.info` étendus, library-assets 2e instance), PQ1 puis PF1 (libretro-ffi).
- **Version 10 = mobile**, méthode mattpocock ; grill tour 1 fait : `docs/specs/v10-mobile-grill.md`.

- **7.6.0 taguée** (v7.6.0 → 8476e54) après l'essai Windows de l'utilisateur (6/6 puis re-test Maniac Mansion OK ;
  fiche <https://claude.ai/artifact/W6aEFUVhcKgz4D5h1nNSA5>). Défaut trouvé à l'essai et corrigé : sauvegarde d'un jeu
  multi-disquette d'une même archive (`plan_moves`, emu-runtime), chemins complets dans le rapport sous Windows.
- `main` après le tag : dettes de jaquettes fusionnées (`clear_cover` / `clear_all_covers` / `CoverCache::remove`,
  réponses périmées ignorées), CHANGELOG « Non publié ».
- **Numérotation** : 8.0 systèmes personnalisés (Atari Lynx en premier, marque de scan en base PB0 → schéma v8 : une 7.x
  refusera la base), 8.1 7z, 8.2 anglais, 8.3 jgenesis, 8.4 CD, 8.5 ordinateurs, 8.6 arcade, 9.0 rendu matériel après
  la 8.6. 7z : conception reprise sur `wip-8.1` (partie de `main` en avance, le 2026-11-27) ; `wip-7.7` n'est plus
  qu'une archive.

## Prochaine session — dans cet ordre

0. **Intégration progressive des skills mattpocock** — plan version par version : `docs/skills-mattpocock.md` (validé le
   2026-11-26, chaque rétro valide skill par skill) :
   - adapter maintenant en copies locales dans `.claude/skills/` d'emu-rust les 6 skills du flux (grill-with-docs,
     to-spec, to-tickets, implement, code-review, retro) : français, sans PR, délégation aux agents de crate, push par
     lots ; efficacité jugée à la rétro ;
   - **tdd** sur PB0 et **code-review** (en plus de `reviewer`) sur PB0 et PA1 ;
   - tickets : des fichiers `.md` pour l'instant ; issues GitHub à partir de la 9 ou de la 10 (à évoquer en rétro) ;
   - **retro** : fusionnée avec notre rétro (questionnaire, mesures), seulement dans ce repo.

1. `git fetch`, lire ce fichier, créer le cron de suivi.
2. 8.0 : PU4 (bascule, `ui`, design §0.10 à §0.13 « Aval pour PU4 »), puis PQ2, PV1, PP2 ; essai réel PE1 après copie du
   dossier de données (migration v8). Grill v10 tour 2 si l'utilisateur le demande.
3. Après la livraison de la 8.0 : recaler `wip-8.1` sur `main`, prévenir l'utilisateur (`/emu-rust:implement`).
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
6. Rétro n° 4 (2026-11-26), lots décidés : (a) commit court dans le pied de page et `--version` (`7.6.0+8476e54`,
   platform-build + ui) ; (b) étude : lancer les tests Windows en local avant de pousser (cible windows-gnu + wine) ;
   (c) qa-systems : nommer puis corriger le test instable (priorité basse). Règles écrites dans CLAUDE.md (tests
   portables, guide d'essai avec cas particuliers, reviewer opus pour les données, défaut d'essai → x.y.1).
7. Demander les caractéristiques du PC et les inscrire dans CLAUDE.md (« Cibles »).
8. Branches distantes supprimables avec l'accord de l'utilisateur : `feat/7.2-manette`, `feat/7.4-sega-cd-32x`,
   `feat/7.5-7z`.

## Budget

Arrêt fixé par l'utilisateur à 20 % d'usage hebdomadaire le 2026-11-26 (17 % à 17 h 36). Demander `/usage` aux
points de suivi.
