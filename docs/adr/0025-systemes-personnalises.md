# ADR 0025 — Version 8.0 : systèmes personnalisés (systèmes libretro non testés)

Statut : **accepté, amendé pour la fusion le 2026-11-25**, puis **le 2026-11-26** (numérotation 8.x, marque de scan en
base, journal version 2 : « Amendement du 2026-11-26 » ci-dessous) et **le 2026-11-27** (textes de PD1, fusion des
données au scan, essai à blanc : « Amendement du 2026-11-27 ») ; les amendements **priment**. Historique :
accepté le 2026-11-25 par l'utilisateur, sur la ligne PC (`wip-7.1`), après la revue indépendante et quatre relectures courtes ;
réponses aux questions 8.0 et R1-R3 dans `docs/questions-ouvertes.md`).
Périmètre : `docs/specs/v7.1-v8.0-utilisateur.md` §8.0 et feuille de route (ligne 8.0) ; étude
`docs/essais-systemes-personnalises.md`, dont les réponses de l'utilisateur (§10, 2026-11-21) **font foi** : défaut
recommandé partout, sauf **Q15** (export / import d'une définition : **reporté**, à rappeler à la conception de la
8.4, palier CD) et **Q16** (un système devenu officiel : la définition de l'utilisateur **reste en place**, emu-rust
**propose la bascule, recommandée** ; jeux, sauvegardes et touches conservés). Essais :
`docs/plan-essais-8x-9x.md`.
Précise ADR 0001 (un core par processus), 0005 (règle « Builtin », amendée le 2026-11-27 : lot PD1), 0013 (cores
manuels, `.info`, BIOS), 0014 (catalogue, DAT, jaquettes), 0022 (pas de processus enfant par partie), 0023
(rangement ; journal en version 2 pour la bascule, amendée le 2026-11-27 : lot PD1). Conception et découpage :
`docs/design/v8.0-systemes-personnalises.md`.
**Graphe des crates inchangé. Aucune dépendance de crate nouvelle (la sérialisation de l'enfant d'essai reste dans
`emu-ui`, qui a déjà `toml`). Les systèmes personnalisés n'ajoutent aucune migration de `library.sqlite` ; la 8.0
passe pourtant en v8 pour la marque de scan (amendement du 2026-11-26).** `unsafe` nouveau : `emu-libretro` (copie
de deux structures C) et `emu-runtime` (`SetErrorMode` sous Windows, §5). Aucun hôte réseau nouveau.

## Amendement de fusion (2026-11-25) — base Mac, paliers renumérotés

La conception a été écrite contre le code de `wip-7.1` (ligne PC). Après la fusion des deux lignes
(`.claude/fusion-plan.md`, décisions D1-D15 de l'utilisateur), les points suivants priment sur le texte :

- **Schéma** : la base est en **v7** (`game.needs_32x`, 7.4) ; les systèmes personnalisés n'ajoutent toujours aucune
  migration (la v8 de la 8.0 vient de la marque de scan : amendement du 2026-11-26).
- **Rangement** : livré en **7.6** (et non 7.1) ; la bascule avec déplacement (§10, Q5) en dépend toujours. Le
  glisser-déposer (« 7.2.1 ») vient **après la 7.6**, sans numéro fixé.
- **Paliers renumérotés (D10)**, numérotation **remplacée le 2026-11-26** (amendement ci-dessous) : **8.0**
  cartouches / fichier unique ; **8.4** CD ; **8.5** ordinateurs ; **8.6** arcade et entrées exotiques ; le core
  jgenesis est la 8.3. Le corps du texte est corrigé en place (messages de refus compris, §9) ; Redump et le
  rappel de Q15 (export / import) passent à la 8.4.
- **PC Engine** (roadmap Mac du 2026-11-24 : « PC Engine / CD-ROM² inclus » en 8.0) : la **PC Engine à cartes
  (HuCard)** est le système personnalisé de référence des essais de la 8.0 (core par défaut du buildbot, p. ex.
  beetle_pce_fast) ; le **CD-ROM²** est un format disque et suit le palier CD (**8.4**). jgenesis reste une
  alternative au choix (8.3), jamais le défaut.
- **Style** : tout écran nouveau (ajout d'un système, liste, bascule) naît dans le thème de la 7.3 (`theme::`,
  jetons sans `Color32` brut) ; l'assistant de premier lancement (7.3) n'est pas modifié par la 8.0.
- **Recalage du code (lot L10)** : les relevés « dans le code de `wip-7.1` » (§ Contexte) et les fichiers cités par la
  conception sont à revérifier contre `main` après la 7.6 (`GameNeeds`, `Page`, `[app]` / `[ui]` de
  `config.toml`, `theme.rs`, apprentissage de manette 7.2) **avant tout code 8.0**. Le fond de l'ADR ne change pas.
- **Effet sur chaque système livré** (§14) : inchangé ; pour `megacd`, la marque CD 32X (`needs_32x`, marque
  manuelle) n'est pas touchée par la 8.0.

## Amendement du 2026-11-26 — numérotation 8.x, marque de scan, journal version 2

Décisions de l'utilisateur du 2026-11-26 (18:01) et recalage de la conception sur le code de la 7.6
(`wip-7.6-merge`). Ces points priment sur le texte et sur l'amendement de fusion.

- **Numérotation** : la 8.0 part **après la 7.6**, sans attendre les archives `.7z` ni l'interface en anglais.

  | Version | Contenu | Ancien numéro |
  |---|---|---|
  | 8.0 | systèmes personnalisés : cartouches / fichier unique | 8.0 |
  | 8.1 | archives `.7z` (ADR 0027, proposée) | 7.7 |
  | 8.2 | interface en anglais | 7.8 |
  | 8.3 | core jgenesis | 8.1 |
  | 8.4 | systèmes personnalisés : CD (Redump, rappel de Q15) | 8.2 |
  | 8.5 | systèmes personnalisés : ordinateurs | 8.3 |
  | 8.6 | systèmes personnalisés : arcade et entrées exotiques | 8.4 |
  | 9.0 | rendu matériel (ADR 0026), **après toute la 8.x** (après la 8.6) | 9.0 |

  Les messages de refus disent donc « prévu en 8.4 » (disque), « prévus en 8.5 » (clavier complet et souris),
  « prévu en 8.6 » (arcade) et « prévu en 9.0 » (rendu matériel) ; le corps du texte est corrigé en place.
- **Le `.7z` suit la 8.0 au lieu de la précéder** : `7z` reste une extension d'archive réservée dès la 8.0 (§9,
  jamais une extension de définition) ; tant que la 8.1 n'est pas livrée, un `.7z` n'est pas identifié, comme en
  7.x. La 8.1 se recale sur le scan et le rangement de la 8.0 ; un jeu personnalisé en `.7z` suit alors son chemin.
- **Textes** : la 8.0 écrit ses textes en français dans les `text.rs` de module ; la 8.2 les traduit avec les autres.
- **Marque de scan en base (schéma v8)** — réponse de l'utilisateur du 2026-11-25 (« marqueur maintenant, marque
  en base en v8 ») : la 7.6 pose un fichier `library.sqlite.scanning` pendant un scan ; s'il reste, le scan suivant
  ne purge rien (`ScanReport::cautious`). La 8.0 le remplace par une colonne `game.scan_pending` : les lignes
  écrites par un scan qui n'a pas fini sont connues exactement, le scan suivant les rapproche des fichiers déplacés
  et purge normalement. C'est une **migration v7 → v8** (sans perte), hors du sujet des systèmes personnalisés, qui
  n'en demandent toujours aucune (§13). Conséquence : une base v8 est refusée par une 7.x (`NewerSchema`, base
  jamais modifiée), comme après chaque migration. Lot PB0 de la conception. **Confirmé par l'utilisateur le
  2026-11-26** (marque en base dans la 8.0).
- **Journal version 2, retour à une 7.x** (correction d'un fait, Conséquences) : le code de la 7.6 refuse tout
  rangement et toute annulation tant qu'un journal illisible pour lui est **présent** dans `<data>/rangement/`,
  pas seulement s'il est le plus récent, et ne le purge jamais. La décision ne change pas (confirmé par l'utilisateur
  le 2026-11-26) (version 2 réservée à la
  bascule, aucune bascule réelle en 8.0) ; le guide d'essai dit quel fichier mettre de côté.

## Amendement du 2026-11-27 — textes de PD1, fusion des données au scan, essai à blanc

Ces points priment sur le texte et sur les amendements précédents.

- **Lot PD1 fait** : ADR 0005 amendée (§7 ci-dessous : acceptation une fois par core), ADR 0023 amendée
  (« Amendement du 2026-11-27 » : grammaire complète du journal version 2, états, refus, reprise et fin de bascule,
  retour à une 7.x, marque de scan au §4.5). La grammaire attend la relecture PV3 avant PB3.
