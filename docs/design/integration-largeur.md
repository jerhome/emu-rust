# Intégration « en largeur » des versions 2.1 à 6.0 — plan de l'agent harmonie

Décision utilisateur du 2026-11-21 : avancer plusieurs versions en parallèle, avec un agent (« harmonie » =
`architecte`) garant des fichiers partagés, de l'ordre des migrations et des points de fusion. Ce document fait foi
pour l'ordre de travail ; les contrats restent dans les ADR et les designs de chaque version.

Contraintes machine : 8 cœurs, 7 Go de RAM, ~19 Go de disque libres. Demande utilisateur (nuit du 21/11) :
**5 à 6 agents** en même temps. ⇒ 2 worktrees en permanence, **un 3e (C) possible pour la 4.0+** tant qu'il reste
plus de 10 Go libres (un `target/` de `cargo check` ≈ 1 Go, avec les tests ≈ 4 à 6 Go) ; `cargo` se sérialise par
worktree (verrou de `target/`) : `CARGO_BUILD_JOBS=3` avec 2 worktrees actifs, **`2` avec 3** ; `rm -rf
target/debug/incremental` entre les vagues ; dans C, `cargo check` et tests ciblés (`-p <crate>`) seulement, la
vérification complète se fait en B.

Décisions utilisateur de la nuit (prioritaires) : **la 7.0 n'est pas abordée sans son feu vert** (ADR 0021
abandonnée, aucune tâche 7.0 ci-dessous ne doit être lancée) ; **tout jusqu'à la 6.0 est validé par nous-mêmes**
(les tâches « essai réel » E3, DE1, CE1, KE1, GE1 ne bloquent plus la version suivante : la revue et la vérification
complète suffisent pour le tag rc) ; **Mega CD express dans la 2.1** (§8), à livrer en `v2.1.0-rc.1` cette nuit.

## 1. Branches

Ligne **linéaire**, confirmée : `main` ← `wip-2.1` ← `wip-3.0` ← `wip-4.0` ← `wip-5.0` ← `wip-6.0` ← `wip-7.0`.

- Une branche `wip-N` est créée **depuis la tête de `wip-(N-1)`**, au moment de sa tâche de contrat (X0, DX0, CX0,
  GX0), pas avant : sans contrat posé, il n'y a rien à y écrire.
- Les fusions vont **toujours vers l'aval** (`wip-(N-1)` → `wip-N`, par `git merge`, jamais de rebase ni de
  cherry-pick vers l'amont). Un correctif se fait sur la **branche la plus en amont concernée**, puis descend par
  fusion. Un agent de `wip-N` qui trouve un défaut d'une version antérieure le signale, il ne le corrige pas sur place.
- `wip-N` est taggable `vN.0.0-rc.K` quand elle est verte (vérification complète) **et** revue ; le tag ne dépend pas
  de l'aval. `main` avance par fast-forward, dans l'ordre des versions, sur chaque branche taggée.
- Version (`Cargo.toml`, `Cargo.lock`) et section du `CHANGELOG` : écrites **au moment du tag seulement**, par la
  session principale. En cas de conflit de fusion sur ces fichiers : garder la version de l'aval, régénérer
  `Cargo.lock` (`cargo update -w`), garder les deux sections du `CHANGELOG`.
- **Emplacements** : A = `/workspace/other/emu-rust`, B = `/workspace/other/emu-rust-wt30`, C (facultatif) =
  `/workspace/other/emu-rust-wt40`. Régime permanent : **B = intégration UI, QA, revue et rc de la version N ;
  l'autre emplacement = contrat et crates bas de la version N+1.** Aujourd'hui A = `wip-2.1`, B = `wip-3.0`.
  C est créé au point **M3** sur `wip-4.0` (sans attendre le tag de la 2.1) ; quand la 2.1 est taggée, A est libéré
  et reprend le rôle de C (C supprimé, `target/` compris) ou accueille `wip-5.0` si le disque le permet.
- Ce qui débloque la largeur : `app.rs` est le goulot (une instance à la fois, toutes versions confondues). Les
  crates bas n'attendent donc pas l'UI de la version précédente : **la tâche de crate de N+1 démarre dès que la
  tâche du même crate de N est fusionnée** (point M3 ci-dessous), pas quand N est « terminée ». Les lignes
  « Dépend de : N terminée » des tâches DX0 / CX0 / GX0 se lisent ainsi.

