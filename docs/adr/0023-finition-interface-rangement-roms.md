# ADR 0023 — Finition de l'interface, console cachée, rangement de `roms/`, fichiers d'origine (PC 7.1 → 7.5 et 7.6)

Statut : **accepté, amendé pour la fusion le 2026-11-25** (voir « Amendement de fusion » ci-dessous, qui **prime** sur
le reste du texte). Historique : accepté le 2026-11-21 (architecte ; proposé le même jour, amendé après la revue indépendante — reprise
par unité, marqueur d'identités, groupes multi-CD, console à deux exécutables, copie entre volumes retirée ;
**amendé le 2026-11-22 après la revue FV0** du journal : §4.3 `.bin` sans en-tête, §4.8 ; **précisé le même jour
après la revue RV1** : §4.8, « Précisions RV1 » ; **amendé le 2026-11-23 après la revue RV3** : §4.8,
« Décisions après la revue RV3 » — lecture seule, écriture refusée, ReFS, borne de 512 Mio retirée). Choix
de l'utilisateur faits : icône **B** (croix et boutons), avertissement BIOS **une fois par (core, fichier)** ; les
autres défauts sont appliqués et consignés (`docs/questions-ouvertes.md`, lignes 7.1). Périmètre :
`docs/specs/v7.1-v8.0-utilisateur.md` §7.1 (fait autorité ; point 5 = BIOS / firmware d'origine fourni par
l'utilisateur, jamais téléchargé ; rangement avec aperçu et annulation). Précise
ADR 0005 (rien de commercial n'est téléchargé), 0006 (arborescence `roms/<système>/`), 0011 (identité de jeu,
écritures atomiques, `config.toml`), 0012 (paquets), 0014 (bibliothèque), 0013 / 0021 (firmware, repli du core).
Réutilise sans les changer les règles de classement des ADR 0015, 0017 à 0021 (ordinateurs, CD, extensions
partagées). Conception et découpage : `docs/design/v7.1-finition-interface.md`.
**Amendé le 2026-11-27 pour la 8.0** (lot PD1 : journal version 2 de la bascule, marque de scan en base ; **précisé
le même jour après la relecture PV3**, 6 majeurs et 7 mineurs ; voir « Amendement du 2026-11-27 », qui prime sur §4.5
et §4.8).
**Graphe des crates inchangé. Schéma de `library.sqlite` inchangé (v7 depuis la 7.4 ; ni la 7.5 ni la 7.6 n'en
ajoutent ; la v8 est celle de la 8.0, ADR 0025). Aucun code réseau nouveau.**

## Amendement de fusion (2026-11-25) — livraison en 7.5 et 7.6 sur la base Mac 7.4.0

Contexte : cette ADR a été écrite et réalisée sur la ligne PC (`wip-7.1`, jamais fusionnée) pendant que la ligne Mac
livrait dans `main` une autre 7.1 (`docs/design/v7.1-finition.md`, sans ADR), puis 7.2 (manette), 7.3 (refonte) et
7.4 (CD 32X). La fusion réapplique le travail PC sur `main` (`.claude/fusion-plan.md`, décisions D1 à D15 de
l'utilisateur, toutes au défaut). Contrat et découpage : `docs/design/v7.5-finition-pc.md`. Ce paragraphe prime sur
le texte qui suit.

| Partie | Livrée en | Ce qui change par rapport au texte ci-dessous |
|---|---|---|
| §1 Pied de page | 7.5 | module PC (`ui/footer.rs`) sur **toutes** les pages et le menu en jeu, texte « emu-rust 7.5.0 — dev by claude@jerhome » (remplace « v7.4.0 · dev by… » du Mac), couleurs `theme::` ; le bouton « Journal » ouvre `Page::Journal` (D5) |
| §2 Icône | 7.5 | **dessin Mac** (manette, accent `#4aa3ff`, `assets/icon.svg`) à la place de la variante B (D2) ; la **chaîne PC** le produit : `scripts/gen-icons.sh`, empreinte `SOURCE.sha256`, PNG, `.ico`, `.icns`, hicolor, `.desktop` ; `winresource` sous `cfg(windows)` (version PC) avec l'icône Mac. Emplacement des fichiers générés : choisi par `platform-build` (L1), qui garde le chemin lu par `window_icon` d'`emu-ui` ou le signale |
| §3 Console | 7.5 | Windows : **`emu-rust-cli.exe`** (feature `cli-bin`, `emu-app` bibliothèque + 2 binaires) remplace `emu-rust-console.exe` (feature `console`) du Mac (D3). Réglage : **`[app] show_terminal`** (Mac, libellé « Afficher le terminal (après redémarrage) »), `[interface] console` n'est plus qu'un **alias lu** au chargement (design 7.5 §4). macOS : `terminal_macos.rs` du Mac porté (relance dans Terminal.app), le réglage y est donc proposé ; Linux : ligne d'explication |
| §3 Journal | 7.5 | moteur PC (`emu_ui::log` : nettoyage, bornes, réserve panique, verrou multi-instance, lignes des cores) + règle Mac `stderr_wanted` pour la couche `stderr` ; le « panneau » est une **page** `Page::Journal` ouverte depuis le pied de page, avec un lien dans Réglages ; le volet Journal des Réglages Mac est retiré (D6) |
| §3 Erreur fatale | 7.5 | inchangé : `emu_runtime::fatal_dialog` (`MessageBoxW`), seul `unsafe` nouveau |
| §4 Rangement | **7.6** | inchangé sur le fond ; schéma v7 (§4.7) ; écrans en `theme::` ; `Page::Tidy` ajoutée en 7.6 seulement ; la marque manuelle CD 32X (`[library] cd32x`, ADR 0024) reste portée par le nom du fichier, que le rangement ne change jamais |
| §5 Fichiers d'origine | 7.5 | **bandeau du jeu sélectionné (Mac)** avec « Importer… », alimenté par les **données et la vérification PC** (`optional_firmware` de `cores.toml`, tables `required_firmware` à `fallback`, md5, « non vérifié », MT-32) ; **abandon** de l'annonce « une fois par (core, fichier) » et de la clé `[interface] firmware_hints_seen` (D7, qui remplace le choix du 2026-11-21) ; `data/firmware_hints.toml` du Mac retiré ; l'écran Cores garde sa ligne permanente ; l'avertissement « une fois par jeu » de la 7.0 (cores à `fallback`) est inchangé |
| Presse-papiers | 7.5 | feature `clipboard` d'`egui-winit` (53a1490) : sans elle, « Copier » de `main` ne copie rien (conflit 8) |

- **Contexte (interface)** : l'accent `#285AA0` et les couleurs citées sont remplacés par le thème de la 7.3
  (`theme.rs`, accent `#4aa3ff`) ; tout écran porté du PC utilise les jetons `theme::`, jamais `Color32` brut.
- **Conséquences** : la « tâche de contrat (FX0) » est remplacée par le lot **L1** (`platform-build`) pour les lignes
  de `Cargo.toml` / `Cargo.lock` ; `windows-sys` 0.61 (version déjà au verrou) devient dépendance directe
  d'`emu-runtime` sous `cfg(windows)`.
- **Effet sur chaque système livré** : design 7.5 §10 (une ligne par système de `systems.toml`) ; le rangement
  (7.6) aura son propre tableau dans la conception 7.6.

## Amendement du 2026-11-27 (8.0, lot PD1) — journal version 2 de la bascule, marque de scan

Écrit pour la 8.0 (ADR 0025 §10.4, amendement du 2026-11-26 ; esquisse : `docs/design/v8.0-systemes-personnalises.md`
§2.6). Ce paragraphe **prime** sur §4.5 et §4.8 pour ce qu'il touche ; tout le reste du format version 1
(`docs/design/v7.1-finition-interface.md` §3.4.1, livré en 7.6) vaut pour la version 2, à l'identique. Grammaire
relue par `reviewer` (lot PV3) **avant** que l'exécution soit écrite (lot PB3), comme le demandent les Conséquences.

**§4.5 (filet du scan) — marque de scan en base, schéma v8 (lot PB0).** Le fichier `library.sqlite.scanning` de la 7.6
n'est plus écrit : toute ligne `game` écrite par un scan porte `scan_pending = 1` dans la transaction de son lot ; la
transaction finale d'un scan **terminé** (rapprochement, purge) remet **toutes** les marques à `NULL`. Un scan annulé
proprement ou un scan prudent lève, dans sa dernière transaction, les marques de ses propres lignes mais **garde**
celle des lignes marquées au départ qui portent des données de l'utilisateur (PV3, majeur 6) : le scan suivant doit
encore les voir pour faire la fusion. Les lignes
encore marquées au départ d'un scan (scan tué entre deux lots) sont candidates au rapprochement des fichiers déplacés
sans l'heuristique « ligne vierge » de la 7.6, puis la purge se fait normalement (plus de scan « prudent »). Une ligne
marquée qui porte des données de l'utilisateur (favori, historique, jaquette) n'est jamais absorbée ; **décision de
l'utilisateur du 2026-11-27** : si elle est la **jumelle unique** d'une ligne dont le fichier a disparu (mêmes critères
que le rapprochement, un seul candidat de chaque côté), les données de la ligne disparue lui sont **fusionnées** avant
la purge — favori = le plus fort des deux, nombres de parties additionnés, dernière date de partie la plus récente,
jaquettes : une jaquette `cached` l'emporte sur une réponse `missing` (une `cached` n'est jamais remplacée) — ; la ligne
disparue ainsi fusionnée est comptée dans `ScanReport::moved`, pas dans `removed`. **Rien** n'est fusionné si le couple
est ambigu, si le scan est prudent ou s'il est annulé (marques gardées, ci-dessus). Fichier `.scanning` laissé par une
7.6 : le premier scan 8.0 est prudent (ni purge ni fusion de données) ; le fichier n'est retiré **qu'après la
transaction finale d'un scan terminé** — jamais après un scan annulé ou en erreur, qui le laissent au scan suivant ; un
retrait impossible donne `marker_failed`. Une base v8 est refusée par une 7.x (`NewerSchema`, base non modifiée).

**§4.8 — journal version 2, réservé à la bascule avec déplacement.** Le rangement ordinaire (et son annulation)
écrit toujours la version 1. Lecteurs de la 8.0 : versions 1 (à l'identique) et 2 ; tout autre en-tête : illisible.

```text
journal    = "emu-rust tidy 2" LF  "R " racine LF  bascule  [ aller [ annulation ] ]
bascule    = "X " id-perso " " id-officiel " " nom-retiré " " n LF
             "K " ( "copy" | "user" | "official" ) LF
             "P " ( "-" | core { "," core } ) LF
             "Q " ( "-" | profil-encodé ) LF
aller      = { bloc }  [ "E" LF  [ "F" LF ]  [ "I" LF ] ]
annulation = { bloc }  [ "U" LF  [ "F" LF ]  [ "I" LF ] ]   ; seulement après le "E" (suivi ou non de "F", "I")
bloc       = (version 1, inchangé)
```

- **Champs de l'en-tête.** `id-perso`, `id-officiel` : `[a-z0-9]{1,24}`, distincts. `nom-retiré` =
  `retired_file_name(id-perso, Replaced, unix)` où `unix` est celui du nom du journal ; le lecteur le recalcule
  et refuse tout autre nom (aucun chemin libre). `n` : nombre d'unités du plan, `u32` ≥ 1, écriture décimale
  canonique (règle de la version 1). `K` : choix des touches (`KeysChoice` : `Copy`, `KeepUser`,
  `KeepOfficial`). `P` : préférence de core antérieure de l'id officiel, de 1 à 8 ids de core distincts à la
  syntaxe de `CoreId`, ou `-` (aucune) ; un core nommé `-` est refusé (à l'écriture comme à la lecture) ;
  `prior_preference: Some(vec![])` s'écrit `-` et se relit `None` (même effet). `Q` : profil de touches officiel
  antérieur (fragment TOML opaque pour `emu-library`, ≤ 16 Kio **décodé**, UTF-8), ou `-` (aucun) ; un profil vide
  (`Some("")`) s'écrit `-` et se relit `None`. Encodage canonique, **seul** accepté par le lecteur : octets `0x21`
  à `0x7E` sauf `%` écrits tels quels, tous les autres (espace, fin de ligne, `%`, non ASCII) en `%XX`
  (hexadécimal majuscule) ; un `%XX` en minuscules, incomplet ou qui code un octet à écrire tel quel rend le journal
  illisible. Seule exception : un profil égal à `-` (un seul octet) s'écrit `%2D` ; partout ailleurs `-` s'écrit
  tel quel et `%2D` est non canonique.
- **Effet de `P` et `Q` à la fin d'annulation** : `P -` retire l'entrée de l'id officiel de `[cores.preferred]`,
  `Q -` retire sa clé de profil ; sinon la valeur notée est remise. L'interface analyse le `Q` décodé comme une
  **table TOML** : s'il n'en est pas une, le profil n'est pas rendu (signalé au journal de l'application et dans le
  rapport) et la fin continue. `emu-library` ne l'analyse jamais.
- **Nom du fichier** : un journal version 2 n'est **jamais** suffixé `-<k>` : `apply_switch` prend
  `unix = max(maintenant, dernier + 1)` et l'augmente tant que `journal-<unix>.txt` ou `.retired/<nom-retiré>`
  existent, si bien que deux bascules ne partagent jamais un nom retiré ; un journal version 2 dont le nom porte
  `-<k>` est illisible.
- **Création.** Comme en version 1 : le fichier est créé juste avant le premier `B` de l'aller ; les six lignes
  d'en-tête sont écrites ensemble avec ce `B`, chacune synchronisée. Un **en-tête coupé** (fichier vide, ou moins
  des six lignes complètes et contenu préfixe d'un en-tête valide) vaut « rien n'a bougé » (état 0 de la version 1) :
  `recover_tidy` le supprime. **En-tête complet sans `B`** (coupure après la sixième ligne, avant la ligne `B`) :
  aller en cours sans aucun bloc ; `recover_tidy` pose `E` ⇒ bascule **incomplète vide** (aucune unité validée,
  rien n'a bougé, jamais de `F`, liste nette vide). L'invariant de la version 1 tient : tout préfixe d'un journal
  valide coupé à une fin de ligne est valide.
- **Bornes** (inchangées) : 16 Mio à la lecture ; l'aller est clos avant 7 Mio, **en-tête compris** (profil de
  16 Kio encodé compris) ; 20 journaux gardés ; seuls ceux dont la dernière ligne est `I` sont purgés.
- **`F` = fin de bascule faite** (étapes communes d'ADR 0025 §10.3 / §10.5, posée par `complete_switch`), toujours
  **juste après** le `E` ou le `U` de sa section : ordre `E F I` (ou `U F I`). `F` d'aller : seulement si chacune
  des `n` unités `0..n` a un bloc validé `C u` à l'aller. `F` d'annulation : seulement si l'aller a son `F` et
  si chacune des `n` unités a un bloc validé dans la section d'annulation.
- **Clos** = dernière ligne `E`, `U`, `F` ou `I` (règle du design 8.0 §2.6) ; sinon **en cours** (états 1 et 4
  de la version 1, distingués par la présence d'un `E`). `TidyJournalInfo::complete` = clos ; `identities_done` =
  dernière ligne `I`.

**États** (en plus des états 0 à 6 de la version 1, lus de la même façon) :

| Forme | Nom | Rangement, annulation | `pending_switch` | Identités dues |
|---|---|---|---|---|
| … `E`, les `n` unités validées | **fin due (aller)** | refusés : `SwitchPending`, quelle que soit la racine | `Forward` | non (rien tant que `F` manque) |
| … `E`, une unité au moins sans `C` | bascule **incomplète** | permis ; annulation : chemins seulement | — | oui (liste nette) |
| … `E` `F` | bascule faite | permis ; annulation sous `check_switch_undo` (fichier retiré présent, place libre) | — | oui |
| … `U` après un aller à `F`, les `n` revenues | **fin due (annulation)** | refusés : `SwitchPending`, quelle que soit la racine | `Undo` | non |
| … `U` sinon (aller sans `F`, ou annulation partielle) | annulé | rangement permis ; annulation refusée (état 5) | — | oui |
| … `U` `F` | annulation faite | rangement permis ; annulation refusée (état 5) | — | oui |
| … [`F`] `I` | clos (états 3, 6) | comme la version 1 | — | non |

`mark_tidy_identities` refuse un journal à fin due (`TidyRefused { reason: SwitchPending }`) et accepte une dernière
ligne `F` comme `E` ou `U` : la règle « dernière ligne ni `E` ni `U` ⇒ `NotClosed` » de la version 1 se lit
« ni `E`, ni `U`, ni `F` ». La purge ne retire jamais un journal à fin due (sa dernière ligne n'est pas `I`).

**`SwitchPending` ne dépend pas de la racine** (PV3, majeur 5) : la fin ne touche que la base et `systems/`, pas les
fichiers. Un journal à fin due, quelle que soit sa ligne `R`, fait refuser `apply_tidy`, `apply_switch`, `undo_tidy` et
`mark_tidy_identities` pour **toute** racine (le changement de dossier des ROMs ne la contourne pas) ; `pending_switch`
et `complete_switch` lisent tous les journaux lisibles du dossier sans comparer `R` à la racine courante
(`TidyRootMismatch` ne s'applique pas à eux). Il y a au plus un journal à fin due par construction ; s'il y en a
plusieurs (fichiers copiés à la main), le plus ancien est rendu d'abord. Une bascule incomplète ne reçoit jamais de `F`
: on relance une nouvelle bascule (nouveau journal, `plan_switch` reprend les unités restantes).

**Refus du journal entier** (`LibraryError::Journal`), en plus des sept familles de la version 1 : en-tête suivi
d'autre chose avant d'être complet, ou lignes `X K P Q` dans un autre ordre ; champ d'en-tête hors de sa syntaxe
(ids égaux ou invalides, `nom-retiré` différent du nom recalculé, `n` nul ou non canonique, `K` inconnu, `P` vide,
de plus de 8 cores, avec un doublon ou avec le core `-`, `Q` vide ou non canonique — `%2D` hors du profil `-`
compris —, non UTF-8 une fois décodé ou de plus de 16 Kio) ; nom de fichier suffixé `-<k>` ; toute
ligne `X`, `K`, `P`, `Q` hors de l'en-tête ; `B u` avec `u ≥ n` (aller ou annulation) ; `F` qui ne suit pas
immédiatement `E` ou `U`, second `F` dans une section, `F` d'aller alors qu'une unité n'est pas validée, `F`
d'annulation sans `F` d'aller ou alors qu'une unité n'est pas revenue ; `I` juste après le `E` (ou le `U`) d'une
fin due (il faut `F` d'abord). La **règle 6** de la version 1 se lit, en version 2 : quoi que ce soit après `U` hors
`[F] [I]` dans cet ordre (un seul de chaque) ; `I` ailleurs que juste après `E`, `U` ou `F` ; `F` ailleurs que
juste après `E` ou `U` ; second `E`. La lettre `X` (et non `S`, prise par le dossier d'assemblage) nomme la bascule.

**Reprise et fin.** `recover_tidy` traite un journal version 2 exactement comme un journal version 1 (blocs
ouverts défaits, puis `E` ou `U`) ; il ne pose jamais `F`. Au démarrage, **après** la reprise et **avant** les
identités (ADR 0025 §13), `pending_switch` rend la fin due ; l'interface applique touches et préférence de core
d'après l'en-tête, sans aucune question, puis `complete_switch` fait, sous le verrou du rangement et de façon
idempotente : (1) `reassign_system` de **toutes** les lignes (aller : id perso → id officiel ; annulation :
l'inverse) ; (2) définition : à l'aller, `systems/<id perso>.toml` → `systems/.retired/<nom-retiré>`, à l'annulation
l'inverse ; (3) `F`. Puis publication du catalogue, identités, scan. Règles de l'étape (2) (PV3, majeur 3 et mineur
Windows) :

- **Déplacement sans écrasement** : `systems/.retired/` est créé s'il manque (puis `systems/` synchronisé) ; lien
  physique vers la destination puis retrait de la source — jamais `rename` simple, qui écrase sous Unix et sous
  Windows. **Volume sans liens** (PV3 tour 2) : copie vers un **nom temporaire** du dossier de destination
  (`.retired/` à l'aller, `systems/` à l'annulation ; nom `.<id perso>.emu-rust-tmp`, sans suffixe `.toml`,
  ignoré par le catalogue et par la liste des retirés), synchronisée, puis posée sous son nom final **sans écraser**
  (lien, ou renommage qui refuse d'écraser), puis retrait de la source ; une copie coupée ne laisse ainsi jamais de
  destination tronquée, prise à tort pour une « place prise » puis gardée comme fichier retiré. Un temporaire orphelin
  est supprimé à la reprise (`complete_switch`, avant les cas ci-dessous). `systems/.retired/` et `systems/`
  synchronisés **avant** `F`.
- **Existence par nom exact** : lecture du dossier et comparaison des noms octet par octet (sous Windows et macOS,
  `Path::exists` répond aussi pour une autre casse) ; un nom qui ne diffère que par la casse compte comme une place
  prise, jamais comme « déjà fait ».
- **Cas** : seule la destination existe ⇒ déjà fait ; source et destination présentes et **identiques** (même fichier
  physique, ou même contenu octet pour octet — une définition fait au plus 64 Kio) ⇒ déplacement interrompu, la
  source est retirée ; présentes et différentes ⇒ échec (« place prise ») ; à l'aller, **définition active absente
  traitée comme déjà retirée** (noté au journal de l'application) ; à l'annulation, aucune des deux ⇒ échec
  (« fichier retiré absent »).

- **Pendant tout journal de bascule ouvert** (relecture du 2026-11-25, mineur 1 ; PV3, majeur 2) — du premier `B` de
  l'aller jusqu'à son `E`, ou son `F` s'il est dû (aller en cours, fin due), et de même pour l'annulation : les
  Réglages **refusent** de créer, modifier, supprimer ou restaurer la définition `<id perso>` (« une bascule est en
  cours » / « à terminer ») ; c'est ce qui rend sûre la règle « déjà fait » de l'étape (2). Un journal clos sans fin
  due (bascule incomplète, faite, annulée) ne bloque plus rien. Test en PB3 (refus de l'API) et PU4 (interface).
- **Seule exception : échec « place prise »** (PV3, majeur 1) — à l'aller, définition active et fichier retiré présents
  et différents ; à l'annulation, `systems/<id perso>.toml` présent et différent du retiré. Le bandeau propose alors
  « Retirer ce système et terminer la bascule » : la définition active est retirée par `retire_custom(…,
  Retirement::Removed, maintenant)` (déplacée dans `.retired/`, jamais supprimée, restaurable plus tard), puis
  `complete_switch` est relancé aussitôt. L'interface reconnaît ce cas en relisant `systems/` (noms exacts), pas d'après
  le texte de l'erreur. Test PU4 : fin due en échec « place prise » à l'aller et à l'annulation, retrait proposé puis
  fin réussie, définition retirée visible dans « Systèmes retirés ».
- **Core de l'utilisateur à la fin d'aller** (PV3, majeur 2) : l'interface le lit dans la définition active, sinon dans
  `.retired/<nom-retiré>` (copie entière de la définition, nom noté dans l'en-tête) ; si aucune des deux n'existe
  (définition active absente), l'étape (b) d'ADR 0025 §10 est sautée, notée au journal de l'application, et les
  cores officiels s'appliquent. Aucune question, même résultat à chaque reprise.
- **Échec de la fin** (mineur 2) : le journal reste « fin due » ; rangement, annulation et pose de `I` sont refusés
  (`SwitchPending`) et la fin est retentée à chaque démarrage. C'est la **seule exception** à « jamais de jeu
  rattaché à un système absent du catalogue » (ADR 0025 §10) : après l'étape (1), les lignes portent l'id
  officiel encore masqué (aller) ou l'id perso pas encore restauré (annulation). Le scan ne les touche pas (ADR 0025
  §13), ils ne sont pas affichés ; la bibliothèque et les Réglages montrent un bandeau « Bascule de (système) à
  terminer » avec la raison et l'action (« `systems/<id>.toml` existe déjà » avec le bouton « Retirer ce système et
  terminer la bascule », exception ci-dessus ; « fichier retiré introuvable : remettez `systems/.retired/<nom>` puis
  relancez »), et rien n'est jamais supprimé.
- **Annulation d'une bascule faite** (PV3, majeur 2) : après un `F` d'aller, `check_switch_undo` refuse aussi, avant
  toute action (`SwitchNotReversible`), si `.retired/<nom-retiré>` est absent (nom exact) — « fichier retiré
  introuvable : remettez `systems/.retired/<nom>` » —, en plus de `systems/<id perso>.toml` présent ; sinon
  l'annulation finirait en fin due bloquée.
- **Bascule incomplète et scan** : une ligne connue (même chemin) garde son `game.system` tant que ce système est
  dans l'instantané, même rangée sous `roms/<id officiel>/` alors que l'officiel est masqué ; le scan ne réattribue
  pas son système d'après le dossier (test PB3 : bascule incomplète, puis scan, jeux déplacés toujours sous le
  système personnalisé).
- **Annulation partielle** (annulation interrompue, devenue définitive, §4.8 RV1 5 ; fichier changé entre le
  contrôle et l'exécution) : pas de `F` ; la définition reste retirée, l'officiel actif ; les jeux revenus restent
  rattachés à l'officiel depuis `roms/<id perso>/`, dossier réservé au rangement (ADR 0025 §12) : **le rapport et
  le guide d'essai disent de les déplacer à la main vers `roms/<id officiel>/`, puis de relancer le scan**
  (réponse R1 a)).
- **Annulation et dossiers réservés** (relecture du lot PB2, 2026-11-27 soir ; ADR 0025 §12) : l'annulation d'un
  journal, version 1 comprise et même écrit avant qu'un dossier ne devienne réservé, ne prend **rien** dans un
  dossier `roms/<id>/` réservé de l'instantané courant et n'y pose rien : une telle unité reste entière où elle est
  et figure au rapport (§4.6) ; pour un journal de bascule, `check_switch_undo` la compte comme une unité qui ne peut
  revenir (refus avant toute action). Un dossier réservé n'est **jamais** retiré par l'annulation, même vide (§4.6 :
  « dossiers créés retirés s'ils sont vides » ne s'y applique pas). Seuls exemptés, pour un journal de bascule :
  `roms/<id perso>/` et `roms/<id officiel>/`. Noms comparés sans tenir compte de la casse ASCII. Le déplacement
  de la définition (étape (2)) est idempotent selon les « Cas » ci-dessus : `move_no_overwrite` retire la source
  quand la destination identique est déjà là. Tests : design 8.0 §2.6.
- **Retour à une 7.x** : un binaire 7.x refuse en entier un journal version 2 (en-tête exact `emu-rust tidy 1`
  attendu) et refuse alors tout rangement et toute annulation **tant que ce fichier est présent** dans
  `<data>/rangement/`, même si un journal version 1 plus récent existe ; il ne le purge jamais. Revenir à une 7.x
  après une bascule demande donc de mettre ce fichier de côté (écrit aussi dans `docs/guide-essai-8.0.md`, lot
  PU3). Sans effet pratique en 8.0 (aucune bascule réelle) ; la base v8 est de toute façon refusée par une 7.x.
- **Tests (PB3)** : ceux du design 8.0 §2.6 (troncature à chaque octet, en-tête compris ; annulation entre deux
  unités puis coupure ; unité changée depuis l'aperçu ; ligne d'une racine externe débranchée ; définition recréée
  avant l'annulation ; échec de `complete_switch` ; panne entre chaque étape de `complete_switch` ; journal
  version 1 lu à l'identique), plus : définition active absente à l'aller (traitée comme retirée) ; création ou
  suppression de la définition refusée pendant une fin due ; `I` refusé sur une fin due ; après la relecture PV3 :
  panne entre le lien et le retrait de la source, puis entre le déplacement et `F` (reprise ⇒ déplacement fini,
  `F`) ; `systems/.retired/` absent ; nom de casse différente (place prise, jamais « déjà fait ») ;
  `check_switch_undo` sans fichier retiré ; `SwitchPending` sous une autre racine, `pending_switch` et
  `complete_switch` d'un journal d'une autre racine ; `mark_tidy_identities` après `F` ; troncature juste avant
  le premier `B` (bascule incomplète vide) ; `P` et `Q` non canoniques, `P` avec le core `-`, profil `-`
  (`%2D`), profil et préférence vides écrits `-` ; nom suffixé `-<k>` refusé ; scan après bascule incomplète.

## Contexte

Faits relevés dans le code de `wip-7.1` (= `v7.0.0-rc.1`) :

- `emu-app` est un binaire console : sous Windows une fenêtre de terminal s'ouvre avec l'application. Le journal
  (`tracing`, filtre `warn` + `info` pour nos crates) et les erreurs fatales (`eprintln!`) ne vont que sur `stderr`.
  `tracing-subscriber` n'est une dépendance que d'`emu-app`.
- `unsafe` est interdit hors `emu-libretro` / `emu-runtime` ; `emu-app` ne dépend que d'`emu-ui`. Les appels
  `AttachConsole` / `AllocConsole` de Windows sont des appels FFI. `windows-sys` est déjà dans `Cargo.lock`
  (0.52.0, 0.59.0, 0.61.2).
- Un programme du sous-système graphique lancé depuis `cmd` ou PowerShell n'est **pas attendu** par l'invite :
  la sortie se mêle à l'invite et `$LASTEXITCODE` / `%ERRORLEVEL%` ne sont pas renseignés, même après
  `AttachConsole`. Un programme du sous-système console lancé par un processus sans console reçoit de Windows une
  console neuve. Dans un processus graphique sans redirection, les poignées standard sont nulles (lisible en Rust
  sûr : `AsRawHandle`).
- Le scan ne suit aucun lien de dossier (`scan.rs`, parcours) ; `group_media` ne regroupe que des fichiers du
  **même dossier** ; une ligne nouvelle est insérée par `store_batch` avant la purge des lignes disparues ; les
  lignes d'une racine externe absente sont protégées (`keep`). Le registre d'identité compare des chemins
  **canoniques** (`registry_key`). Le verrou du registre est un verrou de l'OS (`File::lock`).
- La bibliothèque identifie un jeu par son **chemin** (`game.path UNIQUE`, `game_media.path UNIQUE`) : favori,
  historique, identification et jaquettes sont portés par la ligne. Le scan ne reconnaît pas un fichier déplacé :
  il supprime la ligne et en crée une autre (favori et historique perdus).
- Sauvegardes (ADR 0011, amendement 2.1) : `.srm`, états, options par jeu et dossier `.disks` sont nommés par le
  `save_name`, dérivé du **nom du fichier**, pas de son dossier. Le registre `<saves>/<core_id>/.emu-rust-games`
  mémorise le chemin de la source ; « même empreinte et ancien chemin disparu ⇒ même identité » existe déjà.
- Classement au scan : dossier nommé d'après l'`id` du système > sondes d'en-tête > DAT > extension ; les
  extensions partagées (`folder_extensions` : `.chd`, `.cue`, `.iso`, `.m3u`, `.ipf`, `.zip` d'ordinateur…) ne sont
  attribuées **que** sous `roms/<id>/`, sinon le fichier est « non classé ». Les jeux-dossiers DOS ne sont reconnus
  que sous `roms/dos/`. 17 systèmes au catalogue (`systems.toml`).
- Firmware : `cores.toml` porte `required_firmware` (avec `fallback` pour PlayStation, 3DO, Amiga ⇒ état
  `CoreFallback`, avertissement une fois par jeu depuis la 3.0 / 7.0) et `builtin_firmware` ; les `.info` installés
  listent `firmwareN_path` / `firmwareN_opt` (facultatif) ; `firmware.toml` donne les md5 de référence.
- Interface : thème sombre d'egui, accent bleu `#285AA0`, contour de focus `#96C8FF`, avertissement `#FFBE50`.
  Aucune icône (ni fenêtre, ni exécutable, ni `.app`, ni `.desktop`). Presse-papiers : voir l'amendement en fin d'ADR.
- Outils ici : ImageMagick (rendu SVG interne) et Python 3 sans Pillow ; ni `rsvg-convert` ni Inkscape.

## Décision

### 1. Pied de page

Bandeau bas (`TopBottomPanel::bottom`) sur les écrans hors jeu (bibliothèque, cores, réglages) et en bas du menu en
jeu : `emu-rust <version> — dev by claude@jerhome`, version = `env!("CARGO_PKG_VERSION")` (version du workspace,
exposée par `emu_ui::VERSION`, la même que `--version`). Le bandeau porte aussi le bouton « Journal » (§3). Jamais
dessiné par-dessus l'image du jeu.

### 2. Icône

- **Une seule image**, source SVG `packaging/icon/emu-rust.svg` = **variante B** (croix directionnelle et deux
  boutons ronds sur fond bleu ; choix de l'utilisateur du 2026-11-21 ; formes pleines, sans dégradé, sans texte,
  sans marque ni manette d'un fabricant). Les quatre propositions restent dans `docs/design/icone/`.
- **Fichiers générés et commités** (une compilation n'a besoin d'aucun outil graphique) par `scripts/gen-icons.sh` :
  PNG 16 à 1024 px, `emu-rust.ico` (16, 24, 32, 48, 64, 256), `emu-rust.icns`. Rasterisation et `.ico` par
  **ImageMagick** (son rendu SVG interne suffit à des formes pleines : vérifié sur la planche) ; `.icns` par un
  script Python sans dépendance (conteneur de PNG). Pas d'image Docker d'outillage. Le script écrit
  `packaging/icon/SOURCE.sha256` (empreinte du SVG) ; la CI vérifie seulement que cette empreinte correspond au
  SVG commité (un rendu identique au bit près d'une version d'outil à l'autre n'est pas exigé).
- **Windows** : icône et informations de version dans l'exécutable par une ressource, posée par `build.rs`
  d'`emu-app` avec `winresource` (MIT, `build-dependencies` sous `cfg(windows)`, outil `rc.exe` du runner MSVC).
  Compilation croisée depuis Linux : pas de ressource, pas d'échec. **macOS** : `Contents/Resources/emu-rust.icns`
  et `CFBundleIconFile`. **Linux** : `share/icons/hicolor/<taille>/apps/emu-rust.png`, `scalable/apps/emu-rust.svg`,
  `share/applications/emu-rust.desktop` (`Terminal=false`, `StartupWMClass=emu-rust`) dans le `tar.gz`.
- **Fenêtre** (toutes les fenêtres : jeu, bibliothèque, cores) : `with_window_icon` de `winit` à partir des PNG
  de `packaging/icon/png/` embarqués par `include_bytes!` (32, 64, 256 ; décodés par `png`, déjà au workspace ;
  aucune copie dans `emu-ui`) — barre de titre et barre des tâches Windows, X11
  (`_NET_WM_ICON`, donc noVNC). Wayland n'a pas d'icône de fenêtre : `app_id` = `emu-rust` pour que le `.desktop`
  s'applique. macOS ignore l'icône de fenêtre (Dock = `.icns`). Aucune dépendance nouvelle à l'exécution.

### 3. Console cachée, journal dans l'interface

- **Windows : deux exécutables, le même programme.** `emu-rust.exe` (sous-système graphique : aucun terminal au
  double-clic) et `emu-rust-cli.exe` (sous-système console), deux cibles `[[bin]]` d'`emu-app` de trois lignes
  chacune qui appellent la même fonction de la bibliothèque d'`emu-app`. Le second n'est construit et livré que
  sous Windows (fonctionnalité Cargo `cli-bin`). Solutions écartées : `AttachConsole` / `AllocConsole` dans un
  exécutable unique — l'invite n'attend pas, `$LASTEXITCODE` est faux (scripts et guide d'essai cassés), chaque
  poignée standard doit être reprise à la main (`CONOUT$`, `SetStdHandle`), le tout en `unsafe` et sans test
  automatique possible hors ConPTY. Avec deux exécutables, **Windows fait le travail** et il ne reste aucun appel
  de console :
  1. ligne de commande (`--help`, `--version`, `cores …`, `--check-bios`, scripts, CI) : `emu-rust-cli.exe` —
     attendu par l'invite, code de retour et tubes corrects. `README.txt` et l'aide le disent ;
  2. `emu-rust.exe` avec `[interface] console = true`, en mode fenêtre et sans sortie standard : il **relance**
     `emu-rust-cli.exe` (fichier voisin, mêmes arguments, variable `EMU_RUST_CONSOLE_CHILD=1` contre toute boucle)
     et se termine ; Windows ouvre la console du second. Le réglage est lu au démarrage : **effet après
     redémarrage** (dit dans les Réglages). Voisin absent ⇒ avertissement au journal, démarrage sans console ;
  3. `emu-rust.exe` appelé avec une sous-commande : elle s'exécute (sortie correcte si elle est redirigée : tube,
     fichier) ; sans sortie standard, une boîte de message renvoie vers `emu-rust-cli.exe`.
  Seul appel FFI restant : `MessageBoxW` (erreur fatale avant l'ouverture d'une fenêtre, sans console), dans
  `emu-runtime` (module `dialog`, `// SAFETY:`, revue `reviewer`), derrière une fonction sûre réexportée par
  `emu-ui`. La détection « pas de sortie standard » est en Rust sûr. Le lancement réel avec console (point 2)
  reste un **essai de l'utilisateur** ; la décision (pure) et les en-têtes PE des deux exécutables sont testés.
- **Linux / macOS / Docker** : il n'y a pas de fenêtre de terminal propre à l'application. « Caché » veut dire :
  lancement par l'icône (`.desktop` `Terminal=false`, `.app` par le Finder) sans terminal ; lancé **depuis** un
  terminal, le journal continue d'y être écrit (c'est le terminal de l'utilisateur, `docker logs` en dépend). Le
  réglage « Afficher la console » n'est proposé que sous Windows ; ailleurs une ligne d'explication le remplace.
- **Journal capturé** : une couche `tracing` (`emu_ui::log`) alimente un tampon circulaire en mémoire, installé par
  `emu-app` dès le début de `main` à côté de la sortie `stderr` (conservée). Bornes : 2 000 entrées, message tronqué
  à 2 000 caractères, lignes des cores nettoyées (`sanitize_core_line` : caractères de contrôle et bidi retirés).
  Niveaux capturés : ceux du filtre actuel (`warn` global, `info` pour nos crates et le core ; `RUST_LOG` l'emporte).
  Les secrets ne sont déjà jamais journalisés (`emu_assets::Secret`) : inchangé.
- **Fichier** : `<data>/logs/emu-rust.log`, alimenté par le même tampon dès que la racine est connue ; au démarrage
  l'ancien devient `emu-rust.log.1` ; écriture arrêtée à 2 Mio (ligne « journal tronqué »). Un `panic` y est écrit
  (crochet de panique) : sans console c'est la seule trace d'un plantage. Mode `0600` sous Unix. Rotation ou
  ouverture impossible (seconde instance) ⇒ pas de fichier pour cette instance, un avertissement, rien d'autre.
- **Panneau « Journal »** (pied de page, avec un compteur d'erreurs et d'avertissements non lus ; aussi dans le menu
  en jeu) : liste horodatée, filtre Erreurs / Avertissements / Tout, origine (application ou core) et « Ouvrir le dossier
  des journaux » (le fichier suffit, pas d'« Enregistrer sous… » ; copie par le bouton « Copier », voir l'amendement presse-papiers). Ce qui n'existait que sur le terminal y
  figure : racine des données, core et jeu chargés, messages du core, refus d'un core (`core_error`), conseils de
  `diagnostics`. Les messages déjà affichés (bandeaux, OSD) restent où ils sont.

### 4. Rangement de `roms/`

**4.1 Périmètre.** Réglages → « Dossier des ROMs » : (a) « Créer les dossiers des systèmes » crée `roms/<id>/` pour
chaque système du catalogue (17 aujourd'hui), sans rien déplacer ; (b) « Ranger les ROMs… » analyse, **montre un
aperçu**, puis déplace ce que l'utilisateur a laissé coché. Seul `<data>/roms` est rangé (les dossiers externes de
`[library] roots` ne sont jamais touchés). Est « déjà rangé » tout élément dont le chemin relatif commence par le
dossier de **son** système. Le rangement **déplace, ne renomme jamais, ne supprime jamais, n'écrase jamais, ne
décompresse jamais** ; il ne suit aucun lien symbolique (un lien est laissé en place et signalé).

**4.2 Unités.** L'analyse regroupe les fichiers en unités indivisibles, déplacées en bloc :

| Unité | Contenu | Destination |
|---|---|---|
| Fichier seul | ROM de console, `.zip`, image unique (`.chd`, `.iso`, `.pbp`, `.adf`, `.st`, `.d64`…), avec le `.sbi` de même nom d'un `.chd` / `.pbp` / `.iso` PlayStation | `roms/<id>/<nom>` |
| Jeu à plusieurs disquettes | groupe `(Disk N of M)` (`group_media`), mêmes règles que le scan | `roms/<id>/` (à plat, côte à côte) |
| CD à plusieurs fichiers | `.cue` + pistes référencées (`cue_references`) + `.sbi` ; `.m3u` + ses disques et leurs pistes | `roms/<id>/<nom du .cue ou du .m3u>/` |
| **Jeu à plusieurs CD sans `.m3u`** | le groupe `(Disc N)` **entier** : tous ses `.cue` (ou `.chd`) et toutes leurs pistes (`group_media` exige le même dossier, sinon le jeu serait éclaté en un jeu par disque) | `roms/<id>/<nom du groupe>/`, **un seul dossier** ; système sans `group_discs` (3DO) : une unité par disque |
| Pistes `.bin` sans `.cue` | regroupées comme le scan (`hide_disc_tracks`, `audio_track_name`) : piste de données + pistes audio de même radical | comme un CD à plusieurs fichiers ; regroupement impossible ⇒ **bloquée** |
| Jeu-dossier DOS | dossier avec un `.exe` / `.com` / `.bat` à sa racine | `roms/dos/<dossier>/` (le dossier entier) |
| Dossier quelconque | ni CD ni DOS | ses fichiers sont traités un par un ; le dossier vidé est retiré (noté au journal) |

Un dossier source qui contient exactement une unité à dossier (et rien d'autre) est déplacé tel quel, sous son
propre nom. Un jeu-dossier DOS laissé décoché reste **entier** en place : ses fichiers ne redeviennent jamais des
unités séparées. Un fichier appartient à une seule unité (piste citée par deux `.cue` ⇒ les deux unités bloquées).
Les fichiers qui ne sont pas des jeux (textes, images, BIOS déposés par erreur) restent en place ; un fichier dont
le md5 est celui d'un BIOS connu est signalé avec « Importer un BIOS… ».

**Unité bloquée** (expliquée dans l'aperçu, rien n'est déplacé pour elle) : référence de `.cue` / `.m3u` absolue,
hors de son dossier ou manquante ; lien symbolique ; **`roms/<id>` qui n'est pas un vrai dossier** (lien,
jonction) ou qui est sur un **autre volume** (point de montage) — le scan ne suit pas les liens et la 7.1 ne
copie pas entre volumes ; chemin non UTF-8 (la base stocke du texte) ; chemin de destination de plus de 240
caractères sous Windows ; dossier à créer dont le nom est l'`id` d'un système (il fausserait la règle de dossier) ;
système de fichiers sans liens physiques (FAT, exFAT).

**4.3 Système de destination.** Une unité **déjà connue de la bibliothèque** (`known_game`) garde le système de sa
ligne : statut Sûre, raison « bibliothèque » (ses sauvegardes sont sous `<saves>/<core_id>/`, donc liées au
système). Sinon, même ordre que le scan, appliqué « comme si aucun dossier ne décidait » :
(1) sondes d'en-tête (`disc_info`, `probe_bin`, iNES, en-tête Mega Drive / 32X…) ; (2) empreinte dans un DAT importé
(CRC32 / SHA1, série, piste 01) ; (3) contenu de l'archive (`.zip` : extensions des entrées ; une image CD dans un
zip reste refusée, règle 2.1) ; (4) nom présent dans **un seul** DAT ; (5) extension propre à un seul système.

- **Sûre** (bibliothèque, 1, 2, 3 ou 5) : cochée d'office.
- **Suggérée** (4 ; jeu-dossier DOS ; zip pris pour du DOS ; **`.bin` sans en-tête**) : montrée **décochée**,
  avec la raison. Un `.bin` que seule son extension désigne (ni en-tête Mega Drive / 32X reconnu par la sonde, ni
  empreinte dans un DAT) n'est jamais « Sûre » : l'extension est aussi celle de BIOS et de pistes de CD isolées
  (choix de FB1a, validé par la revue FV0).
- **Ambiguë** (plusieurs systèmes possibles : `.chd`, `.m3u`, `.dsk`, `.bin`, `.gb` sans DAT…) : l'aperçu propose
  les systèmes candidats, unité par unité ou **pour toute une extension d'un coup** (« tous les `.chd` ambigus →
  PlayStation ») ; sans choix, l'unité reste en place.
- **Inconnue** : laissée en place.

Choisir pour un jeu connu un système **autre** que celui de sa ligne est possible mais averti : « le core change,
les sauvegardes et états de ce jeu ne suivront pas ». Une fois sous `roms/<id>/`, la règle de dossier du scan fait
foi : c'est ce qui rend lançable un fichier jusque-là « non classé ». Les alias de dossier (`ps1/`) restent refusés.

**4.4 Collisions.** Destination déjà occupée (comparaison insensible à la casse, sur le disque **et** entre unités
du plan) : même taille et même CRC32 ⇒ **doublon**, sinon **conflit** ; dans les deux cas l'unité reste en place,
signalée. Aucun renommage automatique : le `save_name` dépend du nom du fichier.

**4.5 Exécution.** Tâche de fond annulable (entre deux unités), refusée pendant un scan, un téléchargement ou une
partie de cette instance ; **verrou de l'OS** (`File::try_lock` sur `<data>/rangement/lock`, comme le registre
2.1 : libéré par le système si le processus meurt). `roms_root` est **canonisé** avant toute chose : journal, base
et registres voient la même écriture des chemins. Par unité :

1. `B <unité>` et les intentions au **journal** (`<data>/rangement/journal-<unix>.txt`, texte, une opération par
   ligne, chemins relatifs à `roms/` échappés, racine canonique en en-tête, `sync_all` avant tout déplacement) ;
2. déplacement **sans jamais écraser** : fichier seul = `fs::hard_link(source, destination)` (échoue si la
   destination existe) puis `fs::remove_file(source)` ; unité à dossier assemblée = `create_dir` exclusif de
   `<destination>.emu-rust-part/`, liens physiques de chaque fichier, **un seul `rename`** du dossier vers son nom
   final, puis retrait des sources ; dossier déplacé tel quel (jeu DOS, CD déjà dans son dossier) = **un seul
   `rename`** (qui ne peut remplacer qu'un dossier vide sous Unix et échoue sous Windows). Autre volume
   (`ErrorKind::CrossesDevices`) ou liens physiques refusés ⇒ unité bloquée, rien n'a bougé ;
3. **bibliothèque**, dans une transaction : `game.path`, `game_media.path` et `game.pinned` de l'unité sont
   réécrits (`Library::relocate`, clés construites par la même fonction que le scan) — la ligne est conservée,
   donc favori, historique, identification, région et jaquettes aussi ; une ligne périmée déjà enregistrée à la
   destination (fichier absent) est retirée dans la même transaction ;
4. `C <unité>` (unité validée). Un échec en 2 ou 3 défait l'unité ; les autres continuent ; tout figure au rapport.

Après la dernière unité, ligne `E`, puis : (5) `emu_runtime::relocate_identities` réécrit le chemin des sources
dans les registres `<saves>/<core_id>/.emu-rust-games` et les manifestes `.disks` (sous leur verrou, écriture
atomique, comparaison par `registry_key` de chemins canoniques) ; **alors seulement** l'UI fait poser la ligne
`I` (« identités faites ») par `Library::mark_tidy_identities` ; (6) scan incrémental, qui ajoute les jeux
nouvellement classés. **Aucun fichier de `saves/`, `states/`, `game-options/` ni `covers/` n'est déplacé ni
renommé** : ils sont nommés par le `save_name` ou rattachés à la ligne du jeu, qui ne changent pas.

L'étape 5 n'est pas facultative : sans elle le registre dit encore « `roms/a.sfc` → sauvegarde `a` », et un
**autre** `a.sfc` déposé plus tard à la racine hériterait du `.srm` du jeu rangé. Tant qu'un journal validé n'a
pas sa ligne `I`, l'étape 5 est rejouée (idempotente) à l'ouverture de la bibliothèque **et** avant tout lancement
direct par la ligne de commande.

**Reprise** (au démarrage, avant le scan ; refusée si la racine canonique du journal n'est pas la racine courante),
**une seule règle, par unité** : une unité commencée et non validée (`B` sans `C`) est **défaite en entier** —
chaque fichier présent à la destination et absent à la source revient à la source ; **source et destination
présentes et identiques** (même fichier physique prouvé par le système de fichiers : arrêt entre le lien et le
retrait ; deux copies égales ne suffisent pas, §4.8) ⇒ la destination est retirée, la source reste ; présentes et différentes ⇒ rien n'est touché, signalé ;
un dossier `.emu-rust-part` est retiré après ce contrôle fichier par fichier ; la base est remise sur les chemins
sources (réécriture inverse, idempotente). Une unité validée (`C`) n'est jamais retouchée. Puis `E` est posée et
l'étape 5 rejouée.

Filet de sécurité indépendant : le **scan** reconnaît un fichier déplacé — ligne dont le chemin a disparu + ligne
nouvelle de mêmes système, taille et empreinte (ou mêmes nom, taille et date pour un fichier non haché), un seul
candidat de chaque côté. La ligne nouvelle étant déjà insérée (`store_batch`), ses champs sont **reportés sur
l'ancienne ligne, qui garde son `id`** (les jaquettes y sont rattachées), puis la nouvelle est supprimée. Les
lignes protégées (`keep` : racine externe débranchée) ne sont jamais candidates. Cela protège aussi les
déplacements faits à la main.

**4.6 Annulation.** « Annuler le dernier rangement » (Réglages, et bouton du rapport ; même verrou, même contrôle
de racine) rejoue le dernier journal à l'envers, unité par unité, avec les mêmes primitives : une unité revient si
tous ses fichiers sont encore à leur destination, inchangés (taille + date), et si leur place d'origine est libre ;
sinon elle reste entière où elle est et figure au rapport. Dossiers créés retirés s'ils sont vides, dossiers vidés
recréés ; mêmes mises à jour de la base et des registres en sens inverse (avec leur propre ligne `I`). Le journal
est alors marqué « annulé » ; les 20 derniers sont gardés. Seul le journal **le plus récent** s'annule ; s'il est
illisible, rien ne s'annule (on ne saute jamais à un plus ancien). Précisions : §4.8.

**4.7 Schéma.** Aucune migration : le journal est un fichier, `relocate` n'utilise que les colonnes existantes. Après
la fusion (schéma **v7**, 7.4), `relocate` réécrit les chemins sans toucher aux autres colonnes : `game.needs_32x`
suit le jeu déplacé (test exigé en 7.6).

**4.8 Journal : décisions de l'amendement du 2026-11-22** (revue FV0 du format écrit par FB1a ; grammaire, états
et reprise détaillés : design §3.4.1, qui fait foi pour le format). La version du format reste 1 : aucun journal
n'a encore été écrit chez un utilisateur.

- **Taille et date dans les intentions.** `M <n> <taille> <mtime> <de> <vers>` ; `W <fichiers> <taille> <mtime>
  <de> <vers>` (agrégat récursif du dossier : nombre de fichiers, somme des tailles, date la plus récente). `mtime`
  = secondes entières depuis 1970 UTC, tronquées (la valeur du scan, `game.mtime_unix`). Comparaison du journal au
  disque : taille égale et dates à **2 s** près. C'est le « inchangés (taille + date) » du §4.6 ; à la reprise, la
  date ne prouve jamais que deux fichiers sont identiques (fichier physique ou taille + CRC32, §4.5), elle sert
  seulement à ne pas rapatrier un fichier que le journal ne prouve pas avoir posé.
- **Section d'annulation.** L'annulation écrit **dans le même journal**, après `E`, des blocs `B u` … `C u` aux
  déplacements inversés, puis `U`. La règle de reprise du §4.5 (« `B` sans `C` ») s'y applique telle quelle : une
  unité dont le retour a été interrompu est remise à son emplacement rangé, puis `U` est posé. Une seule section
  d'annulation par journal : après une annulation partielle (unité modifiée, place d'origine occupée, interruption,
  coupure), les unités restées rangées le restent.
- **Identités.** Tout marqueur de clôture (`E`, `U`) rend caduc le `I` antérieur : les identités sont dues tant
  que la dernière ligne du journal n'est pas `I`. La liste rejouée est **nette** — une paire (chemin périmé →
  chemin actuel) par fichier d'une unité validée, dans le sens de l'aller si l'unité est restée rangée, inverse
  si elle est revenue — donc juste que le `I` de l'aller ait été posé ou non.
- **Journal strict.** Toute ligne hors grammaire (déplacement hors d'un bloc, `C` qui ne ferme pas le `B` ouvert,
  numéro dupliqué, assemblage mal nommé, destination hors de l'assemblage, inverse qui n'est pas celui de
  l'aller…) fait refuser le journal **entier** : rien n'est rejoué ni complété. Un journal en cours ou illisible
  bloque tout nouveau rangement et toute annulation ; un fichier vide ou à en-tête coupé vaut « rien n'a bougé ».
  Le journal est créé au premier `B` ; l'aller est clos avant 7 Mio pour que son annulation tienne sous la borne
  de lecture de 16 Mio ; la purge des 20 derniers ne supprime qu'un journal dont la dernière ligne est `I`.
- **Précisions RV1** (revue du contrat, 2026-11-22 ; détail : design §3.4.1).
  1. Le journal refuse tout `M` / `W` dont `de` et `vers` sont égaux après pliage de casse et normalisation
     Unicode ; sous Unix, la reprise exige `nlink ≥ 2` avant de retirer une destination « identique » à sa
     source. Un seul fichier vu sous deux noms n'est jamais retiré.
  2. Si la défaite d'un bloc échoue elle-même (erreur d'E/S), l'exécution s'arrête sans `E` ni `U` : le journal
     reste en cours et est repris au démarrage suivant ; `recover_tidy` rend une erreur sans poser de clôture.
  3. Nettoyage d'un assemblage défait : les `D+` du plus profond au moins profond (cherchés aux deux
     emplacements, comme les `M`), puis `S`, puis `<D>` ; « non vide » est silencieux pour un `D+`.
  4. Un bloc peut écrire `D+` pour les ancêtres de `<D>` qu'il crée réellement ; un `D+` n'est écrit que par le
     bloc qui crée vraiment le dossier à l'exécution, jamais d'après le plan.
  5. Chaque ligne du journal est synchronisée (`m`, `w`, `C`, `E`, `U`, `I` compris) ; le dossier source l'est
     après le retrait, avant `C`. Une annulation interrompue devient **définitive** (l'unité est remise rangée,
     `U` posé) et le rapport de reprise le dit. Un journal plus récent d'une autre racine bloque l'annulation.
- **Limites assumées.** ~~Sous Windows, un fichier plus gros que la borne de hachage, coupé entre le lien et le
  retrait de sa source, ne peut pas être prouvé identique : il reste en double à la destination.~~ (Caduc depuis
  FV1a 1 et RV3 3 : l'identité physique ne dépend plus de la taille.) Sur FAT / exFAT (unités `W` seulement, les autres y sont bloquées), un changement d'heure
  peut décaler les dates d'une heure : l'unité n'est alors pas annulée (elle reste rangée, signalée). Deux
  instances qui posent `I` et annulent en même temps ne sont pas départagées au-delà du verrou de chaque appel
  (limite « seconde instance » ci-dessous).
- **Décisions après la revue FV1a** (2026-11-22 ; rapport `.claude/fv1a-7.1.md`).
  1. **Identité physique (I3).** Dans la branche destructrice de la reprise et de la défaite (retirer un
     exemplaire « identique »), l'identité de deux exemplaires est prouvée par le crate `same-file` (déjà
     transitif via `walkdir`, il devient dépendance directe d'`emu-library` ; l'implémenteur vérifie sa licence),
     avec en plus le garde `canonicalize(src) != canonicalize(dst)`. La borne de 512 Mio (taille + CRC32)
     disparaît pour cette branche. Si un fichier ne peut pas être ouvert, le cas est signalé et rien n'est retiré.
  2. **Windows, `sync_dir`** (m4) : sans effet, acceptable sur NTFS. Sur FAT / exFAT, un renommage de dossier
     (unités `W`) n'est ni atomique ni ordonné : limite assumée, à ajouter à celles ci-dessus.
  3. **`fold_path`** : le pliage de casse et d'Unicode est volontairement plus grossier que NFC (`é` == `e`) et
     incomplet pour certains alphabets. Il va dans le sens sûr : refus ou `Conflict`, jamais fusion.
     `Pokémon.gb` et `Pokemon.gb` visant le même dossier restent non rangés, le message montre le fichier existant.
  4. **Fichier ouvert par un autre programme** (Windows) : violation de partage, donc `Fatal` ; le journal reste
     en cours jusqu'à la fermeture du fichier, puis reprise. L'UI doit l'expliquer.
  5. **Sur `Fatal`**, le `TidyReport` partiel est perdu : l'UI enchaîne `recover_tidy` puis
     `pending_tidy_identities`.
- **Décisions après la revue RV3** (2026-11-23 ; rapport `.claude/rv3-7.1.md` ; détail et interfaces : design
  §3.4.2). **À livrer avant tout tag 7.1.**
  1. **Fichier en lecture seule (Windows).** L'attribut « lecture seule » n'empêche pas le lien physique mais
     interdit le retrait du nom : sans parade, l'aller crée le lien, échoue au retrait de la source, échoue
     aussi à défaire (même fichier, même attribut) et chaque reprise repart en `Fatal` — rangement bloqué
     pour toujours, sans perte. Décision : (a) avant tout lien, sous `cfg(windows)`, tout fichier que le bloc
     retirera (sources `M` à l'aller, noms rangés `M` à l'annulation) est contrôlé ; attribut posé ⇒ unité
     laissée, motif dédié `TidyBlock::ReadOnlyFile`, déjà montré dans l'aperçu ; (b) si malgré tout le retrait
     d'un **second nom prouvé** (même fichier physique, §4.8 FV1a 1, ou lien que le bloc vient de poser)
     échoue sur « accès refusé », ce n'est **pas fatal** : les deux noms restent, le cas est rendu dans
     `TidyReport::extra_names` (« retirez l'attribut puis retirez ce nom (X) à la main »), la base pointe sur le nom
     gardé et le journal peut être clos ; (c) emu-rust **ne modifie jamais** l'attribut ni les droits ;
     (d) tests sous `cfg(windows)` exécutés par la CI Windows.
  2. **Écriture refusée** (droits du dossier, volume protégé en écriture, Unix comme Windows) : un lien, une
     création de dossier ou un `rename` refusé **avant** tout retrait de source laisse l'unité, motif
     `TidyBlock::NotWritable { dir }`, au lieu de `NoHardLinks` ou d'une erreur. Les codes sont distingués
     (design §3.4.2) : « pas de liens physiques » ≠ « écriture refusée ».
  3. **Borne de 512 Mio retirée** de toute branche destructrice : retirer un exemplaire exige l'identité
     physique (FV1a 1), quelle que soit la taille ; deux copies égales (taille + CRC32) ne suffisent jamais.
     `max_file_size` ne sert plus, dans le rangement, qu'à l'aperçu (doublon ou conflit), sans rien retirer.
  4. **Liens comptés sous Windows aussi.** La règle `nlink ≥ 2` (RV1 1) et le refus d'annuler un fichier à
     plusieurs liens valent sous Windows : `winapi_util::file::information(..).number_of_links()` (API sûre,
     `winapi-util` 0.1.11 devient dépendance directe d'`emu-library` sous `cfg(windows)`, déjà au verrou,
     Unlicense/MIT ; aucun `unsafe` dans `emu-library`). Handle impossible à ouvrir ⇒ identité non prouvée,
     rien retiré.
  5. **ReFS.** (a) ReFS sans liens physiques (versions anciennes) : le lien est refusé ⇒ `NoHardLinks`, rien
     n'a bougé ; (b) sur ReFS, l'index de fichier 64 bits qu'utilise `same-file` n'est pas garanti unique (les
     identifiants ReFS font 128 bits) : sous Windows, la preuve d'identité exige en plus des tailles et des
     dates de création et de modification **exactement** égales sur les deux noms, `nlink ≥ 2` et deux chemins
     canoniques distincts. Le faux positif résiduel (deux fichiers différents de même index, même taille, mêmes
     dates, chacun à plusieurs liens, aux deux noms d'une ligne du journal) est une **limite assumée**.
  6. **Aller : identité vérifiée avant le retrait de la source.** Après le lien et avant de retirer la source,
     `same_file::is_same_file(source, destination)` doit répondre « oui », sinon erreur d'E/S de l'unité (défaite
     par la règle de reprise). Branche « même fichier » de la reprise : le dossier de la source est synchronisé
     **avant** le retrait de la destination.
  7. **Erreurs distinguées** (interfaces : design §3.4.2). `LibraryError::Journal` = journal **illisible**
     seulement (le seul cas où l'UI conseille de déplacer le fichier) ; un journal lisible dont l'état refuse
     l'action = `LibraryError::TidyRefused { journal, reason }` ; un échec d'écriture du journal =
     `LibraryError::Io`. `recover_tidy` dit ce qu'il a clos (`TidyReport::closed`, avec la section
     interrompue) et ne perd plus son rapport quand un autre journal reste bloquant.

### 5. Cores améliorés par un fichier d'origine

- **Source de l'information**, sans rien télécharger : (a) *dérivée* — tout `required_firmware."<système>"` de
  `cores.toml` qui déclare un `fallback` (PlayStation ×3, 3DO, Amiga) : le core tourne avec un remplaçant, le
  fichier d'origine l'améliore ; (b) *déclarée* — nouvelle clé `optional_firmware."<système>"` de `cores.toml`
  (liste `{ file, benefit }`), tenue à la main et courte (BIOS GBA de mgba, ROM de démarrage Game Boy, ROM MT-32 de
  dosbox_pure…), **inerte** si le `.info` installé du core ne liste pas ce fichier. Les `.info` seuls ne suffisent
  pas : `firmwareN_opt` ne distingue pas « améliore » de « requis pour certains contenus ». md5 de référence :
  `firmware.toml`, sinon `.info`. Un fichier **requis** sans repli (BIOS Mega CD) garde son dialogue bloquant.
- **Affichage permanent** : écran Cores, ligne du core — « Peut être amélioré par un fichier d'origine », avec le
  fichier, son état (présent et vérifié / absent / md5 inattendu), le bénéfice et « Importer un BIOS… » (3.0).
- **Avertissement, une fois, non bloquant** : au premier lancement d'un jeu concerné, une fois par couple
  (core, fichier) — mémorisé dans `[interface] firmware_hints_seen` de `config.toml`. Type *dérivé* : le dialogue
  de la 7.0 (« Importer un BIOS… » / « Continuer »), qui passe d'« une fois par jeu » à « une fois par core et
  fichier » (**confirmé par l'utilisateur** le 2026-11-21) ; un jeu déjà averti (`GameInput::aros_acknowledged`
  de ses options) ne l'est pas de nouveau et sa clé est ajoutée. Type *déclaré* : simple bandeau d'information
  dans la bibliothèque, refermable. Le texte dit toujours : « emu-rust ne télécharge jamais ce fichier ; il doit
  provenir de votre propre machine. »

## Conséquences

- `unsafe` nouveau : **un seul appel** (`MessageBoxW`, `emu-runtime::dialog`) ⇒ revue `reviewer`. `windows-sys`
  **0.61.2** (version déjà au `Cargo.lock`, aucune autre) devient une dépendance directe d'`emu-runtime` sous
  `cfg(windows)`. `tracing-subscriber` et `png` : dépendances d'`emu-ui`. `winresource` : dépendance de build
  d'`emu-app` sous Windows. `emu-app` devient une bibliothèque + deux binaires minces. Toutes ces lignes de
  `Cargo.toml` / `Cargo.lock` sont posées par la tâche de contrat (FX0), jamais par les tâches parallèles.
- Le paquet Windows contient deux exécutables ; `release.yml` vérifie `emu-rust-cli.exe --version` (code de retour
  compris) et le sous-système de chacun.
- Le rangement écrit dans le dossier de l'utilisateur : première fonction d'emu-rust qui déplace ses ROMs. Les
  garde-fous (aperçu, jamais d'écrasement ni de suppression de données, journal écrit avant d'agir, unité
  indivisible, annulation, reprise) sont le contrat ; `reviewer` relit le **format du journal avant** que
  l'exécution soit écrite, puis `emu-library/src/tidy/` comme du code sensible. Même règle pour le journal
  version 2 de la 8.0 : PV3 avant PB3, puis PV4 (amendement du 2026-11-27).
- Limites assumées : pas de rangement vers un autre volume ni sur un système de fichiers sans liens physiques
  (unité bloquée, à ranger à la main — à revoir si l'essai le demande) ; fichier en lecture seule ou dossier
  non inscriptible : unité laissée, jamais d'attribut ni de droit modifié (§4.8 RV3) ; `roms/<id>` lié ou monté ailleurs :
  bloqué ; une **seconde instance** d'emu-rust en train de jouer n'est pas exclue (le verrou n'exclut qu'un autre
  rangement) : ne pas ranger pendant qu'une autre fenêtre joue ; un état sauvegardé d'un core qui mémorise le
  chemin absolu du média (cores en `media_direct`) peut demander de réinsérer le disque après rangement — à
  mesurer par `qa-systems`, à écrire dans le guide d'essai ; sous Windows un fichier ouvert par un autre programme
  fait échouer son unité ; le lancement avec console n'est vérifié que par l'essai de l'utilisateur.
- Les jeux à la racine de `roms/` de l'utilisateur (ses consoles) gardent favoris, historique, jaquettes,
  sauvegardes, états et options après rangement, et après annulation.
- Feuille de route : lignes 7.5 (finition) et 7.6 (rangement) ; tableau des schémas : « 7.5, 7.6 : aucune migration ».
- **Amendement (presse-papiers, revue FV1b B1)** : sur demande explicite de l'utilisateur (copier/coller des messages
  d'erreur), la feature `clipboard` d'`egui-winit` est activée au niveau workspace (arboard ; smithay-clipboard sous
  Wayland ; clipboard-win et error-code sous Windows). Cela lève la restriction « pas de presse-papiers » d'ADR 0002.
  Licences : MIT / Apache-2.0, plus BSL-1.0 (permissive) ajoutée à `about.toml` ; aucun OpenSSL, aucune GPL.