- **Marque de scan, fusion des données** (décision de l'utilisateur du 2026-11-27) : une ligne marquée qui porte des
  données de l'utilisateur n'est jamais absorbée ; si elle est la jumelle **unique** d'une ligne dont le fichier a
  disparu, elle reçoit ses données avant la purge — favori = le plus fort, parties additionnées, dernière date la
  plus récente, jaquettes : `cached` l'emporte sur `missing`. Rien si le couple est ambigu, si le scan est prudent
  (fichier `.scanning` d'une 7.6) ou annulé.
- **Essai à blanc, dossier des copies privées** (relecture du lot PR1 ; choix technique réversible, tranché par la
  session principale) : pour tenir « un essai ne touche jamais les données de l'utilisateur » (§5), l'enfant
  n'écrit que sous le dossier de l'essai (`scratch_dir`) ; un `copy_dir` (copie privée d'un core résident,
  ADR 0022) n'est accepté que s'il est **dans** `scratch_dir`, sinon la requête est refusée. Détail : design 8.0
  §2.4.
- **Rendu matériel facultatif** (limite, conforme au §8) : un core qui a fait une demande `SET_HW_RENDER` (même
  facultative, avec repli logiciel) et dont le chargement du jeu échoue ensuite (ROM abîmée, par exemple) est
  rapporté `HardwareRenderRequired` (« prévu en 9.0 ») et non « échec du chargement » : on ne sait pas distinguer
  les deux causes. Écrit dans le guide d'essai 8.0.
- **Relecture PV3 du journal version 2** (2026-11-27, aucun bloquant) : ADR 0023 précisée (« Amendement du
  2026-11-27 ») — clos = dernière ligne `E`, `U`, `F` ou `I` ; `SwitchPending` indépendant de la racine ;
  déplacement de la définition sans écrasement, dossiers synchronisés avant `F`, noms exacts ; Réglages bloqués
  pendant tout journal de bascule ouvert, sauf le retrait proposé en cas de « place prise » ; annulation refusée si
  le fichier retiré manque ; marque de scan gardée sur les lignes avec données après un scan annulé ou prudent.
  §10.4 et §10.5 corrigés en place.

## Amendement du 2026-11-27 (soir) — relecture du lot PB2

Choix sûrs et réversibles tranchés par la session principale, signalés à l'utilisateur ; ils priment sur le texte et
sur les amendements précédents. Détail, signatures et tests : design 8.0 §0.9, §2.1, §2.6, §4.

- **Lignes d'un id en conflit gelées** (M1) : quand deux systèmes ont pu écrire des lignes sous un même id — officiel
  masqué par une définition de même id, ou définition en erreur portant l'id d'un officiel actif —, une ligne de cet
  id que le système actif n'attribuerait pas (extension du chemin, entrées d'un `.zip`) n'est jamais purgée ni
  déclassée : elle est gelée comme une ligne de système absent (§13). Sans cela, un `psx.toml` incertain limité à
  `xyz` ferait purger les jeux PS1, favoris et historique compris.
- **Double conflit** (id d'un officiel A, `database` d'un officiel B) : définition refusée au chargement
  (`DuplicateDatabase`), B n'est pas masqué (§10).
- **« Restaurer » et `Replaced`** : `restore_custom` accepte les deux natures (l'annulation d'une bascule restaure un
  `Replaced`) ; c'est l'interface qui ne propose « Restaurer » que pour `Removed` (§9).
- **Borne de 256 définitions** : chaque fichier au-delà devient un problème `TooManyDefinitions`, affiché comme les
  autres, son id réservé au rangement ; une nouvelle définition est refusée à la borne ; listage borné, trié avant
  troncature (§2).
- **Dossiers réservés comparés sans tenir compte de la casse ASCII** (Windows, macOS ; §12).
- **Suppression en conflit incertain** : le contrôle « aucun jeu sous `roms/<id>/` » porte sur les fichiers du disque
  que la définition revendique (`custom_games_on_disk`, emu-library, en tête de PB3) ; dialogue en PU3 (§9).
- **« Oublier ses jeux » en conflit incertain** : n'oublie que les lignes revendiquées par la définition et que
  l'officiel masqué n'accepte pas (`ForgetScope::ClaimedBy`, lot PB3, avant PU3 ; §9).
- **Problèmes `TooManyDefinitions` regroupés** dans l'interface : une ligne « N définitions ignorées au-delà de
  256 », liste repliée (PU3).
- **Annulation et dossiers réservés** (PB3) : une annulation ne prend ni ne pose rien dans un dossier réservé et ne le
  retire jamais, même vide ; seuls les deux dossiers d'une bascule en sont exemptés (ADR 0023, amendement du
  2026-11-27).
- **Relecture PB2, tour 2** (code de référence) : un `.zip` d'un id en conflit est jugé sur l'entrée enregistrée en
  base, jamais sur le disque (zip disparu à entrée acceptée : purgé ; sans entrée connue : gelé) ; les **lignes
  gelées sont masquées** dans la bibliothèque et non lançables avec le core de la définition (PU3, §13) ; une
  **bascule acceptée ne purge jamais** les lignes que l'officiel n'accepte pas : listées dans l'aperçu, puis gelées
  jusqu'à « Oublier ses jeux » ou restauration (PB3, §10) ; `custom_games_on_disk` ouvre le lot PB3 ; dossiers
  réservés reconnus à toute profondeur sous `roms/<id>/`, limite acceptée (§12).

## Amendements après revue (2026-11-25)

Revue indépendante : `VERDICT : À AMENDER` (1 bloquant, 5 majeurs, 15 mineurs). Changements :

- **B1 (Q16)** : une définition valide en conflit avec un système officiel (même id **ou** même `database`) reste
  **active** et masque l'officiel ; la bascule est proposée dans tous les cas, reportée avec un message si elle
  exige un déplacement de dossier (remplacé par la réponse Q5 : déplacement par le rangement) ; « en erreur » est réservé aux fichiers invalides ; tant que la définition est
  active, sa liste de cores prime sur `cores.toml` ; les clés de sauvegarde de `cores.toml` sont gelées par un test
  (§10). Table des ids injective, entrées futures = réduction sans suffixe (§3).
- **M1** : liste explicite des extensions de disque de la 8.0 et palier par machine dans la table des ids (§2, §9).
- **M2** : `database` et `name` en liste blanche, ids réservés, plus de valeur sentinelle dans `database`
  (accesseurs `libretro_name()` / `core_key()`), bornes de lecture des définitions (§2).
- **M3** : durcissement de l'enfant d'essai (aiguillage avant toute initialisation, aucune boîte, sorties nulles,
  résultat borné, chien de garde, core vérifié, copie du jeu, dossiers par instance, rapports de plantage) (§5).
- **M4** : le scan ne supprime **jamais** les lignes d'un système absent du catalogue ; ordre de démarrage fixé,
  modes CLI compris (§2, §13).
- **M5** et mineurs : découpage, tests et critères corrigés dans le design ; mineurs traités dans le texte (m2, m3,
  m6-m9, m11, m12, m15 ici ; m1, m4, m5, m10, m13, m14 dans le design).

Deuxième relecture (`VERDICT : OK`, 4 mineurs) et **réponses de l'utilisateur du 2026-11-25** aux 12 questions
(`docs/questions-ouvertes.md`) ; changements :

- **Noms** : `info_name` seulement nettoyé (contrôle, bidi) ; `name` proposé normalisé avant validation ; noms
  libretro de la table hors liste blanche acceptés comme exceptions embarquées, fichier DAT local par
  `dat_file_stem` (§2).
- **Ids de la table réservés** à leur propre `database` ; bascule « incertaine » quand seul l'id coïncide (§3, §10).
- **Touches à la bascule** : clé officielle déjà présente et différente ⇒ demande, défaut « garder les siennes »
  (§10).
- **Q1 (non défaut)** : supprimer un système **garde ses jeux masqués** ; ils réapparaissent s'il est recréé ou
  restauré ; « Oublier ses jeux » est une action explicite séparée (§9, §13).
- **Q5 (non défaut)** : la bascule qui exige un déplacement passe par **le rangement** de la 7.1 (aperçu, accord,
  journal, annulation) ; précise ADR 0023 (en-tête de bascule, journal en version 2) (§10).
- **Q12 (non défaut)** : la liste des ids n'est pas montrée à l'utilisateur ; l'architecte la relit seul (§3).
- Autres réponses : défauts confirmés (Q2-Q4, Q6-Q11) ; Q8 précisée : un non-émulateur est ajoutable s'il accepte
  un format à fichier unique (ni disque, ni arcade) (§9).