## 2. Fichiers partagés : propriétaire et ordre

Règle : une seule instance d'agent par fichier, **tous worktrees confondus** quand le fichier est modifié des deux
côtés d'une fusion à venir. Ordre = ordre des versions ; une tâche ne prend le fichier qu'après la fusion de la
précédente.

| Fichier | Ordre des propriétaires | Règle |
|---|---|---|
| `crates/emu-ui/src/app.rs` | 2.1 T3 → 3.0 U4 → 4.0 DU2 (puis DG1) → 5.0 CU2, KU2 → 6.0 GU2 | Strictement en série. U4 ne démarre qu'après la fusion M2. X0 y a touché **un seul bloc** (`App::build`, calcul de `working_copy`) |
| `emu-ui` : `config.rs`, `config_gamepad.rs`, `gamepad/**` | 2.1 (T2-T4) → 4.0 DG1 → 6.0 GU1 (`gamepad/mapping.rs`) | Intouchables sur `wip-3.0` |
| `emu-ui` : `lib.rs`, `Cargo.toml` | 2.1 T2 (gilrs) → 3.0 U3 (`opener`, `rfd`, lignes `mod`) → suivantes | U3 après M1 ; U3 n'ajoute à `lib.rs` que des lignes `mod` |
| `emu-ui` : `config_computer.rs`, `game_options.rs`, `computer/computed.rs`, `computer/keymap.rs`, `content.rs` | 3.0 U3 → 4.0 DU1 → 5.0 CU1 / KU1 → 6.0 GU1 | Logique pure ; série par version |
| `emu-ui/data/keyboards/*.toml` | un fichier par machine (`amiga` U3, `pc` DU1, `cpc` CU1, `c64` KU1) | Fichiers neufs : aucun conflit ; l'`include_str!` se fait dans `vkbd.rs` (tâche d'intégration) |
| `emu-library/data/systems.toml` | **2.1 Mega CD express (`megacd`)** → B3 (`amiga`) → DB1 (`dos`) → CB1 (`cpc`), KB1 (`c64`) → GB1 (`gba`, `32x` ; `megacd` étendu) | Blocs ajoutés **en fin de fichier**, jamais réordonnés. Nouvelles clés : `dat` en liste, `folder_extensions` (3.0) ; `default_input` (5.0). Pas de bloc posé d'avance : l'analyseur refuse les clés inconnues et des tests énumèrent les systèmes |
| `emu-assets/data/cores.toml` | **2.1 Mega CD express** → A3 → DA1 → CA1, KA1 → GA1 | Même règle. Clés : `media_direct`, `game_saves`, `builtin_firmware`, `required_firmware` (3.0) ; `featured_options`, `layout_option`, `analog` (4.0) ; `save_scope`, `options_by_extension`, `m3u_launch`, `actions` (5.0) ; 6.0 et 7.0 : données seulement |
| `emu-assets/data/firmware.toml` | **A3, en une passe** | A3 ajoute d'un coup toutes les sections utiles de `System.dat` (même commit épinglé) : Amiga, Atari ST, DOS (MT-32), JiffyDOS s'il y figure (les BIOS Mega CD y sont déjà depuis la 1.0 ; PlayStation et 3DO : pas avant le feu vert de la 7.0). Données inertes tant qu'aucun core ne les demande ; l'import de BIOS les reconnaît dès la 3.0. `THIRD-PARTY-DATA.txt` mis à jour une fois (P3). Les versions suivantes n'y touchent plus, sauf oubli |
| `emu-assets/data/free_bios.toml` | 4.0 DA1 seulement (SoundFont) | — |
| `emu-assets/src/cores/{select,firmware}.rs`, `bios.rs` | A3 → DA1 → CA1 → GA1 | Série |
| `emu-library/src/db.rs` et migrations | **v4 = B3 (3.0) → v5 = DB1 (4.0) → v6 = GB1 (6.0)** ; 5.0 et 2.1 : aucune | `MIGRATIONS` en ajout seul. `SCHEMA_VERSION` monte **dans le même commit** que sa migration (X0 l'a laissé à 3). v4 est **gelée au point M3** : DB1 n'écrit v5 qu'après. Une migration contenue dans un tag rc n'est plus jamais modifiée (correctif = migration suivante) |
| `emu-library/src/{scan,systems}.rs` | B3 → DB1 → CB1 / KB1 → GB1 | Série (même agent `library-assets`) |
| `emu-runtime/src/{media,emulator,error}.rs` | R3 → DR1 → CR1 | Série. La tâche 2.1 « identité de jeu » (sauvegardes, états) touche `saves.rs` et peut-être `emulator.rs` : voir §6 |
| `emu-libretro/src/{callbacks,sys,core}.rs` | F3 → DF1 → CF1 → GF1 | Série ; `vfs.rs` (DF1) est un fichier neuf |
| `tests/mock-core/mock_core.c` | Q4 (3.0) → DQ1 → CQ1 → GQ1 | Un seul fichier C : chaque comportement nouveau est **gardé par sa variable d'environnement**, comportement par défaut inchangé. Q4 peut poser d'avance les comportements simples de 5.0 / 6.0 (dossier de sauvegarde et touches visibles, taille d'état variable, log ERROR + refus de `retro_load_game`) ; VFS (4.0) et env 64 (5.0) restent à leurs versions |
| `tests/tools/*.py` | un outil par machine (`mkadf` Q4 ; `mkdos` DQ1 ; `mkcpc`, `mkcdt` CQ1 ; `mkc64`, `mktap` KQ1 ; `mkcd` GQ1) | Fichiers indépendants : **faisables dès cette nuit** sur `wip-3.0` |
| `scripts/fetch-test-assets.sh` | P3 → DP1 → CP1, KP1 → GP1 | Un bloc par version, ajouté en fin ; SHA-256 épinglés. P3 peut poser d'avance les blocs 4.0-6.0 dont les hashes sont dans les études `docs/essais-*.md` |
| `.github/workflows/*.yml`, `about.toml`, `docker/**` | 2.1 T5 → P3 → DP1 → … | `platform-build` seul ; P3 n'y touche qu'après M1 |
| `packaging/THIRD-PARTY-DATA.txt`, README de release, `docker/README.md` | P3 → DP1 → CP1, KP1 → GP1 | Série |
| `docs/questions-ouvertes.md`, `docs/roadmap.md`, `CHANGELOG.md` | session principale et `architecte` | Ajout en fin seulement. Conflit de fusion = garder les deux côtés. Proposé à la session principale : `docs/questions-ouvertes.md merge=union` et `CHANGELOG.md merge=union` dans `.gitattributes` |

## 3. Socle commun : règles du §3.5 du design 3.0

Les versions 4.0 à 7.0 **n'ajoutent que des données** à ces mécanismes ; toute modification repasse par
l'`architecte` (ADR ou amendement du design 3.0), jamais par un agent de crate d'une version suivante.

| Règle (design 3.0) | Posée par | Gelée à | Utilisée par |
|---|---|---|---|
| §3.5.1 ordre d'évaluation des médias (`MediaRules`) | X0 + R3 | M3 | 4.0 (`zip`, `dosz` directs), 5.0 (`m3u_launch`), 6.0 |
| §3.5.2 extensions partagées, `folder_extensions`, non classés | X0 + B3 | M3 | 4.0 (CD sous `dos/`), 5.0 (`sna`, `tap`), 6.0 (CD + sonde) |
| §3.5.3 `game_saves`, `reset_core_game_files` | X0 + R3, A3 | M3 | 4.0, 5.0, 6.0 |
| §3.5.4 défauts calculés (`computed_options`) | U3 | M4 | 4.0 (`layout_option`), 5.0 (`options_by_extension`) |
| §3.5.5 contrôle de firmware unique (`check_required`) | X0 + A3 | M3 | 5.0 (`Builtin`), 6.0 et 7.0 (variante = région) |
| §3.5.6 format de `firmware.toml`, source `System.dat` | A3 | M3 | toutes |
| §3.5.7 notices non embarquées | P3 | M4 | toutes |
| Schéma v4 (`(system, source)`) | B3 | M3 | 5.0 (deux DAT CPC) |

## 4. Points de fusion (vérifiés par harmonie)

À chaque point : fusion, `cargo check --workspace --all-targets`, puis la liste ci-dessous ; un écart = retour à
l'agent propriétaire, pas de correction par harmonie hors `docs/` et signatures.

| Point | Fusion | Condition | Vérifications |
|---|---|---|---|
| **M0** | — (commit de X0 sur `wip-3.0`) | `cargo check` vert | fait ; liste des fichiers touchés au §7 |
| **M1** | `wip-2.1` → `wip-3.0` | 2.1 T2-T5 verts | `app.rs` : le bloc de X0 survit ; `Cargo.lock` régénéré ; `about.toml` et CI de T5 présents ; aucun `match` sur `PadStyle` / `FirmwareState` cassé |
| **M2** | `wip-2.1` → `wip-3.0` | 2.1 revue, `v2.1.0-rc.1` | « identité de jeu » (§6) compatible avec `game_key`, `{game}` de `game_saves` et les dossiers `.disks` ; **U4 peut démarrer** |
| **M3** | — (sur `wip-3.0` ; création de `wip-4.0`) | R3, A3, B3, F3 verts | plus aucun `todo!()` hors UI ; noms identiques partout (`media_direct`) ; `cores.toml` ↔ `systems.toml` cohérents (test `emu-ui`) ; migration v4 testée sur une base 2.0 ; **§3.5 gelé** ; DX0 peut démarrer en A |
| **M4** | `wip-3.0` → `wip-4.0` | U4, Q5, Q6, V3 ; `v3.0.0-rc.1` | `app.rs` passe à DU2 ; B se libère pour l'intégration 4.0 ; A crée `wip-5.0` depuis `wip-4.0` quand DR1 / DA1 / DB1 / DF1 sont verts |
| **M5, M6…** | même schéma pour 4.0 → 5.0 → 6.0 → 7.0 | — | migrations v5 puis v6 dans l'ordre ; `mock_core.c` : comportement par défaut inchangé |

## 5. Vagues proposées pour la nuit

Jusqu'à **6 agents** en même temps, un par crate **et par worktree**, une instance par fichier (§2). Deux instances
du même type d'agent sont permises si elles travaillent sur des crates (ou des worktrees) différents et des fichiers
disjoints. A = `wip-2.1`, B = `wip-3.0`, C = `wip-4.0` (à partir de M3).

| Vague | Tâche | Agent (modèle) | Où | Dépend de |
|---|---|---|---|---|
| 1 (en cours) | 2.1 T2-T4 | `ui` | A | — |
| 1 (en cours) | 2.1 T5 | `platform-build` | A | — |
| 1 (en cours) | design 2.1 « identité / disques » et **design Mega CD express** (`docs/design/v2.1-megacd-express.md`), docs seulement | `architecte` (Fable) | A | — |
| 2-3 | **Mega CD express** (§8) : tâches du design, relu par harmonie avant code | `library-assets`, `ui` (après T3 : `app.rs`), `qa-systems` | A | design relu ; prioritaire sur l'identité de jeu |
| 1 (en cours) | **Q4** core factice, `mkadf.py` | `qa-systems` (sonnet) | B | — |
| 1 (fait) | X0 3.0, ce plan | harmonie | B | — |
| **2** | **A3** import de BIOS, cores Amiga, `check_required`, `recommended_on`, `firmware.toml` en une passe | `library-assets` (**opus**) | B | X0 |
| **2** | **R3** règles de médias, `reset_core_game_files` | `runtime-av` (**opus**) | B | X0 |
| **2** | **B3** bibliothèque Amiga, schéma v4, extensions partagées | `library-assets` (**opus**, 2e instance : crate `emu-library`) | B | X0 |
| **2** | **F3** environnement puae | `libretro-ffi` (sonnet) | B | X0 |
| **2** | **U3a** logique pure en **fichiers neufs seulement** : `computer/amiga.rs`, `computer/computed.rs`, `platform.rs` (sans `opener`), `data/keyboards/amiga.toml` + lignes `mod` | `ui` (sonnet, 2e instance : B, fichiers disjoints de T2-T4) | B | X0 |
| **2** | Outils de test neufs 4.0-6.0 : `tests/tools/mkdos.py`, `mkcd.py` (puis extensions `mkcpc`, `mkcdt`, `mkc64`, `mktap`) | `qa-systems` (sonnet, après Q4) | B | Q4 fini |
| — | **M1** (`wip-2.1` → `wip-3.0`) | harmonie | B | T2-T5 verts |
| **3** | **U3b** reste de U3 : `Cargo.toml` (`opener`, `rfd`), `file_picker.rs`, `computer/keymap.rs`, `config_computer.rs`, `game_options.rs`, `content.rs` | `ui` (sonnet) | B | M1, U3a |
| **3** | **P3** plate-forme (blocs 3.0 ; blocs 4.0-6.0 d'avance dans `fetch-test-assets.sh`) | `platform-build` (sonnet) | B | M1 |
| **3** | **Q5a** tests d'intégration runtime / assets / bibliothèque | `qa-systems` (sonnet) | B | R3, A3, B3 |
| **3** | 2.1 : identité de jeu (selon `docs/design/v2.1-identite-disques.md`), T6-T7 | `runtime-av` + `ui` | A | design 2.1, T2-T5 ; §6 H1 |
| **3** | Re-validation des designs 4.0, puis 5.0 et 6.0, contre le code (même exercice que 3.0 §10) | `architecte` (Fable) | B (`docs/` seulement) | M0 |
| — | **M3** (R3, A3, B3, F3 verts ; §3.5 gelé ; création de `wip-4.0` et du worktree C) | harmonie | B → C | vague 2 |
| **4** | **U4** intégration UI et CLI (seul sur `app.rs`) | `ui` (sonnet) | B | **M2**, U3b, R3, A3, B3 |
| **4** | Q5b (UI, réel) → **Q6** → **V3** (opus) → `v3.0.0-rc.1` → **M4** | `qa-systems`, `reviewer` | B | U4 |
| **4** | 2.1 : vérification complète, revue, `v2.1.0-rc.1`, puis **M2** | `qa-systems`, `reviewer` | A | vague 3 |
| **4** | **DX0** contrat 4.0 | `architecte` | C | M3, design 4.0 re-validé |
| **4** | **DF1** VFS v3, `reset_with` (opus) ‖ **DA1** cores DOS, SoundFont (opus) ‖ **DR1** instantané (opus) ‖ **DB1** schéma v5 | `libretro-ffi`, `library-assets` ×2, `runtime-av` | C | DX0 ; une instance par crate dans C |
| **5** | **DU1** puis **DU2** (4.0), en B une fois la 3.0 taggée ; **CX0** puis crates bas 5.0 dans l'emplacement libéré | — | B, A ou C | M4 |

Largeur maximale réaliste : vague 2 = 6 agents (2 en A qui finissent, 4 à 5 en B) ; vagues 3 et 4 = 5 à 6. Les agents
de B partagent un seul `target/` : leurs `cargo` s'attendent les uns les autres ; leur demander `cargo check -p
<crate>` et `cargo test -p <crate>` pendant le travail, la vérification complète une seule fois à la fin.

Ce qui peut avancer **sans attendre la 3.0** (4.0, 5.0, 6.0) :

- **documents** : re-validation des designs 4.0 / 5.0 / 6.0 contre le code réel, guides d'essai ;
- **outils de test neufs** (`tests/tools/mkdos.py`, `mkcd.py` ; extensions de `mkcpc`, `mkcdt`, `mkc64`, `mktap`),
  fichiers inertes pour la 3.0, commités sur `wip-3.0` ;
- **données en une passe** : sections `System.dat` de `firmware.toml` (A3), blocs de `fetch-test-assets.sh` (P3),
  comportements gardés du core factice (Q4) ;
- **claviers virtuels** `pc.toml`, `cpc.toml`, `c64.toml` : fichiers neufs, mais à écrire après U3 (qui fixe la table
  `[remap]`) ;
- **rien d'autre** : les blocs `systems.toml` / `cores.toml`, les migrations v5 / v6, `PadStyle::Gba` et tout ce qui
  passe par `app.rs` attendent leur tour (§2).

## 6. Risques d'harmonie ouverts

| # | Risque | Décision par défaut |
|---|---|---|
| H1 | 2.1 « sauvegardes et états indexés par identité de jeu » touche le nom de jeu, que la 3.0 utilise partout (`game_key`, dossiers `.disks`, `{game}` de `game_saves`, options par jeu) ; contrat en cours d'écriture (`docs/design/v2.1-identite-disques.md`, sur `wip-2.1`) | Harmonie relit ce design avant tout code ; R3 ne change pas le nommage ; côté 2.1, la tâche se limite à `saves.rs` / `workcopy.rs` et à l'UI ; si `emulator.rs` ou `media.rs` doivent changer, ces changements se font **après** la fusion de R3 (ou sont confiés à R3) |
| H7 | U3 coupée en deux (U3a fichiers neufs, U3b fichiers existants) pour démarrer avant M1 | U3a n'ajoute à `lib.rs` / `computer/mod.rs` que des lignes `mod` (conflit trivial avec T2-T4) ; aucune dépendance nouvelle avant M1 |
| H8 | Trois `target/` et 6 agents sur 7 Go de RAM | `CARGO_BUILD_JOBS=2` dès que C existe ; pas de build `--release` ni d'image Docker pendant les vagues 2 à 4 ; C abandonné si le disque passe sous 10 Go |
| H2 | Deux agents `ui` (A : T3 sur `app.rs` ; B : U3) | U3 ne touche ni `app.rs` ni `config*.rs` ni `gamepad/` ; il attend M1 pour `Cargo.toml` et `lib.rs` |
| H3 | `Cargo.lock` (gilrs en 2.1, `opener` / `rfd` en 3.0) | Conflit attendu à M1 : régénérer, ne jamais fusionner à la main |
| H4 | Une rc 4.0 (schéma v5) essayée, puis retour à une rc 3.0 (v4) | Déjà géré : base plus récente refusée avec un message, jamais modifiée ; à dire dans les guides d'essai |
| H5 | Disque (93 %) : deux `target/` complets + assets de test de 5 versions | `fetch-test-assets.sh` ne télécharge que le bloc demandé ; `rm -rf target/debug/incremental` entre les vagues ; `docker builder prune` si < 10 Go (accord utilisateur) |
| H6 | Écart 2.0 découvert : le téléchargement des DAT (`library/worker.rs`) utilise la base No-Intro pour tous les systèmes et ignore `SystemInfo::dats` ; deux sources d'un même système donnent le même nom de fichier | Corrigé en 3.0 par U4 (boucle par source, dossier local `database/<source>/`) ; design 3.0 §10 |

## 7. X0 de la 3.0 : ce qui a été posé (M0)

Contrat posé sur `wip-3.0`, `cargo check --workspace --all-targets` vert, comportement 2.0 inchangé (les chemins
existants délèguent ; seules les fonctions neuves sont en `todo!()`). Détail et écarts avec le design : design 3.0
§10. Fichiers de la zone en cours de modification sur `wip-2.1` touchés : **`crates/emu-ui/src/app.rs` seulement**,
un bloc (`App::build`, `matches!` sur `ContentSpec::Media { rules, .. }`).

## 8. Mega CD express dans la 2.1 : contraintes d'harmonie (à respecter par son design)

Le design (`docs/design/v2.1-megacd-express.md`, sur `wip-2.1`) n'existait pas encore à l'écriture de cette section ;
harmonie le relit dès qu'il existe. Il avance d'une partie de la 6.0 avant la 3.0 : il doit donc **n'utiliser que les
mécanismes de la 2.0** et rester un sous-ensemble strict de ce que la 6.0 (ADR 0020) fera avec les mécanismes de la
3.0. Contraintes :

| Sujet | Contrainte pour la 2.1 express | Reprise |
|---|---|---|
| `systems.toml` | Un bloc `megacd` **en fin de fichier**, clés 2.0 seulement (`id`, `name`, `extensions`, `database`, `screenscraper`, `igdb`, `dat`), `id = "megacd"` et `database = "Sega - Mega-CD - Sega CD"` (ceux de la 6.0, **persistés** : ne plus les changer). Pas de `folder_extensions` (3.0) : les extensions `cue` / `chd` / `iso` ne sont partagées avec aucun système avant la 4.0, donc aucune ambiguïté en 2.1 | B3 ajoute `amiga` **après** ce bloc (conflit de fusion trivial à M1) ; DB1 (4.0) rend `cue` / `chd` / `iso` restreintes au dossier pour `dos` et `megacd` (`folder_extensions`, design 3.0 §3.5.2) ; GB1 (6.0) ajoute Redump, la sonde `SEGADISCSYSTEM` et la région |
| `cores.toml` | `[systems."Sega - Mega-CD - Sega CD"] cores = ["genesis_plus_gx", "picodrive"]`, clés 2.0 seulement ; **pas** de `required_firmware` (clé 3.0 : l'analyseur de la 2.1 la refuserait) | A3 n'y touche pas ; **GA1 (6.0)** ajoute la table `required_firmware` par région |
| `firmware.toml` | **Rien à ajouter** : `bios_CD_E/J/U.bin` y sont depuis la 1.0 | — |
| Contrôle du BIOS avant lancement | Le contrôle `.info` de la 1.0 (`check_firmware`, avertissement « lancer quand même ») ; au plus un message qui nomme le fichier attendu. **Ne pas** poser de `check_required`, de `FirmwareCheck` ni de variante « région » sur `wip-2.1` : ces noms sont posés sur `wip-3.0` (X0, 0a1ff5a), un doublon donnerait un conflit de fusion sémantique | 6.0 : contrôle unique par région (données) |
| `.m3u` multi-CD | En 2.0, `.m3u` est une extension partagée attribuée par dossier, sinon au premier système du catalogue (`atarist`) : un `.m3u` de CD n'est donc reconnu `megacd` que **sous un dossier `megacd/`**. À dire dans le guide ; ne pas réécrire l'attribution (c'est B3, design 3.0 §3.5.2) | B3 : système d'un `.m3u` = celui de ses entrées |
| Schéma de la bibliothèque | **Aucune migration** en 2.1 (la v4 appartient à B3, déjà lancée) : pas de `game.region`, pas de table nouvelle | v6 (GB1) |
| Bibliothèque : `.cue` + pistes `.bin` | Risque principal : sans règle, chaque piste `.bin` devient un « jeu » (ou `.bin` est pris pour une ROM Mega Drive). Règle minimale permise : dans `scan.rs`, un `.bin` / `.iso` référencé par un `.cue` du même dossier n'est pas listé. À écrire comme une fonction isolée (`cue_references`, nom et signature du design 4.0 §3.4) pour que DB1 la **reprenne sans la réécrire** | DB1 (4.0) : règles CD génériques ; GB1 : Redump, `disc_serial` |
| Fichiers de code | `emu-library/src/{scan,systems}.rs` sont aussi modifiés par **B3 (en cours en B)** : la tâche express y fait des ajouts localisés (fonction neuve + un appel), signalés à harmonie pour la fusion M1. `emu-runtime/src/media.rs` et `emulator.rs` (R3 en cours) : **ne pas y toucher** ; un `.cue` / `.chd` se lance par le chemin 1.0 (`ContentSpec::File`, `needs_fullpath`), un jeu multi-CD par `.m3u` (2.0). `app.rs` : après T3, même agent `ui` | — |
| Sauvegardes | RAM interne du Mega CD : fichier `.brm` écrit par le core dans `saves/` ; rien à coder. Ne pas ajouter `game_saves` (clé 3.0) | 6.0 : `game_saves = ["{game}.brm"]` |
| Import du BIOS | Pas de bouton « Importer un BIOS… » en 2.1 (c'est A3 + U4, 3.0) : dépôt manuel dans `system/`, à expliquer dans le guide d'essai (`docs/bios-utilisateur.md`) | 3.0 |
| Tests | Aucun BIOS ni jeu commercial : image CD générée (`tests/tools/mkcd.py`, fichier neuf, repris tel quel par la 6.0) ; lancement réel non testable sans BIOS ⇒ tests de scan et de choix du core seulement, essai réel par l'utilisateur (Dune) | GQ1 / GQ2 |

Ordre des propriétaires amendé (§2) : `systems.toml` et `cores.toml` commencent par la **2.1 express**, puis B3 / A3.
Conséquence pour la 6.0 : le périmètre Mega CD de l'ADR 0020 devient « compléter » (région, Redump, contrôle par
région, sonde, `.brm` par jeu) ; à noter dans l'ADR 0020 quand le design express est accepté.

Conflits à surveiller à **M1** : `emu-library/src/scan.rs` (express ↔ B3), `data/systems.toml` (deux blocs ajoutés
en fin), `crates/emu-ui/src/app.rs` (bloc de X0), `Cargo.lock`. Aucun autre attendu si les contraintes ci-dessus
sont tenues.

## 9. Relecture du design 2.1 « identité de jeu, disquettes ajoutées » (risque H1)

`docs/design/v2.1-identite-disques.md` (sur `wip-2.1`) : **bon pour lancer I0 → I1 / I2 → I3**, aux conditions
ci-dessous. Il est cohérent avec la 3.0 : `save_name` reste le nom nu pour les jeux non homonymes (aucun nommage
nouveau dans R3), `Emulator::content_name()` est exactement le `{game}` du §3.5.3, aucune migration de schéma.

**R3 n'a rien à changer** : `reset_core_game_files(dir, content_name, motifs)` ne regarde que le dossier reçu, non
récursif ; c'est l'appelant (U4) qui passera `identity.core_save_dir` au lieu de `<saves>/<core_id>`. Le premier
paramètre se lit donc « dossier de sauvegarde donné au core pour ce jeu ».

| # | Condition / point de fusion | Pour |
|---|---|---|
| C1 | Sur `wip-2.1`, tout code neuf qui filtre `ContentSpec::Media` écrit `{ set, .. }` et ne nomme jamais le champ `policy` (devenu `rules` sur `wip-3.0`) ; `prepare_media` n'est pas appelée par du code neuf hors `emulator.rs` | I0, I1 |
| C2 | `lib.rs` d'`emu-runtime` : les exports nouveaux vont dans des lignes `pub use` **séparées** (la liste `media::{…}` a déjà changé sur `wip-3.0`) | I0 |
| C3 | I2 : le code des disquettes ajoutées et des orphelins (`added`, `forget_added`, `adopt_orphan`, `orphan_copies`, `remove_orphan_copies`) et ses tests vont dans un **fichier neuf** (`added.rs`, `impl Disks<'_>` y compris) ; dans `media.rs`, seulement les retouches indispensables de `stage_added_copy` / `Disks::add`. R3 modifie le haut du fichier (`prepare_media_with`, `Stage`, listes `.m3u`) et y ajoute des tests | I0, I2 |
| C4 | `emulator.rs` : `load_content_as` est une fonction neuve ; le bloc `ContentSpec::Media { … }` de `load_content` (modifié par X0) n'est retouché que pour `set.name` | I1 |
| C5 | `EmulatorOptions` gagne un champ : 19 littéraux `EmulatorOptions { … }` existent, d'autres naissent en B (tests de R3, Q5) ; `StateHeader` de même (2 hors `saves.rs`). À M2, harmonie complète les littéraux de `wip-3.0` (conflit sémantique, pas textuel) | M2 |
| C6 | `mock_core.c` : I4 (« expose le dossier de sauvegarde reçu ») et Q4 (déjà commité sur `wip-3.0`, ecc023a) modifient le même fichier ⇒ conflit textuel à M2, à résoudre en gardant les deux comportements ; I4 garde chaque ajout derrière sa propre variable `MOCK_*` et n'en réordonne aucune | I4, M2 |
| C7 | `app.rs` : I3 réécrit le début de `App::build` (`content_spec` → identité → options), là où X0 a changé le calcul de `working_copy` : conflit attendu à M2, résolu par harmonie (garder la forme `rules.policy`) | M2 |
| C8 | **Ordre dans la 2.1** : le design place le Mega CD express « après I3 » ; la priorité utilisateur (essai de Dune demain) impose l'inverse : T3 / T4 → **Mega CD express (UI)** → `v2.1.0-rc.1` ; I3 ensuite (dans la rc.1 seulement s'il est vert et revu à temps, sinon rc.2). I0, I1, I2 (`emu-runtime`) peuvent tourner en parallèle de l'express, qui ne touche pas ce crate | session principale |
| C9 | U4 (3.0) a besoin de `GameIdentity` et de `content_name()` : **M2 se fait après I1-I3**. Si l'identité glisse en rc.2, faire une fusion intermédiaire `wip-2.1` → `wip-3.0` dès qu'I2 est vert, pour ne pas retarder U4 | M2 |
| C10 | 5.0 : `save_scope = "game"` devient « sous-dossier `<save_name>/` pour tous les jeux du core » ; à reporter dans le design 5.0 lors de sa re-validation | architecte |

