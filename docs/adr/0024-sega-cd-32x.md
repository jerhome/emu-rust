# ADR 0024 — Sega CD 32X (Mega CD + 32X) : compléments PC (7.2 PC → 7.5)

Statut : **accepté, amendé pour la fusion le 2026-11-25** : l'« Amendement de fusion » ci-dessous **prime** ; les §2
et §3 sont **remplacés** par le socle de la 7.4 du Mac. Historique : accepté le 2026-11-21 (architecte ; proposé le même jour, amendé après la revue indépendante — borne de
CRC, suppression de la tâche de contrat, clé de la marque manuelle, jeux non regroupés, règle `cd32x_cores` — et
après deux précisions de l'utilisateur : son Night Trap 32X est en **deux `.chd`** ; clic droit validé, avec une
**invitation** à s'en servir). Décisions de l'utilisateur du 2026-11-21 (`docs/questions-ouvertes.md`, « Réponses de
l'utilisateur ») : **7.2 = Sega CD 32X seul** (glisser-déposer en 7.2.1, non conçu ici) ; jeux rangés **sous « Mega
CD » avec une pastille « 32X »** (Q10) ; **marquage manuel « Ce jeu est un CD 32X »** (Q11) ; **pré-release sur la
base des tests factices**, l'essai réel décidant de la suite (Q12). Étude préalable :
`docs/essais-sega-cd-32x.md`. Précise ADR 0013 (choix du core) et ADR 0020 §7 (« CD 32X : picodrive seulement, non
testé ») ; réutilise ADR 0020 (système `megacd`, Redump, région, BIOS Mega CD par région, `.srm`) et ADR 0023
(rangement). Conception : `docs/design/v7.2-sega-cd-32x.md`. **Graphe des crates inchangé. Aucun `unsafe`, aucun
code réseau, aucun core ni BIOS nouveau ; schéma v7 de la 7.4 inchangé.**

## Amendement de fusion (2026-11-25) — socle Mac 7.4 + compléments PC, livrés en 7.5

La 7.4 du Mac (`main`, `docs/design/v7.4-sega-cd-32x.md`) a livré un CD 32X sur un autre socle que celui décrit ici :
sonde du programme de démarrage `32X Initial Program` (`disc_needs_32x`), repli sur le nom Redump
(`name_needs_32x`), colonne `game.needs_32x` (**schéma v7**), choix du core par `choose_game_core` /
`GameNeeds` (cores de la liste « Sega - 32X »). Décision de l'utilisateur D4 : **le socle Mac est gardé**, les
compléments PC s'y ajoutent. Contrat : `docs/design/v7.5-finition-pc.md` §2-§4.

- **Abandonnés** (PC) : `cd32x_cores` de `cores.toml`, `CorePreferences::cd32x_cores`, `choose_cd32x_core`,
  `Game::cd32x_by_name`, `cd32x_tag` (ses formes passent dans `name_needs_32x`), le paramètre `cd32x: bool` de
  `plan_launch`. Le §2 (« pas de migration », schéma v6) et le §3 (`cd32x_cores`) sont **remplacés**.
- **Gardés** (PC, ajoutés au socle) : marque manuelle `[library] cd32x` (§2 : clé = nom du fichier en minuscules,
  collision comptée, échec d'écriture annulé en mémoire) ; pastille, action « Ce jeu est un CD 32X » au **clic droit**
  et à la touche **X**, dans un menu unique avec « Réinitialiser la jaquette » (touche **R**, 7.3), invitation dans la
  ligne d'aide (§4) ; message « picodrive requis, jamais de repli sur genesis_plus_gx » (§5) ; jeux à plusieurs
  disques (§6) ; tests `cd32x_scan.rs`, `cd32x_config.rs`, `cd32x_real.rs` adaptés ; `tests/tools/mkcd.py --variant
  32x` écrit la signature `32X Initial Program` (§7).
- **Marque effective** = `game.needs_32x` (sonde ou nom, posée par le scan) **ou** marque manuelle ; calculée par
  `emu-ui` (`effective_needs_32x`) et passée à `GameNeeds::needs_32x` partout.
- **Étiquettes (§1 b)** : `Sega CD 32X`, `Mega-CD 32X`, `Mega CD 32X`, `32X CD`, `CD 32X` ; tiret et espace
  équivalents, casse ignorée. **L'étiquette nue `(32X)` est refusée** (D9) : un faux positif enverrait un jeu Mega CD
  vers picodrive sans que l'utilisateur l'ait demandé. Le scan réévalue la règle du nom (sans lecture) sur les lignes
  déjà marquées « non 32X ».
- **Borne de CRC (§1 a)** : reste **512 Mio** (D8) ; les quatre disques au-delà sont reconnus par la sonde
  `32X Initial Program` (`.cue` / `.iso`), par leur nom, ou marqués à la main.
- **BIOS (§5)** : « Les BIOS 32X sont intégrés à picodrive » est faux : picodrive **n'intègre aucun BIOS Sega** et
  génère son propre code de démarrage (GV1, ADR 0020 réécrite). Seul le BIOS Mega CD de l'utilisateur est requis.
- **Interaction avec le rangement** : le rangement passe en 7.6 ; l'invariant « ne renomme jamais » porte toujours la
  clé de la marque manuelle ; `relocate` conserve `game.needs_32x` (ADR 0023 §4.7 amendé).

Effet sur chaque système livré : seul `megacd` change (formes d'étiquette, marque manuelle, menu clic droit) ; les
jeux Mega CD non marqués se lancent comme en 7.4. Les autres systèmes : rien (tableau complet : design 7.5 §10).

## Contexte

- Six jeux (Corpse Killer, Fahrenheit, Night Trap, Slam City, Supreme Warrior, Surgical Strike). Ce sont des CD
  Mega CD : même sonde `SEGADISCSYSTEM`, même DAT Redump (40 entrées / 22 noms étiquetés `(Sega CD 32X)`,
  `(Mega-CD 32X)` ou `(32X CD)`), mêmes BIOS. Rien, sur ce qu'emu-rust lit aujourd'hui, ne les distingue.
- Seul **picodrive** les fait tourner ; la 32X s'y allume sans option quand le programme écrit ADEN (constaté avec
  un disque et un BIOS synthétiques). Le core par défaut du Mega CD, genesis_plus_gx, n'émule pas la 32X :
  aujourd'hui un CD 32X est lancé avec lui et ne fonctionne pas.
- Ce que le scan sait faire (lu dans le code, `emu-library/src/scan.rs`, `db.rs`) :
  - un **`.chd` n'est jamais lu** (ni en-tête, ni empreinte) : il n'est identifié que si son nom est un nom du DAT
    Redump, et hors de `roms/megacd/` il n'est classé `megacd` qu'à cette même condition ;
  - un `.cue` / `.iso` est identifié par la taille + le CRC32 de la piste 01, **jusqu'à 512 Mio** (`crc_limit`) ;
  - l'identification par numéro de série rend `None` quand deux jeux du DAT partagent le numéro (`by_serial`).
- D'après la revue indépendante (lecture du DAT ; non re-vérifié ici) : les pistes 01 de Night Trap disque 1
  (617 Mo), Corpse Killer, Slam City et Fahrenheit disque 2 dépassent 512 Mio ; les deux disques de Night Trap
  partagent `T-16202F` ; les noms DAT de Slam City (4 disques) et de Supreme Warrior (USA) diffèrent par plus que le
  marqueur `(Disc N)`.
- Le Night Trap 32X de l'utilisateur : **deux disques en `.chd`**, nom exact pas encore connu. Pour lui la
  reconnaissance repose donc sur **le nom du fichier, sinon sur le marquage manuel**.
- Aucun CD 32X libre n'existe ; l'en-tête réel d'un tel disque n'a pas pu être relevé.

## Décision

### 1. Reconnaissance : une marque « CD 32X » par jeu, dans le système `megacd`

Pas de système à part (Q10). Un jeu du système `megacd` porte la marque si **l'une** de ces conditions est vraie :

| # | Critère | Mécanisme |
|---|---|---|
| a | **Empreinte de la piste 01** (taille + CRC32) trouvée dans le DAT Redump, sur une entrée étiquetée | identification 6.0 : `game.title` prend le nom DAT, qui porte l'étiquette ⇒ cas (b). `.cue` / `.iso` seulement. **La borne de CRC du Mega CD passe de 512 à 800 Mio** (comme la 3DO), sans quoi quatre des disques ne sont jamais identifiés. *(Remplacé par l'amendement de fusion (D8 : 512 Mio) ; ces disques sont reconnus par la sonde `32X Initial Program`.)* |
| b | **Étiquette du nom** : groupe entre parenthèses, casse et tiret indifférents, égal à `Sega CD 32X`, `Mega-CD 32X`, `Mega CD 32X`, `32X CD`, `CD 32X` ou `32X` *(Remplacé par l'amendement de fusion (D9 : `(32X)` refusé))*, dans `game.title`, dans le nom du fichier `game.path` ou dans celui d'un des disques `game.media` | fonction pure `emu_library::cd32x_tag` ; seul critère automatique pour un `.chd` *(Remplacé par l'amendement de fusion : `name_needs_32x`)* |
| c | **Marquage manuel** par l'utilisateur | §2, §4 |

- La règle « un disque suffit » (titre, fichier **ou** un média) couvre les `.m3u` et les groupes `(Disc N)`.
- L'étiquette nue `(32X)` est acceptée parce que la fonction n'est appliquée qu'aux jeux `megacd`. Un faux positif
  est sans gravité : picodrive fait aussi tourner les CD Mega CD. *(Remplacé par l'amendement de fusion (D9 :
  refusé).)*
- **Faux négatifs connus, renvoyés au marquage manuel** : étiquette entre crochets (TOSEC `[32X]`), nom sans
  étiquette (un `.chd` nommé comme l'édition Mega CD est même *identifié* par son nom comme l'édition Mega CD),
  `.cue` dont la piste dépasse 800 Mio ou n'est pas au DAT. *(Remplacé par l'amendement de fusion (D8 : 512 Mio) ;
  la sonde `32X Initial Program` reconnaît aussi ces `.cue` / `.iso`.)*
- **Ligne déjà scannée** : une ligne inchangée n'est pas ré-analysée par le scan. Un `.cue` scanné avant la 7.2 ne
  bénéficie de la nouvelle borne qu'après un **rescan complet** (ou le marquage manuel) ; le guide d'essai le dit.
  *(Remplacé par l'amendement de fusion : pas de nouvelle borne (D8) ; le scan réévalue la règle du nom sur les
  lignes déjà marquées « non 32X ».)*
- **Numéro de série : aucun rôle en 7.2.** `by_serial` ne tranche pas quand deux disques partagent le numéro (cas
  de Night Trap), et le motif `T-1620xF` n'est pas codé (numéros de boîte, en-tête réel non vérifié, trois jeux y
  échappent). À rouvrir après relevé de l'en-tête d'un vrai disque.
- **Pas de sonde d'en-tête nouvelle** (champ console « SEGA 32X ») : non vérifiée. Un test unitaire fixe seulement
  l'ordre de `bin_kind` : une piste `SEGADISCSYSTEM` dont le champ à `0x100` dirait `SEGA 32X` reste une piste
  Mega CD, jamais une cartouche `32x`. *(Remplacé par l'amendement de fusion : sonde du programme de démarrage
  `32X Initial Program` (`disc_needs_32x`) de la 7.4.)*
- Pas de liste des six titres en dur : le marquage manuel est le filet.

### 2. Stockage de la marque : dérivation, **pas de migration**

- Marque automatique (a, b) : **dérivée à la lecture** de colonnes existantes (`game.title`, `game.path`,
  `game_media.path`) par `Game::cd32x_by_name`. Rien n'est stocké ; le schéma reste **v6**. *(Remplacé : colonne
  `game.needs_32x`, schéma v7, posée par la 7.4 ; voir l'amendement de fusion.)*
- Marque manuelle (c) : liste `cd32x` de la section `[library]` de `config.toml`, tenue par `emu-ui`. Clé = **nom du
  fichier de `game.path`** (avec extension), en **minuscules Unicode** ; la comparaison normalise les deux côtés,
  donc une entrée écrite à la main en majuscules fonctionne. Six jeux existent : la liste reste minuscule.
- Pourquoi c'est sûr sans colonne : (1) le rangement 7.1 **ne renomme jamais** (ADR 0023 §4.4) et un déplacement
  hors application garde le nom : la clé survit là où une clé par chemin se perdrait ; (2) un `config.toml` 7.2
  relu par une version antérieure conserve la clé inconnue (`extra`) ; (3) **le retour à la 7.1, à la 7.0 ou à la
  6.0 stable reste possible sur le même dossier de données** — une base v7 serait refusée (`NewerSchema`) par ces
  versions, alors que la 7.2 est une pré-release dont l'essai réel décide de la suite (Q12). *(Remplacé : le
  schéma v7 de la 7.4 est gardé ; voir l'amendement de fusion.)*
- **Collision de clé** (rips nommés `image.cue`, `disc1.cue`…) : la clé est gardée, mais au marquage emu-rust
  **compte les jeux `megacd` de même clé** et le dit dans le message (« 3 jeux Mega CD portent ce nom de fichier :
  tous seront lancés avec picodrive »).
- Limites acceptées : renommer le fichier perd la marque manuelle (c'est alors l'occasion d'ajouter l'étiquette) ;
  une entrée dont le jeu a disparu est conservée (inoffensive).
- Si l'écriture de `config.toml` échoue (fichier verrouillé, disque) : la marque est **annulée en mémoire** et
  l'échec affiché — jamais de marque qui disparaîtrait au redémarrage sans le dire.
- Une colonne `game.cd32x` (schéma v7) reste la solution de repli si une version ultérieure migre pour une autre
  raison ; elle n'est pas justifiée seule. *(Remplacé : colonne `game.needs_32x`, schéma v7 de la 7.4.)*

### 3. Choix du core : picodrive imposé par `cd32x_cores`

*(Remplacé par l'amendement de fusion : `choose_game_core` / `GameNeeds::needs_32x`, cores de la liste
« Sega - 32X » ; `cd32x_cores` et `choose_cd32x_core` abandonnés.)*

```toml
[systems."Sega - Mega-CD - Sega CD"]
cores = ["genesis_plus_gx", "picodrive"]
cd32x_cores = ["picodrive"]
```

- Une clé unique, pas de mécanisme générique de marques (un seul cas d'usage). `choose_cd32x_core` : premier core
  de la liste installé, sinon `Install(premier)` ; **jamais** de repli sur la liste `cores` du système ni sur
  l'extension. `choose_core` n'est pas modifié ; un jeu non marqué se lance comme en 7.1.
- La préférence « Core utilisé » (`[cores.preferred]`) ne s'applique pas aux jeux marqués.
- Tout le reste suit du core choisi, sans code nouveau : `media_direct`, table `required_firmware` de picodrive pour
  le Mega CD (déjà dans `cores.toml`), profil de touches picodrive, `.srm` de la RAM interne écrit par le runtime
  sous `saves/picodrive/`, interface disque et `.m3u`. Aucune option de core n'est forcée ni mise en avant.

### 4. Pastille, marquage manuel et invitation (interface)

- **Pastille « 32X »** sur la vignette (coin de la jaquette, à l'opposé de l'étoile des favoris) pour tout jeu
  marqué, quelle que soit l'origine de la marque. Le jeu reste compté et filtré sous « Mega CD / Sega CD ».
- **Action « Ce jeu est un CD 32X »** : **clic droit** sur la vignette (validé par l'utilisateur), doublé d'une
  touche pour le jeu sélectionné (pas d'hypothèse « souris seulement » ; touche choisie par l'agent `ui`).
  Proposée pour un jeu `megacd` que son nom ne marque pas déjà ; elle bascule la marque manuelle (« Ce n'est pas un
  CD 32X » pour la retirer). Un jeu marqué par son nom n'a pas d'action : la pastille suffit.
- **Message après marquage** : le jeu sera lancé avec **picodrive** et non plus avec le core du Mega CD ; ses
  sauvegardes faites avec genesis_plus_gx (`.brm`) ne sont pas reprises ; nombre de jeux de même nom de fichier
  s'il y en a plusieurs (§2) ; « si le jeu a d'autres disques listés à part, marquez-les aussi ».
- **Invitation** (demande de l'utilisateur) : quand le jeu **sélectionné** est un jeu `megacd` non marqué dont
  l'identification n'est **pas fondée sur le contenu** (`Unknown` ou `Name` — donc tout `.chd`), la ligne d'aide de
  la bibliothèque affiche en plus : « Jeu Sega CD 32X ? Clic droit → Ce jeu est un CD 32X ». Elle disparaît dès que
  le jeu est marqué ou identifié par son empreinte. Justification du choix :
  - elle apparaît **au moment et à l'endroit utiles** (jeu sélectionné, juste avant de le lancer), au clavier
    comme à la souris, à la différence d'une infobulle ;
  - **aucun état à mémoriser** et rien à refermer, à la différence d'un bandeau « une fois » — que verraient aussi
    les utilisateurs sans CD 32X et qui serait oublié le jour où le jeu arrive ;
  - **aucune heuristique**, à la différence d'un message après un « arrêt rapide » avec genesis_plus_gx : on ne
    sait pas comment ce core se comporte avec un CD 32X (écran noir ? image fausse ?), et l'utilisateur aurait
    déjà subi l'échec.
- Un `.chd` posé **hors de `roms/megacd/`** et sans nom Redump n'est pas classé, donc pas marquable : le guide dit
  de ranger ces fichiers sous `megacd/`.

### 5. picodrive absent, BIOS absent

- **picodrive non installé** : dialogue d'installation existant, avec la raison (« … est un CD 32X : il lui faut le
  core picodrive ; genesis_plus_gx n'émule pas la 32X. Installer picodrive ? »). Refus ou échec ⇒ pas de lancement.
  **Jamais de repli silencieux sur genesis_plus_gx**, ni de bouton « lancer quand même » avec lui.
- **BIOS Mega CD absent ou d'empreinte inconnue** : contrôle de firmware par région de la 6.0, inchangé, appliqué à
  picodrive (« BIOS manquant : bios_CD_U.bin (Mega CD / Sega CD, USA) », « Importer un BIOS… », « Lancer quand
  même »). Sans BIOS le core refuse le jeu (`Missing BIOS`, constaté). Rien n'est téléchargé (ADR 0005). Pour la
  32X, picodrive n'a besoin d'aucun BIOS (il n'en intègre aucun de Sega : GV1, ADR 0020).
- Comme en 6.0 : la région d'un `.chd` ne vient que de son nom (ou de la ligne Redump) ; un `.chd` Europe avec le
  seul BIOS USA finit en `Missing BIOS`. Le guide le rappelle.

### 6. Jeux à plusieurs disques

`group_media` ne regroupe que des fichiers d'un même dossier dont les noms ne diffèrent que par le marqueur
`(Disc N)`. Aucune règle spéciale n'est ajoutée en 7.2.

| Cas | Résultat | Marque |
|---|---|---|
| Noms identiques au marqueur près (Night Trap, Supreme Warrior Europe ; `.cue` ou `.chd`) | **un seul jeu**, disques dans `game.media`, menu « CD » | une étiquette sur un disque, ou **une** marque manuelle, suffit |
| Noms DAT qui diffèrent davantage (**Slam City**, 4 disques ; **Supreme Warrior USA**) | une entrée **par disque** | chaque entrée porte son étiquette (nom DAT) ; sinon marquer chaque disque |
| **Fahrenheit** : disque 1 `(Key Disc) (Sega CD)`, disque 2 `(32X CD)` | deux entrées ; seul le disque 2 est étiqueté | pour partir du disque clé sous picodrive (ordre réel non vérifié) : **marquer à la main le disque 1** |
| Fichiers nommés sans marqueur reconnu (`…CD1.chd`, `…CD2.chd`) | une entrée par disque | marquer chaque disque, ou renommer en `(Disc 1)` / `(Disc 2)` |

Dans les trois derniers cas, les disques se suivent par le menu « CD » (ajout d'un disque en cours de jeu, 2.1) ou
par un **`.m3u`** écrit par l'utilisateur, qui en fait un seul jeu (marqué si un disque l'est, §1). Le guide
d'essai liste ces titres. Pour le Night Trap de l'utilisateur (deux `.chd`) : nommés au marqueur près, ils forment
un seul jeu et une seule marque ; sinon l'invitation (§4) s'affiche sur chacun des deux.

### 7. Essais et publication

- Sans core : étiquette, marque dérivée, borne de CRC, `choose_cd32x_core`, marque manuelle, et **le routage** —
  c'est le test d'interface (jeu marqué ⇒ picodrive demandé ou installé, contrôle du BIOS sur picodrive) qui le
  prouve. *(Remplacé par l'amendement de fusion : `name_needs_32x`, sonde, `choose_game_core` ; borne inchangée.)*
- Un seul test `#[ignore]` avec le vrai picodrive : disque de `tests/tools/mkcd.py` + **BIOS synthétique** qui écrit
  ADEN, écrit par le test dans un dossier temporaire (jamais distribué, jamais dans `system/`). La ligne
  `32X startup` prouve que picodrive, chargé par emu-rust avec un CD, allume la 32X — **ni le routage, ni
  l'émulation d'un jeu**. Sans le core dans `tests/cores/`, le test se saute proprement.
- Pré-release `v7.2.0-rc.1` sur cette base (Q12) ; le jeu réel (Night Trap 32X) est validé par l'utilisateur.

## Interaction avec la 7.1 (ADR 0023)

- **Système de destination** : un CD 32X est une unité `megacd` comme tout CD Mega CD ; aucun dossier `megacd32x`.
  Un `.chd` sans nom Redump est « ambigu » pour le rangement comme tout `.chd` : l'utilisateur choisit Mega CD.
- **Unités de CD** : `.cue` + pistes ⇒ `roms/megacd/<nom>/` ; un groupe `(Disc N)` reconnu va entier dans un seul
  dossier ; les disques non regroupés (§6 : Slam City, Supreme Warrior USA, Fahrenheit) sont des unités séparées,
  un dossier chacune (`.chd` : à plat) ; un `.m3u` qui les cite en fait une seule unité.
- **Invariant utilisé** : « le rangement ne renomme jamais » (ADR 0023 §4.4) porte la clé de la marque manuelle
  (§2). Le changer exigerait de réécrire la liste `[library] cd32x` dans la même opération.
- La 7.2 est développée sur `wip-7.2`, créée depuis `wip-7.1` une fois la 7.1 livrée ; elle n'utilise aucune API du
  rangement. Fichiers touchés par les deux versions : `emu-ui/src/config_library.rs`, `library/**`,
  `emu-assets/src/cores/select.rs`, `emu-library/src/scan.rs` (ajouts ou une constante).

## Conséquences

- Interfaces ajoutées (`docs/design/v7.2-sega-cd-32x.md` §2) : `emu_library::cd32x_tag`, `Game::cd32x_by_name` ;
  `emu_assets::cores::{CorePreferences::cd32x_cores, choose_cd32x_core}` ; `LibraryConfig::cd32x`.
  `emu-runtime`, `emu-libretro`, `emu-app` : inchangés. *(Remplacé par l'amendement de fusion : ces interfaces
  sont abandonnées au profit de `name_needs_32x`, `choose_game_core` et `effective_needs_32x` ; `LibraryConfig::cd32x`
  gardé.)*
- Le scan lit jusqu'à 800 Mio d'une piste 01 Mega CD **quand le DAT liste sa taille** (une fois par fichier).
  *(Remplacé par l'amendement de fusion (D8 : 512 Mio).)*
- Un `config.toml` et une base de la 7.1 sont repris sans changement ; les jeux Mega CD non marqués se comportent
  comme en 6.0 / 7.1. *(Remplacé : la base passe en schéma v7 depuis la 7.4 ; les jeux non marqués se lancent comme
  en 7.4.)*
- Les deux éditions d'un même titre (Mega CD seul, CD 32X) sont deux jeux distincts ; marquer à la main un jeu déjà
  joué avec genesis_plus_gx le fait repartir sans ses `.brm` (dit dans le message).
- **Risque principal** : compatibilité réelle des six jeux avec picodrive (fonction ancienne, peu exercée),
  inconnue avant l'essai de l'utilisateur ; en cas d'échec, la 7.2 ne laisse aucune trace irréversible.
- Points ouverts consignés dans `docs/questions-ouvertes.md` (lignes 7.2) : nom exact des fichiers de
  l'utilisateur, Fahrenheit, numéro de série.