Relecture courte du 2026-11-25 (`.claude/rv-8.0-2.md`, `VERDICT : À AMENDER`, 0 bloquant, 4 majeurs, 8 mineurs) ;
changements :

- **M1** : une bascule avec déplacement ne change le système des lignes et la définition **qu'en une fois** (ligne
  `F`), quand toutes les unités sont à destination ou toutes revenues ; bascule ou annulation incomplète : aucun
  jeu masqué ; annulation proposée seulement si toutes les unités peuvent revenir (§10).
- **M2** : dossiers des systèmes retirés, masqués ou en erreur réservés au rangement (§12).
- **M3** : supprimer une définition en conflit incertain passe par « Oublier ses jeux » (§9).
- **M4** : revue de la grammaire du journal version 2 avant l'exécution (PV3), puis du code (PV4) (Conséquences).
- Mineurs : choix des touches, préférence et profil antérieurs dans l'en-tête (m1) ; aucun chemin libre dans
  l'en-tête (m2) ; DAT gardés, jaquettes non défaites (m3) ; « Restaurer » exclut les définitions remplacées
  (m4) ; version 2 réservée à la bascule, retour à une 7.x écrit (m5) ; annulation limitée au dernier journal
  écrite (m6) ; restes de la « bascule reportée » retirés du design (m7) ; énoncé de Q8 rétabli (m8).
- Questions R1 à R3 ajoutées en fin de `docs/questions-ouvertes.md` (réponse par défaut appliquée ici).

Troisième relecture courte (`.claude/rv-8.0-3.md`, `VERDICT : À AMENDER`, 0 bloquant, 3 majeurs, 5 mineurs) ;
changements : ids actifs jamais réservés, exception de `plan_switch` (M1, §12, §14) ; nombre d'unités du plan dans
l'en-tête, fin due = toutes validées (M2) ; même portée de réécriture à l'aller et à l'annulation, toutes les
lignes (M3) ; fin de bascule avant les identités (m1) ; fin d'annulation seulement après un aller à `F` (m2) ;
définition recréée, échec de la fin de bascule (m3) ; suppression en conflit incertain : DAT gardés, refusée tant
que des jeux restent dans le dossier (m4, R2 reformulée) ; annulation partielle : déplacement à la main écrit
(m5). Contrat 9.0 : `HwContextKind::Direct3D11`, `HwRenderRequest` `#[non_exhaustive]`.

## Contexte

Relevé dans le code de `wip-7.1` (2026-11-25) :

- `emu_library::systems()` rend un `&'static [SystemInfo]` lu une fois (`OnceLock`) de `data/systems.toml` embarqué ;
  `system(id)` et `systems_for_extension` rendent des `&'static SystemInfo`. 154 appels dans 30 fichiers, dont une
  cinquantaine dans `scan.rs`, `db.rs` et `tidy/` ; `SystemInfo` n'est construit que dans `systems.rs`.
- `SystemInfo::dats` est « jamais vide » ; les DAT sont téléchargés vers `<database dir>/<source>/<database>.dat` ;
  les jaquettes libretro utilisent `database` comme nom de dépôt ; une vingtaine de lectures de `.database` dans
  `emu-ui`.
- `CorePreferences` (emu-assets) associe une clé de système à une liste de cores (`cores.toml` embarqué, surcharge
  `[cores.preferred]` indexée par id de système). Les propriétés par core (`media`, `save_scope`, `game_saves`…)
  s'appliquent quel que soit le système. `emu-assets` et `emu-library` ne se connaissent pas ; `emu-ui` les relie.
- `CoreInfo` ne lit ni `systemname`, ni `categories`, ni `hw_render`, `required_hw_api`, `is_experimental`,
  `supports_no_game`, `needs_kbd_mouse_focus`. `CoreManager::infos()` rend déjà tous les `.info` du dernier
  `info.zip`.
- `emu-libretro` refuse `SET_HW_RENDER` et le journalise seulement ; il accepte `SET_INPUT_DESCRIPTORS` et **jette**
  les noms de boutons. La sentinelle `RETRO_HW_FRAME_BUFFER_VALID` est ignorée. `SET_ROTATION` n'est pas géré.
- `PadStyle::from_core_id` donne `Generic` à tout core inconnu ; la clé de profil de touches est l'id du core.
- Un plantage de core ferme l'application (ADR 0022 a écarté le processus enfant **par partie**). Les tests réels
  lancent déjà un processus enfant par core. `emu-app` sait relancer son propre exécutable (ADR 0023 §3) ; en
  sous-système graphique Windows, ses chemins d'erreur ouvrent une boîte de message.
- `DISC_EXTENSIONS` (`emu-library/src/disc.rs`) ne contient que `cue`, `chd`, `iso`.
- Le rangement (ADR 0023) crée `roms/<id>/` pour chaque système du catalogue et classe par bibliothèque, sonde,
  DAT, contenu d'archive, nom, puis extension propre à un seul système.

## Décision

### 1. Catalogue dynamique : instantanés immuables publiés à l'exécution

Le catalogue devient une valeur `Catalog` immuable (systèmes officiels embarqués, puis systèmes personnalisés).
L'application en publie un nouvel **instantané** à chaque changement ; `catalog()` rend l'instantané courant.

| Option | Effet | Coût | Verdict |
|---|---|---|---|
| (a) `Arc<Catalog>`, `Arc<SystemInfo>` partout | propre, mémoire rendue | 154 appels, 30 fichiers, durées de vie à revoir ; conflits avec tout lot parallèle | rejetée |
| (b) `&Catalog` passé explicitement à toute fonction | le plus testable | signatures de tous les crates hauts ; même coût que (a) | rejetée pour la 8.0 (évolution possible ensuite) |
| (c) catalogue lu au démarrage, redémarrage exigé après un ajout | aucun changement de type | contraire à l'ajout « en un clic » (Q4) | rejetée |
| **(d) instantanés `&'static` publiés par fuite bornée** | `systems()`, `system()`, `systems_for_extension()` **gardent leur signature** | interne à `emu-library` ; 20 à 50 Kio par publication | **retenue** |

- `install_catalog(Catalog) -> &'static Catalog` place l'instantané dans un `Box::leak` et publie le pointeur
  (`RwLock<&'static Catalog>`). Un instantané n'est jamais modifié ni libéré. **Rien n'est publié si le catalogue
  est égal au courant** ; un avertissement est journalisé au-delà de 1 000 publications dans un processus.
- **Cohérence** : scan, rangement, import des DAT et jaquettes capturent **un** instantané au départ et ne lisent que
  lui. Les méthodes publiques de `Library` (`db.rs`) lisent `catalog()` à chaque appel. La publication n'est
  **différée** que pendant un scan ou un rangement (ils écrivent des systèmes en base) : elle attend leur fin (file
  d'une publication, la dernière gagne) ; l'import des DAT et les jaquettes ne la bloquent pas.
- **Migration** : seuls les appels internes de `scan.rs`, `db.rs` et `tidy/` passent à l'instantané capturé ;
  `attribute(ext, folders)` garde sa signature en enveloppe (tests existants intacts) ; les appels d'`emu-ui` et
  d'`emu-app` ne changent pas. Livrée **seule, sans changement de comportement** (lot PB1).
- **Tests** : aucun test unitaire n'appelle `install_catalog` (dans aucun crate) ; un test qui a besoin d'un système
  personnalisé construit un `Catalog` et appelle une fonction qui le prend en paramètre, ou vit dans un fichier de
  test d'intégration seul dans son processus, cas sérialisés.

### 2. Définitions utilisateur : un fichier TOML par système sous `<data>/systems/`

- **Emplacement** : `<data>/systems/<id>.toml` (`DataDirs::custom_systems`). Pas dans `config.toml` : secrets et
  droits `0600` à part, taille bornée à 1 Mio, et un fichier par système rend l'export futur (Q15) trivial et la
  suppression atomique. Fichiers retirés : `<data>/systems/.retired/<id>-<unix>.toml` (par l'utilisateur) ou
  `<id>-<unix>.replaced.toml` (par une bascule), seuls noms acceptés, jamais effacés.
- **Propriétaire du format** : `emu-library`. La liste des cores y figure en texte (syntaxe d'id de core vérifiée) ;
  `emu-ui` la convertit en `CoreId` et la passe à `emu-assets` (`CorePreferences::with_custom_systems`).
- **Format 1** (seule version écrite en 8.0) :

  ```toml
  format = 1                 # > 1 : « créée par une version plus récente », refusée, fichier intact
  render = "software"        # seule valeur acceptée en 8.0 ; la 9.0 ajoutera "hardware"

  [system]
  id = "pcengine"            # [a-z0-9]{1,24}, = nom du fichier, figé à la création
  name = "PC Engine"         # affiché, modifiable
  database = "NEC - PC Engine - TurboGrafx 16"   # nom libretro ; absent si le core n'en donne pas
  extensions = ["pce"]       # toutes valables SOUS roms/<id>/ SEULEMENT (Q6)
  dat = ["no-intro"]         # sources trouvées (§9), peut être vide ; no-intro, tosec, libretro en 8.0
  screenscraper = 31         # facultatif
  igdb = 86                  # facultatif

  [cores]
  list = ["mednafen_pce_fast"]        # le premier est le core par défaut
  ignored_firmware = []               # fichiers « ne plus avertir » (§6)

  [origin]
  core = "mednafen_pce_fast"
  info_name = "NEC - PC Engine / CD (Beetle PCE FAST)"
  created_by = "8.0.0"
  created_unix = 1764028800
  ```

- **Lecture bornée** : au plus 256 fichiers `*.toml` lus dans `<data>/systems/`, les premiers par nom après un
  listage lui-même borné (au-delà : non lus, un problème `TooManyDefinitions` par fichier, affiché comme les autres,
  id réservé au rangement ; amendement du 2026-11-27 soir) ; une nouvelle définition est refusée à la borne ;
  fichiers **réguliers** seulement (`symlink_metadata` : ni lien, ni FIFO, ni périphérique) ; lecture par
  `take(64 Kio + 1)`, plus grand ⇒ `TooLarge`.
- **Validation** (fichier local mais modifiable à la main : donnée non fiable) : `deny_unknown_fields` ;
  - `id` : `[a-z0-9]{1,24}`, égal au nom du fichier, ni nom réservé de Windows (`con`, `prn`, `aux`, `nul`,
    `com1`-`com9`, `lpt1`-`lpt9`), ni nom d'un dossier de `<data>` (`roms`, `system`, `saves`, `states`, `cores`,
    `cache`, `logs`, `covers`, `database`, `rangement`, `systems`, `assets`, `gameoptions`) ;
  - `name` (≤ 64) et `database` (≤ 128) : **liste blanche** — lettres, chiffres, espace, `-`, `_`, `+`, `.`, `,`,
    `'`, `&`, `!`, `(`, `)` ; jamais `/ \ : * ? " < > |`, ni `..`, ni caractère de contrôle ou bidi, ni espace ou
    point en début ou fin (le nom `database` devient un nom de fichier local sous Windows). Le `name` proposé à
    l'ajout (tiré de `systemname` ou du `.info`, qui contiennent souvent `/` : « Sega 8/16 bit (various) ») est
    **normalisé avant validation** (caractère interdit ⇒ `-`, espaces réduits) : l'ajout en un clic n'échoue
    jamais pour un nom. **Exception** : un nom `database` présent dans la table du §3 (valeur embarquée de
    confiance) est accepté même hors liste blanche (cas possible : `/` dans un nom de machine, à vérifier par PB2) ;
    le fichier DAT local est alors nommé par `dat_file_stem` (caractère interdit ⇒ `_`), l'URL garde le vrai nom
    (encodée). Test : chacun des ≈ 165 noms de la table passe la liste blanche ou est une exception au
    `dat_file_stem` valide ;
  - 1 à 64 extensions `[a-z0-9]{1,8}`, jamais une extension d'archive, de liste ou de disque de la liste 8.0 (§9) ;
  - `dat` ⊂ {`no-intro`, `tosec`, `libretro`} (Redump : 8.4) ; 1 à 8 cores, `origin.core` compris, à la syntaxe
    d'id de core ; `info_name` ≤ 128 et `created_by` ≤ 32 caractères, **seulement nettoyés** des caractères de
    contrôle et bidi (texte affiché, jamais un chemin : `/` y est permis) ; `ignored_firmware` ≤ 64
    chemins relatifs sûrs (copie de la règle `is_safe_relative_path` d'`emu-assets` dans `emu-library`, égalité des
    deux vérifiée par un test d'`emu-ui`) ;
  - un id ou une `database` déjà pris par un **autre** système personnalisé ⇒ erreur ; un conflit avec un système
    **officiel** n'est **pas** une erreur (§10).
- **Fichier invalide** : le système apparaît « en erreur » dans Réglages → Systèmes personnalisés (raison lisible,
  id = nom du fichier) et n'entre pas au catalogue. Ses jeux ne sont pas perdus : le scan ne touche jamais aux
  lignes d'un système absent du catalogue (§13). Dossier `<data>/systems/` illisible : idem pour tous.
- **État propre à l'utilisateur, non exportable**, dans `config.toml` `[custom_systems]` : `content_accepted`
  (cores dont l'avertissement du §7 est accepté) et `works_for_me` (paires `<id>@<core>`, §9).

### 3. Identifiant : table « nom libretro → id et palier », règle pour les versions futures

- `emu-library/data/libretro_ids.toml` (embarqué) associe chacun des ≈ 165 noms `database` relevés par l'étude à un
  **id** et un **palier** (`cartridge`, `disc`, `computer`, `arcade`). Les 17 systèmes officiels y ont **leur** id.
  La table est **injective** (test), relue **par l'architecte seul** avant le tag (les ids nomment des dossiers
  définitifs ; l'utilisateur ne souhaite pas voir la liste, réponse Q12) et modifiée **en ajout seulement** ; une
  entrée n'est jamais renommée.
- Réduction déterministe d'un nom : minuscules ASCII et chiffres de la partie après le fabricant, 24 caractères.
  **Règle d'ajout** : toute entrée ajoutée après la 8.0 a pour id la réduction **sans suffixe** de son nom
  (test : « entrée ajoutée = réduction ») ; les collisions de réduction entre les 165 noms sont tranchées dans la
  table dès la 8.0.
- `proposed_id(database)` : l'id de la table ; à défaut la réduction, suffixée `2`, `3`… si elle est prise.
- **Un id de la table est réservé à sa propre `database`** : `proposed_id` et `validate_custom` le traitent comme
  pris pour tout autre système (autre `database` ou aucune). Un système sans `database` ne peut donc jamais occuper
  le dossier promis à une machine connue.
- Un système qui a une `database` reçoit **toujours** `proposed_id` (id non modifiable). Un système sans
  `database` (31 cores) reçoit un id dérivé de son nom, hors ids réservés, modifiable à la création seulement.
- **Règle pour les versions suivantes** : un système officiel ajouté plus tard prend l'id de la table (ou la
  réduction de son nom). Le cas normal d'une bascule est donc « même id, même `database` », sans déplacement ; les
  autres cas sont traités au §10.

### 4. Règle « le `.info` propose, le core installé corrige, l'utilisateur tranche »

| Champ | Proposé par le `.info` | Corrigé par le core installé (sonde, §5) | Tranché par l'utilisateur |
|---|---|---|---|
| Nom, machine(s) | `display_name`, `systemname`, `database` (une entrée par nom, Q5) | — | nom affiché |
| Extensions | `supported_extensions` | `retro_get_system_info` **remplace** la liste | ajouter / retirer |
| Rendu matériel | `hw_render`, `required_hw_api` (filtre de la liste) | demande `SET_HW_RENDER` (refus, §8) | non |
| BIOS | `firmwareN_*`, md5 des `notes`, `firmware.toml` | — | « ne plus avertir » par fichier |
| Démarrage sans jeu, manettes, disques | drapeaux | `SET_SUPPORT_NO_GAME`, `SET_CONTROLLER_INFO` | — |
| Noms des boutons | — | `SET_INPUT_DESCRIPTORS` (§11) | remappage |

Les valeurs retenues sont **écrites dans la définition** à la création ; ensuite le fichier fait foi. Une mise à
jour du core ne change jamais une définition en silence ; le bouton « Recalculer depuis le core » rejoue la règle.

### 5. Garde-fou contre les plantages : sonde et essai à blanc en processus enfant, témoin de lancement

ADR 0022 a écarté le processus enfant **pour toute la partie** ; il n'est pas rouvert. La 8.0 ajoute un enfant
**court et sans fenêtre**, technique déjà employée par les tests.

**Enfant d'essai** : l'exécutable lui-même (`current_exe`) avec l'argument caché `--dry-run-child <requête>`.

- **Aiguillage en premier** : `emu-app` reconnaît cet argument avant toute autre initialisation (préparation des
  dossiers, configuration, fichier journal, relance avec console, `ctrlc`) et n'ouvre **jamais** de boîte de
  message : toute erreur devient un code de sortie.
- **Sorties** : `stdin`, `stdout`, `stderr` de l'enfant à `Stdio::null()` (un core bavard ne remplit rien).
- **Requête et résultat** : deux fichiers TOML dans `<data>/cache/dry-run/<pid>-<n>/`, dossier créé par
  `create_new` (droits `0700` sous Unix) avec un fichier verrou ; au démarrage, seuls les dossiers dont le verrou
  est libre (instance morte) sont nettoyés. Le résultat est lu borné (64 Kio), `deny_unknown_fields`, puis
  re-borné par le parent (mêmes bornes que les descripteurs) ; **code 0 sans résultat lisible = plantage**.
  Structures miroirs sérialisables dans `emu-ui` (`emu-runtime` et `emu-libretro` restent sans `serde`). Chemin du
  core ou du jeu non UTF-8 : pas d'essai, avertissement « non testé » seul.
- **Core vérifié** : l'enfant charge le fichier que `plan_launch` a vérifié (SHA-256, ADR 0013) et marqué « en
  usage » pendant l'essai.
- **Jeu** : copié dans le dossier de l'essai s'il fait au plus 256 Mio (l'essai ne peut pas modifier l'original,
  même pour une image de disquette) ; plus grand : pas d'essai, avertissement seul. Sauvegardes, états et copies de
  travail vont dans ce dossier, retiré ensuite : **un essai ne touche jamais les données de l'utilisateur** ;
  `system/` est le vrai (BIOS).
- **Délai et chien de garde** : le parent tue l'enfant après 30 s ; l'enfant se termine seul après 35 s (fil
  dédié), même si le parent est mort.
- **Rapports de plantage** : sous Windows, l'enfant désactive la boîte d'erreur du système (`SetErrorMode`, dans
  `emu-runtime`, `// SAFETY:`) ; sous macOS, le dialogue « a quitté de façon inattendue » ne se supprime pas :
  limite notée, vérifiée aux essais n° 7 et 9.

**Sonde** (sans jeu) : à l'ajout, après l'installation. Lit `retro_get_system_info` et les drapeaux de
l'environnement. Un plantage ⇒ ajout refusé (« ce core se ferme au chargement »).

**Essai à blanc** (Q9) : avant le **premier lancement de chaque jeu** sur un core non testé, 300 images sans
entrée ni rythme. Mémorisé dans `<data>/cache/dry-runs.toml` (≤ 2 000 entrées, les plus anciennes retirées) par
(système, core, SHA-256 du core, chemin du jeu) : une mise à jour du core refait l'essai. Plantage **ou délai
dépassé** (un core incapable de 300 images en 30 s gèlerait l'interface) ⇒ même boîte : « Changer de core »
(défaut) ou « Lancer quand même » (réponse Q3). L'essai rapporte aussi la demande de rendu matériel et les
noms de boutons. Texte : « l'essai vérifie le démarrage seulement ».

**Témoin de lancement** (tous les cores, officiels compris) : avant `load_game`, `emu-ui` crée
`<data>/cache/launch-<pid>.tmp`, le **verrouille** (`File::lock`), y écrit core, système, jeu, « non testé », puis
le renomme en `launch-<pid>.marker` (le verrou suit le fichier ouvert) : une autre instance ne voit jamais un
témoin non verrouillé en cours de création. Retiré à la sortie propre du jeu. Au démarrage, un `.marker`
**verrouillable** est orphelin : message neutre « emu-rust s'est arrêté pendant (jeu) avec (core) — plantage, arrêt
forcé ou coupure » et, pour un core non testé, proposition de changer de core ; puis retrait. Un échec de création
du témoin ne bloque jamais un lancement (avertissement au journal).

**SRAM** : pour un système personnalisé, la mémoire de sauvegarde est comparée toutes les **60 s** à la dernière
version écrite (copie en mémoire, quelques centaines de Kio au plus) et écrite seulement si elle a changé
(`Emulator::flush_sram`, écriture atomique). Ajout de l'architecte, non demandé, au coût négligeable.

Limites assumées : l'essai à blanc n'attrape pas un plantage en cours de partie ; le test « deux chargements »
d'ADR 0022 §7 ne peut pas être fait pour un core inconnu (la copie privée automatique reste la protection).

### 6. BIOS

- Jamais téléchargés (inchangé). Liste = `firmwareN_*` du `.info`, md5 des `notes`, complétés par `firmware.toml`.
  États (ADR 0013 §8) : **valide**, **absent**, **différent**, **présent, non vérifiable** (accepté, Q10).
- **Requis absent** : avertissement avec « Lancer quand même » (Q11), plus « Ne plus avertir pour ce fichier »
  (`ignored_firmware`). Jamais de blocage : requis / facultatif est peu fiable.
- **Import** : « Importer un BIOS… » reconnaît un md5 connu ; sinon il propose les fichiers **attendus et absents**
  des systèmes personnalisés, copie sous le nom choisi (chemin relatif sûr du `.info`), état « non vérifiable ».
  Les arborescences (`Machines/` de bluemsx, données de scummvm) ne s'importent pas : « Ouvrir le dossier système ».

### 7. ROM intégrées : avertissement accepté une fois par core (amende ADR 0005, règle « Builtin »)

Un core **absent de `cores.toml`** n'a jamais été examiné. Avant son installation pour un système personnalisé (ou
son association, core déjà installé ou manuel) : « Ce core n'a pas été examiné par emu-rust ; il peut contenir des
fichiers système d'origine dont le statut n'est pas vérifié. Usage personnel uniquement. » — **Accepter**
(`content_accepted`, Q8) ou Annuler. Les cores de `cores.toml` n'affichent rien : ils servent déjà des systèmes
officiels sous la règle « Builtin » ; les deux cas « à vérifier » de l'ADR 0005 §3 (mgba, picodrive) restent
traités là (écart justifié à la remarque m12). L'ADR 0005 §3 est amendée (lot PD1, 2026-11-27) : « … n'est pas
proposé par défaut **sans acceptation explicite de l'utilisateur, demandée une fois par core** ».

### 8. Rendu matériel : refus « prévu en 9.0 », préparé pour la 9.0

- **Avant installation** : `CoreInfo::render` structure `hw_render` et `required_hw_api` (liste d'API, version
  minimale). `hw_render = true` ⇒ grisé « Rendu matériel : prévu en 9.0 », **même sans `required_hw_api`**
  (7 cores) ; `hw_render` absent ⇒ inconnu, ajoutable (le lancement fait foi). Le filtre est **une fonction**
  (`addability`), jamais une liste de noms. Textes venus du réseau (`systemname`, API inconnue) bornés (128, 64)
  et nettoyés avant affichage.
- **Au lancement et à l'essai à blanc** (fait foi) : `emu-libretro` **mémorise** la demande `SET_HW_RENDER` et la
  refuse toujours ; `Core::hw_render_request()` l'expose. `emu-runtime` rend `RuntimeError::HardwareRenderRequired`
  si le chargement échoue après une demande, si le core envoie la sentinelle `RETRO_HW_FRAME_BUFFER_VALID`, ou si,
  après une demande, 60 images de suite arrivent sans tampon logiciel. Message : « Ce core a besoin du rendu
  matériel : prévu en 9.0 ». Un core qui se rabat sur le logiciel continue.
- **Un seul point de refus** : cette variante d'erreur et une variante de `LaunchCheck`, que la 9.0 changera en
  « accepté si l'API est disponible ». `format > 1` ou `render` inconnu : définition refusée proprement.
- Limite acceptée : un core qui sur-déclare (swanstation) est refusé bien qu'une option le ferait tourner.

### 9. Liste des cores, ajout, paliers

- **Liste** (Q3) : écran Cores → « Ajouter un système non testé… », d'après `CoreManager::infos()` et l'index de la
  plate-forme courante (téléchargés par « Vérifier les mises à jour » ; aucune requête au démarrage). Par défaut :
  catégorie « Emulator », non expérimental ; case « Tout afficher », qui rend aussi ajoutables les non-émulateurs
  (jeux, moteurs) acceptant un format à fichier unique, ni disque ni arcade (réponse Q8).
- **Extensions de la 8.0** (liste explicite, distincte de `DISC_EXTENSIONS`, testée sur des extraits réels de
  `.info`) : disque = `cue`, `chd`, `iso`, `ccd`, `toc`, `gdi`, `cdi`, `mds`, `mdf`, `nrg`, `pbp`, `cso`, `img`,
  `bin` ; archive = `zip`, `7z` ; liste = `m3u`, `m3u8`. **`bin` est gardé** quand le core n'accepte aucune autre
  extension de disque (cartouche Atari 2600 de stella) ; retiré sinon.
- **Ajoutable en 8.0** (`CoreInfo::addability`) : au moins une extension restante. Sinon grisé : rendu matériel ⇒
  « prévu en 9.0 » ; disque seulement ⇒ « prévu en 8.4 » ; arcade (palier `arcade` de la table, ou seule extension
  `zip`) ⇒ « prévu en 8.6 » ; aucune extension ⇒ non proposé. Ordinateur (`needs_kbd_mouse_focus` ou palier
  `computer`) : ajoutable avec « clavier complet et souris : prévus en 8.5 » (réponse Q2).
- **Ajout en un clic** (Q4, Q5) : une case par nom `database` du core, la première cochée ; une machine de palier
  `disc` (« PC Engine CD ») d'un core mixte est **grisée « prévu en 8.4 »** (réponse Q6). « Ajouter » enchaîne :
  avertissement du §7 si besoin, installation (gestionnaire existant), sonde, écriture des définitions, publication
  du catalogue, création de `roms/<id>/`, scan incrémental, puis « Déposez vos jeux dans `roms/<id>/` » avec
  « Ouvrir le dossier ». Une `database` d'un système officiel n'est pas proposée (« déjà pris en charge »).
- **Archives** : un `.zip` sous `roms/<id>/` dont une entrée a une extension du système est un jeu de ce système
  (règle existante du scanner) ; le lancement respecte `block_extract` et `need_fullpath` du core (existant).
- **Cores manuels** (Q12) : utilisables, étiquette « manuel, jamais vérifié » ; préremplissage par le `.info` de
  même id s'il existe, sinon par la sonde seule.
- **DAT et jaquettes par nom** (Q14) : « Chercher la base d'identification » : au plus 3 requêtes (`no-intro`,
  `tosec`, `dat/` de libretro-database ; même hôte, même code) ; les sources trouvées vont dans `dat`. Jaquettes
  libretro-thumbnails par le nom libretro, sur action ; ScreenScraper / IGDB si l'utilisateur saisit les numéros.
  Sans nom libretro : **aucune requête ni aucun fichier** de DAT ou de jaquette (test), jeu sous son nom de
  fichier. `SystemInfo::dats` peut être vide pour un système personnalisé.
- **Marque « non testé »** (Q7) : étiquette permanente sur le système, ses jeux et le core ; avertissement au
  **premier lancement de chaque jeu** de la paire (système, core), au moment de l'essai à blanc, jusqu'à « Ça marche
  chez moi » (`works_for_me`) ; l’étiquette reste (réponse Q9). Journal et diagnostic portent la marque.
- **Suppression** (réponse Q1) : définition dans `.retired/` ; fichiers, sauvegardes, options, core et
  `roms/<id>/` intacts ; **les lignes de ses jeux restent dans la bibliothèque, masquées** (favoris, historique,
  identification, jaquettes compris), ainsi que ses DAT importés. Elles réapparaissent si le système est
  **restauré** (Réglages → Systèmes personnalisés → « Systèmes retirés » → « Restaurer » : la définition revient de
  `.retired/`) ou **recréé** avec le même id. Action séparée et confirmée, sur un système retiré : « Oublier ses
  jeux » (`Library::forget_system(id, ForgetDats::Remove)` : lignes des jeux et DAT de cet id ; fichiers jamais
  touchés).
- **Supprimer une définition en conflit incertain** (relectures courtes, M3 puis m4 ; même id qu'un officiel,
  autre `database` ou aucune) : ses lignes portent l'id de l'officiel, qui redeviendrait visible ; ses jeux
  apparaîtraient sous une autre machine, dont le core pourrait les lancer et y écrire des sauvegardes. La
  suppression ordinaire est donc refusée. Le dialogue propose « Oublier ses jeux et supprimer » ou « Annuler »
  (défaut) ; la première est **refusée tant qu'un fichier de jeu de ce système reste sous `roms/<id>/`** (liste
  montrée, « Ouvrir le dossier » ; l'utilisateur les déplace ailleurs). Contrôle sur le disque, pas sur les lignes :
  fichiers que la définition revendique (ses extensions, `.zip` à entrée reconnue), `custom_games_on_disk`. Elle oublie alors les **lignes des jeux
  seulement** (`forget_system(id, ForgetDats::Keep, ForgetScope::ClaimedBy(catalogue))` : les DAT de cet id sont
  aussi ceux de l'officiel), puis retire la définition. **Portée** (amendement du 2026-11-27 soir, lot PB3) : seules
  les lignes que la définition revendique (extension du chemin, entrées d'un `.zip`) **et** que l'officiel masqué
  n'accepte pas sont oubliées ; celles que l'officiel accepte (ses jeux d'avant, extension partagée) sont gardées
  avec leurs données et réapparaissent sous lui (même logique que les lignes gelées du §13). Question R2 (`docs/questions-ouvertes.md`).
- **« Restaurer »** ne propose que les définitions retirées par l'utilisateur (`Retirement::Removed`) ; une
  définition retirée par une bascule (`Replaced`, nom `<id>-<unix>.replaced.toml`) est listée « remplacée par
  (officiel) », sans bouton : la restaurer masquerait de nouveau l'officiel et cacherait les jeux basculés (m4) ;
  seule l'annulation du rangement (§10) la rend. La primitive `restore_custom` accepte donc les deux natures (elle
  sert à cette annulation) ; le choix du bouton est celui de l'interface, d'après `RetiredSystem::why`.

### 10. Bascule vers un système officiel (Q16)

- **Conflit** : une définition **valide** dont l'id **ou** la `database` est celle d'un système officiel **reste
  active**, l'officiel est **masqué** (absent du catalogue publié) et `CatalogLoad::pending_switches` le signale.
  **Double conflit** (id d'un officiel A **et** `database` d'un autre officiel B) : la définition est refusée au
  chargement (`DuplicateDatabase`, « en erreur »), B n'est jamais masqué ; A reste actif et les lignes de l'id en
  conflit suivent le §13.
  Tant que la définition est active, **sa liste de cores prime** sur celle de `cores.toml` pour sa clé
  (`with_custom_systems` remplace la clé quand l'officiel est masqué) : rien ne change en silence avant la bascule.
- **Nature du conflit** (`SwitchKind`) : **en place** = même id et même `database` (cas normal, §3) ;
  **avec déplacement** = même `database`, id différent (suffixe, ou réduction devenue autre) ; **incertaine** = même
  id, `database` différente ou absente (machine peut-être différente ; rendu rare par les ids réservés du §3).
- **Proposition** : bandeau de la bibliothèque et ligne des Réglages : « emu-rust prend désormais en charge
  (système) officiellement. Basculer (recommandé) » ; jamais de bascule automatique. Cas incertain : pas de
  recommandation, mais « Une machine officielle porte le même nom de dossier : est-ce la même machine ? » — « Oui,
  basculer » ou « Non » (la définition reste active, l'officiel reste masqué, le bandeau peut être fermé ;
  la suppression de la définition suit alors la règle « autre machine, même dossier » du §9).
- **Choix avant toute action** : les questions d'une bascule (touches, ci-dessous) sont posées **avant** la
  première modification, dans le bandeau ou l'aperçu ; la réponse est notée (en-tête du journal pour la bascule avec
  déplacement). Une reprise au démarrage n'interroge jamais l'utilisateur.
- **Étapes communes** de toute bascule, dans cet ordre : (a) **touches** : si la clé de profil officielle
  (`PadStyle::profile_key`) n'existe pas, le profil `<core>@<id>` y est copié ; si elle existe et diffère (core
  partagé avec un système officiel), emu-rust demande « Garder vos touches » (défaut, copie par-dessus) ou « Garder
  celles de (système officiel) » ; (b) **core** : si le core de l'utilisateur n'est pas le premier core officiel,
  `[cores.preferred]` le garde en tête (ses sauvegardes, rangées par core, restent visibles), le core recommandé
  étant proposé en alternative (réponse Q4) ; (c) **lignes et définition** : `game.system` réécrit (bascule avec
  déplacement seulement), puis définition dans `.retired/` (`Retirement::Replaced`) ; (d) publication, scan.
  Toutes sont idempotentes.
- **Bascule en place ou incertaine acceptée** : étapes communes seules, sans journal. Même id ⇒ même dossier, mêmes
  lignes (favoris, historique, jaquettes) : **aucun fichier déplacé**. Elle est **définitive** : une définition
  retirée par bascule ne se restaure pas (§9). **Bascule avec déplacement sans aucune unité** (aucun jeu à déplacer
  sous `roms/<id perso>/` ; contrôle du lot PB3) : traitée de même, sans journal ni annulation, lignes réécrites
  (`AcceptedBy(officiel)`) **avant** le retrait de la définition (`Replaced`) ; une coupure entre les deux laisse la
  définition active et la bascule proposée, relance idempotente (design 8.0 §0.10, point 5).
- **Bascule avec déplacement** (réponse Q5) : elle passe par **le rangement** de la 7.1 (ADR 0023), qu'elle
  précise. **Principe** (relecture courte, M1) : le système des lignes ne change **qu'en une fois**, avec la
  définition, quand **toutes** les unités sont à destination (ou toutes revenues) ; à aucun moment un jeu n'est
  rattaché à un système absent du catalogue.
  1. **Aperçu** calculé comme si la bascule était faite (catalogue où la définition est retirée et l'officiel
     actif) : toute unité sous `roms/<id perso>/` a pour destination `roms/<id officiel>/` (même chemin relatif) et
     pour système l'id officiel ; les fichiers qui ne sont pas des jeux restent en place (signalés). Unité bloquée,
     doublon ou conflit (ADR 0023 §4.2, §4.4) : la bascule n'est **pas** proposée tant qu'il en reste (le rapport dit
     lesquelles résoudre). Texte de l'aperçu : « Annulable tant que vous ne rangez rien d'autre » (point 5).
  2. **Accord** de l'utilisateur sur l'aperçu (choix des touches compris), puis exécution avec les primitives et le
     verrou du rangement (liens physiques, jamais d'écrasement, une transaction par unité). La transaction d'une
     unité ne réécrit **que les chemins** (`Library::relocate`, inchangé) : la ligne garde l'id personnalisé, encore
     actif ; favoris, historique, identification et jaquettes suivent.
  3. **Fin de bascule** : quand l'aller est clos (`E`) **et que toutes les unités du plan sont validées** —
     l'en-tête du journal porte leur nombre `n` (unités numérotées 0 à n − 1) : il faut un bloc validé `C u` pour
     chacune (une unité sans `B`, changée depuis l'aperçu ou laissée par une annulation entre deux unités ou par la
     borne du journal, rend la bascule incomplète) — les étapes communes sont faites :
     `Library::reassign_system` (**toutes** les lignes de l'id perso vers l'id officiel, une transaction, y compris
     les lignes hors de `roms/<id perso>/`, par exemple sur une racine externe débranchée), touches et core
     d'après l'en-tête, retrait de la définition sous le nom noté dans l'en-tête — puis la ligne `F` est posée ;
     publication, scan. **Bascule incomplète** : pas de `F` ; la définition reste active et l'officiel masqué ; les
     jeux déjà déplacés restent visibles sous le système personnalisé, depuis `roms/<id officiel>/` ; le rapport
     dit « bascule incomplète : N jeux restent dans `roms/<id perso>/` ; résolvez-les puis relancez la bascule »,
     et `plan_switch` reprend les unités restantes (nouveau journal). Les DAT importés de l'id perso sont
     **gardés** (lignes d'un système absent, sans effet) : rien à défaire.
  4. **Journal** : la bascule écrit le journal en **version 2** ; **le rangement ordinaire reste en version 1**
     (relecture courte, m5). L'en-tête porte, avant le premier `B`, les lignes de bascule : id perso, id officiel,
     **nom du fichier retiré** (`<id perso>-<unix>.replaced.toml`, `unix` du journal ; aucun chemin libre : le
     fichier actif est `<id perso>.toml` sous `systems/`, le retiré sous `systems/.retired/` ; nom contrôlé par le
     lecteur strict, sans `/` ni `..`), **nombre d'unités du plan**, **choix des touches**, **préférence de core**
     et **profil de touches officiel antérieurs** (bornés). Lecteurs : versions 1 et 2 ; amendement d'ADR 0023
     §4.8 (lot PD1). Esquisse de grammaire : design 8.0 §2.6 ; PD1 l'écrit en entier et `reviewer` la relit
     (**PV3**) **avant** que PB3 n'écrive l'exécution (ADR 0023, Conséquences). La reprise au démarrage rejoue la
     fin de bascule si `E` est posée sans `F` et que les n unités sont validées (idempotent, sans question : le
     choix est dans l'en-tête) ; elle passe **avant** les identités (`F` précède `I`).
     **Échec de la fin de bascule** (`complete_switch` : définition à restaurer dont la place est prise, fichier
     retiré absent, base ou disque en erreur) : le journal reste « fin due » ; il **bloque** tout nouveau rangement
     et toute annulation (un rangement plus récent rendrait la fin introuvable) et la fin est retentée à chaque
     démarrage ; l'interface donne la raison et l'action. Cas « place prise » (`systems/<id>.toml` déjà là) : les
     Réglages, qui refusent sinon toute action sur la définition pendant un journal de bascule ouvert, proposent
     « Retirer ce système et terminer la bascule » (retrait `Removed`, jamais de suppression, puis fin relancée ;
     ADR 0023, amendement du 2026-11-27, test PU4). Cas « fichier retiré introuvable » : « remettez
     `systems/.retired/<nom>` puis relancez ».
  5. **Annulation** : « Annuler le dernier rangement » n'est **proposée que si toutes les unités peuvent revenir**
     (contrôle d'ADR 0023 §4.6 fait d'avance : fichiers inchangés, places d'origine libres) **et, si l'aller a son
     `F`, que `systems/<id perso>.toml` est libre** (définition recréée entre-temps ⇒ « supprimez d'abord le
     système (id) ») **et que `systems/.retired/<nom-retiré>` est présent** (« fichier retiré introuvable ») ; sinon
     elle est refusée avant toute action, avec la raison. Chaque unité revient (chemins seulement). **Fin d'annulation**
     seulement si l'aller a son `F` : quand `U` est posée et que toutes les unités de l'aller sont revenues,
     `reassign_system` de **toutes** les lignes de l'id officiel vers l'id perso (même portée qu'à l'aller : aucune
     ligne basculée n'est oubliée ; une ligne ajoutée par un scan depuis la bascule suit aussi et reste visible sous le
     système restauré), définition restaurée depuis le nom noté, préférence de core et profil de touches antérieurs
     rendus, puis `F`. Aller sans `F` (bascule incomplète) : l'annulation ramène les chemins, rien d'autre (les lignes
     n'ont jamais changé de système). **Annulation partielle malgré tout** (annulation interrompue, devenue définitive,
     ADR 0023 §4.8 RV1 5 ; fichier changé entre le contrôle et l'exécution) : pas de `F` ; la définition reste retirée,
     l'officiel actif ; les jeux revenus restent rattachés à l'officiel (même `database` : même machine, jouables)
     depuis `roms/<id perso>/`, dossier réservé au rangement (§12) et définition remplacée non restaurable (§9) : **le
     rapport et le guide disent de les déplacer à la main vers `roms/<id officiel>/`** (puis scan). **Limites, écrites
     dans l'aperçu et le guide** : seul le **dernier** journal s'annule (ADR 0023 §4.6) — tout rangement après la
     bascule la rend définitive ; les jaquettes renommées (point 6) ne sont pas défaites (elles se retéléchargent sur
     action).
  6. Jaquettes en cache (`covers/<système>/`) : le dossier de l'ancien id est renommé vers l'id officiel s'il
     n'existe pas, sinon les jaquettes se retéléchargent sur action.
- **Sauvegardes d'un même core** : les propriétés par core de `cores.toml` qui fixent le chemin des sauvegardes
  (`media`, `save_scope`, `save_family`, `game_saves`) s'appliquent déjà au système personnalisé ; une bascule avec
  le même core ne les change donc pas. Le risque est qu'une **version future** les ajoute à un core déjà utilisé :
  un test d'`emu-assets` fige ces clés par core (instantané) ; le modifier impose à la version concernée une
  migration des sauvegardes (rangement ADR 0023), règle rappelée dans le design.
- **Lignes que l'officiel n'accepte pas** (relecture PB2, tour 2, M-a ; choix sûr de la session principale, lot PB3) :
  après une bascule acceptée (définition → `.retired/`, `Replaced`), les lignes dont l'extension (ou l'entrée de
  `.zip` enregistrée) n'est connue que de la définition seraient purgées au scan suivant. Elles sont **listées dans
  l'aperçu** de la bascule (« gardés, masqués, jusqu'à Oublier ses jeux »), puis restent **gelées** (masquées, jamais
  purgées automatiquement) jusqu'à « Oublier ses jeux » explicite (ligne « remplacé par… » des systèmes retirés) ou
  restauration de la définition (annulation du rangement). Avec déplacement, la fin de bascule ne réécrit que les
  lignes que l'officiel accepte : les autres gardent l'id perso, retiré ; en place ou incertaine, l'id officiel actif
  dont `.retired/` garde une définition `Replaced` de même id est un id en conflit (§13). Signatures et tests :
  design 8.0 §2.1, §2.6, §4.
- La 8.0 livre et teste les trois cas avec un catalogue officiel et un `cores.toml` synthétiques ; aucune version
  8.0 ne déclenche de bascule réelle.

### 11. Touches : disposition générique, noms de boutons du core (Q13)

- Système personnalisé ⇒ `PadStyle::Generic` **quel que soit le core** ; clé de profil `<core>@<id>`.
- `emu-libretro` copie `SET_INPUT_DESCRIPTORS` (≤ 256 entrées, texte ≤ 64 caractères nettoyé) ;
  `Core::input_descriptors()` l'expose. Affichés dans le remappage et l'aide de touches ; gardés dans
  `<data>/cache/input-descriptors/<core>.toml` après l'essai à blanc ou une partie. Sans descripteur : A, B, X, Y…

### 12. Rangement (7.1) et glisser-déposer (7.2.1) (Q6, Q17)

- **Scan** : les extensions d'un système personnalisé ne valent que sous `roms/<id>/` ; hors de ce dossier,
  l'attribution **et le rapport de scan** (liste des non classés) sont strictement inchangés pour les fichiers des
  systèmes officiels (tests de non-régression). Seul effet visible accepté : un fichier hors dossier d'une
  extension **propre** à un système personnalisé apparaît désormais « non classé » au rapport.
- « Créer les dossiers des systèmes » crée aussi ceux des systèmes personnalisés ; le refus « dossier à créer dont le
  nom est l'id d'un système » suit le catalogue courant. Un `roms/<id>/` existant est signalé à l'ajout.
- **Rangement** : un système personnalisé n'est candidat que si l'extension n'appartient à **aucun autre système** ;
  l'unité est alors **Suggérée** (décochée, raison « système non testé ») ; sinon candidats d'aujourd'hui, sans
  système personnalisé. Une unité déjà connue garde son système.
- **Dossiers réservés** (relectures courtes, M2 puis M1) : réservés = (ids des définitions **retirées**
  `.retired/`, **en erreur**, des officiels **masqués**, et ids de `game.system`) **moins les ids actifs** de
  l'instantané. Un tel `roms/<id>/` n'est ni une source ni une destination du rangement : rien n'y est pris ni posé,
  il n'est jamais retiré même vide, et l'aperçu le signale (« dossier d'un système retiré ou en erreur : laissé tel
  quel »). Un id actif n'est **jamais** réservé : une définition `megadrive` invalide ou retirée (`.replaced`
  compris), une définition retirée puis recréée, un conflit sur le même id ne bloquent pas le dossier du système
  actif. **Exception** : `plan_switch` prend sa source `roms/<id perso>/`, réservée dans son catalogue « après ».
  Liste : `Catalog::reserved_folders()` (déjà diminuée des ids actifs) plus les ids de la base absents du
  catalogue. Sans cela, ses fichiers seraient classés un par un (un `.bin` vers la Mega Drive) et « Restaurer » ne
  retrouverait plus les jeux (contraire à la réponse Q1). « Créer les dossiers des systèmes » ne les recrée pas.
  Un dossier `roms/<nom>/` correspond à un id (minuscules) quand `nom` lui est égal **sans tenir compte de la casse
  ASCII** (systèmes de fichiers de Windows et macOS), pour la réservation comme pour l'exemption. **Limite
  acceptée** (relecture PB2, tour 2) : le contrôle d'une unité reconnaît un dossier réservé à **toute profondeur**
  sous `roms/<id>/` (un sous-dossier de jeu `Ghost/` est bloqué si `ghost` est réservé), comme un id actif depuis la
  7.x ; l'unité est seulement bloquée et l'aperçu dit pourquoi.
- **Glisser-déposer (7.2.1)** : sa conception appellera le même classement, instantané pris au dépôt.

### 13. Schéma, données, ordre de démarrage

- **Aucune migration pour les systèmes personnalisés** : `game.system` et `dat_info(system, source)` sont du texte.
  La marque « non testé » est dérivée du catalogue. `SCHEMA_VERSION` passe de 7 (7.4 : `game.needs_32x`) à 8 pour la
  seule marque de scan (`game.scan_pending`, amendement du 2026-11-26).
- **Le scan ne supprime ni ne modifie jamais les lignes dont le système est absent de l'instantané** (définition
  en erreur, dossier illisible, système masqué ou retiré) ; elles ne sont pas affichées. Seul « Oublier ses jeux »
  (`forget_system`) les retire.
- **Id en conflit** (amendement du 2026-11-27 soir) : officiel masqué par une définition de même id, définition en
  erreur portant l'id d'un officiel actif, ou (PB3) officiel actif dont `.retired/` garde une définition `Replaced`
  de même id (§10). Une ligne de cet id que le système actif n'attribuerait pas (extension du chemin ; pour un
  `.zip`, entrée enregistrée en base, l'archive n'étant pas ouverte) est traitée comme une ligne de système absent :
  ni modifiée, ni purgée. Un `.zip` disparu dont l'entrée enregistrée est acceptée suit donc la règle ordinaire
  (purgé) ; un `.zip` sans entrée connue est gelé. Les autres lignes de l'id suivent la règle ordinaire.
  **Décision de la session principale (relecture PB2, tour 2)** : ces lignes gelées **restent en base mais sont
  masquées** dans la bibliothèque et ne sont **jamais lancées avec le core de la définition** (même prédicat que le
  scan) ; réalisé en PU3 (aujourd'hui `games_in` ne filtre que les systèmes absents du catalogue).
- **Ordre de démarrage**, interface et modes ligne de commande (`library`, `cores`) : chargement et publication du
  catalogue, **puis** reprise du journal de rangement, **puis** fin de bascule en attente (`pending_switch` :
  touches et core d'après l'en-tête, `complete_switch`, nouvelle publication), **puis** identités du rangement, construction des préférences de
  cores, scan.
- Fichiers nouveaux sous `<data>` : `systems/` (+ `.retired/`), `cache/dry-run/`, `cache/dry-runs.toml`,
  `cache/launch-<pid>.marker`, `cache/input-descriptors/`. Écritures atomiques.

### 14. Effet sur chaque système livré

Toute capacité nouvelle est inactive tant que l'utilisateur n'a ajouté aucun système personnalisé, sauf le témoin
de lancement, étendu à tous les cores par la réponse Q11.

| Système (`systems.toml`) | Effet |
|---|---|
| megadrive | change : témoin de lancement (Q11) ; à vérifier : attribution et rapport du scan, rangement (non-régression PB1, PB2 ; `.bin` sous `roms/<id perso>/` reste au système perso, jamais proposé ailleurs) |
| snes, nes, gb, gbc, gamegear, mastersystem, gba, 32x | change : témoin (Q11) ; à vérifier : scan et rangement (PB1, PB2) |
| atarist, amiga, dos, cpc, c64 | change : témoin (Q11) ; à vérifier : scan et rangement (PB1, PB2) |
| megacd, psx, 3do | change : témoin (Q11) ; à vérifier : scan et rangement (PB1, PB2), extensions de disque inchangées (`DISC_EXTENSIONS` non touchée) |

Aucun système livré ne change de core, de clé de sauvegarde (instantané figé, §10), de dossier ou d'id ; la bascule
ne concerne qu'un officiel ajouté par une version future. Le dossier d'un système livré n'est **jamais réservé** au
rangement (§12 : ids actifs exclus), même si une définition du même id est invalide, retirée ou remplacée (test
PB2).

## Conséquences

- Livraison en trois temps : catalogue dynamique sans changement de comportement, définitions et garde-fous, puis
  interface. 8.4, 8.5, 8.6 et 9.0 étendent `format`, `render`, la table des paliers et `addability`.
- Fuite volontaire et bornée des instantanés (20 à 50 Kio par modification réelle).
- Un core non testé peut fermer l'application **en cours de partie** ; le témoin le dit, la SRAM limite la perte.
- Revue `reviewer` du rangement étendu (ADR 0023, Conséquences) : grammaire du journal version 2 **avant**
  l'exécution (lot PV3, sur le texte de PD1), puis `emu-library/src/tidy/` de PB3 comme code sensible (lot PV4).
- Journal version 2 : écrit par la seule bascule avec déplacement. Un binaire 7.x refuse en entier un journal
  version 2, ce qui bloque chez lui tout rangement et toute annulation tant que ce journal est présent dans
  `<data>/rangement/` (il ne le purge jamais : amendement du 2026-11-26) ; écrit dans l'amendement d'ADR 0023
  du 2026-11-27 (PD1) et dans le guide d'essai 8.0. Sans conséquence pratique en 8.0 (aucune bascule réelle) ;
  à partir de la version qui ajoute un officiel, revenir à une version plus ancienne perd de toute façon ce
  système.
- Revue `reviewer` : `unsafe` des copies FFI et de `SetErrorMode` ; enfant d'essai (dès sa livraison) ; analyse des
  définitions et des `.info` ; avertissement « contenu non vérifié ».
- Assistance : un défaut sur un système personnalisé n'est pas un défaut d'emu-rust ; la marque figure partout.
- Q15 (export / import) : le format le permet déjà (ni binaire, ni URL, ni chemin absolu) ; à rappeler en 8.4.
- Non gérés en 8.0 : rotation (`SET_ROTATION`), deux écrans et stylet (DS), sous-systèmes, pistolets.
