# ADR 0024 — Version 7.2 : Sega CD 32X (Mega CD + 32X)

Statut : **proposé** (2026-11-21, architecte). Décisions de l'utilisateur du 2026-11-21 intégrées
(`docs/questions-ouvertes.md`, « Réponses de l'utilisateur ») : **7.2 = Sega CD 32X seul** (glisser-déposer en
7.2.1, non conçu ici) ; jeux rangés **sous « Mega CD » avec une pastille « 32X »** (Q10) ; **marquage manuel « Ce
jeu est un CD 32X »** (Q11) ; **pré-release sur la base des tests factices**, l'essai réel de l'utilisateur décidant
de la suite (Q12). Étude préalable : `docs/essais-sega-cd-32x.md` (ses faits « constatés » / « lus » font foi).
Précise ADR 0013 (choix du core) et ADR 0020 §7 (« CD 32X : picodrive seulement, non testé ») ; réutilise sans les
changer ADR 0020 (système `megacd`, Redump, région, BIOS Mega CD par région, `.srm`) et ADR 0023 (rangement).
Conception : `docs/design/v7.2-sega-cd-32x.md`. **Graphe des crates inchangé. Aucun `unsafe`, aucun code réseau,
aucun core ni BIOS nouveau, aucune migration de schéma.**

## Contexte

- Six jeux (Corpse Killer, Fahrenheit, Night Trap, Slam City, Supreme Warrior, Surgical Strike). Ce sont des CD
  Mega CD : même sonde `SEGADISCSYSTEM`, même DAT Redump (40 entrées / 22 noms étiquetés `(Sega CD 32X)`,
  `(Mega-CD 32X)` ou `(32X CD)`), mêmes BIOS. Rien, sur ce qu'emu-rust lit aujourd'hui, ne les distingue.
- Seul **picodrive** les fait tourner ; la 32X s'y allume sans option quand le programme écrit ADEN (constaté avec
  un disque et un BIOS synthétiques). Le core par défaut du Mega CD, genesis_plus_gx, n'émule pas la 32X.
- Aujourd'hui un CD 32X est donc lancé avec genesis_plus_gx et ne fonctionne pas. Le travail est entièrement côté
  emu-rust : **reconnaître** ces jeux et **imposer picodrive**.
- Aucun CD 32X libre n'existe ; l'en-tête réel d'un tel disque (champ console, numéro de série) n'a pas pu être
  relevé. Le format et le nom du Night Trap 32X de l'utilisateur restent à relever (Q11).

## Décision

### 1. Reconnaissance : une marque « CD 32X » par jeu, dans le système `megacd`

Pas de système à part (Q10). Un jeu du système `megacd` porte la marque si **l'une** de ces conditions est vraie :

| # | Critère | Mécanisme |
|---|---|---|
| a | **Empreinte de la piste 01** (taille + CRC32) trouvée dans le DAT Redump, sur une entrée étiquetée | identification 6.0 inchangée : `game.title` prend le nom DAT, qui porte l'étiquette ⇒ cas (b) |
| b | **Étiquette du nom** : groupe entre parenthèses, casse et tiret indifférents, égal à `Sega CD 32X`, `Mega-CD 32X`, `Mega CD 32X`, `32X CD`, `CD 32X` ou `32X`, dans `game.title`, dans le nom du fichier `game.path` ou dans celui d'un des disques `game.media` | fonction pure `emu_library::cd32x_tag` ; seul critère possible pour un `.chd` (jamais lu) |
| c | **Marquage manuel** par l'utilisateur | §2 |

- La règle « un disque suffit » (titre, fichier **ou** un média) couvre les `.m3u` et les groupes `(Disc N)`.
- L'étiquette nue `(32X)` est acceptée parce que la fonction n'est appliquée qu'aux jeux `megacd` (une cartouche 32X
  est dans le système `32x`). Un faux positif est sans gravité : picodrive fait aussi tourner les CD Mega CD.
- **Numéro de série : indice seulement, jamais critère autonome en 7.2.** Il agit par l'identification Redump par
  numéro de série de la 6.0 (`Identification::Serial`), qui donne au jeu le nom DAT étiqueté. Le motif `T-1620xF`
  n'est pas codé : les numéros du DAT sont ceux des boîtes, le contenu réel de l'en-tête n'est pas vérifié, et
  Fahrenheit, Surgical Strike et les rééditions y échappent. À rouvrir après relevé de l'en-tête d'un vrai disque.
- **Pas de sonde d'en-tête nouvelle** (champ console « SEGA 32X ») : non vérifiée. Un test garantit seulement qu'un
  disque `SEGADISCSYSTEM` dont le champ console dirait `SEGA 32X` reste classé `megacd` (jamais cartouche `32x`).
- Pas de liste des six titres en dur : le marquage manuel est le filet.

### 2. Stockage de la marque : dérivation, **pas de migration**

- Marque automatique (a, b) : **dérivée à la lecture** de colonnes existantes (`game.title`, `game.path`,
  `game_media.path`) par `Game::cd32x_by_name`. Rien n'est stocké ; le schéma reste **v6**.
- Marque manuelle (c) : liste `cd32x` de la section `[library]` de `config.toml`, tenue par `emu-ui`. Clé = **nom du
  fichier de `game.path`** (avec extension, minuscules), jamais le chemin. Six jeux existent : la liste reste
  minuscule.
- Pourquoi c'est sûr sans colonne : (1) le rangement 7.1 **ne renomme jamais** (ADR 0023 §4.4) et un déplacement
  hors application garde le nom : la clé survit là où une clé par chemin se perdrait ; (2) un `config.toml` 7.2
  relu par une version antérieure conserve la clé inconnue (`extra`) ; (3) **le retour à la 7.1, à la 7.0 ou à la
  6.0 stable reste possible sur le même dossier de données** — une base v7 serait refusée (`NewerSchema`) par ces
  versions, alors que la 7.2 est une pré-release dont l'essai réel décide de la suite (Q12).
- Limites acceptées : renommer le fichier perd la marque manuelle (le renommage est alors l'occasion d'ajouter
  l'étiquette, qui rend la marque inutile) ; deux fichiers Mega CD de même nom dans deux dossiers partagent la
  marque. Une entrée dont le jeu a disparu est conservée (inoffensive).
- Une colonne `game.cd32x` (schéma v7) reste la solution de repli si une version ultérieure migre pour une autre
  raison ; elle n'est pas justifiée seule.

### 3. Choix du core : règle par marque dans `cores.toml`, picodrive imposé

```toml
[systems."Sega - Mega-CD - Sega CD".tags.cd32x]
cores = ["picodrive"]
```

- Nouvelle notion générique : une **marque de jeu** (`tags.<marque>`) restreint les cores candidats d'un système.
  `choose_core_tagged` : si le système a une règle pour la marque, **seuls** ses cores sont candidats (premier
  installé, sinon `Install(premier)`) ; jamais de repli sur la liste du système ni sur l'extension. Sans marque ou
  sans règle : comportement de `choose_core`, inchangé.
- La préférence de l'utilisateur « Core utilisé » (`[cores.preferred]`) **ne s'applique pas** aux jeux marqués ;
  l'écran Cores le dit en une ligne sous le Mega CD (« Les CD 32X utilisent toujours picodrive »).
- Tout le reste suit du core choisi, sans code nouveau : `media_direct`, table `required_firmware` de picodrive pour
  le Mega CD (déjà dans `cores.toml`), profil de touches picodrive, `.srm` de la RAM interne écrit par le runtime
  sous `saves/picodrive/`, interface disque et `.m3u`. `featured_options` de picodrive reçoit une ligne Mega CD
  (`picodrive_region`, `picodrive_drc`, `picodrive_ramcart`, `picodrive_aspect`) ; aucune option n'est forcée.

### 4. Pastille et marquage manuel (interface)

- **Pastille « 32X »** sur la vignette (coin de la jaquette, à l'opposé de l'étoile des favoris) pour tout jeu
  marqué, quelle que soit l'origine de la marque. Le jeu reste compté et filtré sous « Mega CD / Sega CD ».
- **Action « Ce jeu est un CD 32X »** sur le jeu sélectionné de la bibliothèque, proposée seulement pour un jeu
  `megacd` : bascule la marque manuelle, écrit `config.toml`, affiche un message (« … sera lancé avec picodrive »).
  Si la marque vient du nom, l'action est montrée cochée et inactive (« reconnu par son nom »). Elle est accessible
  au clavier **et** à la souris, sans hypothèse « clavier seulement ». La bibliothèque n'a pas encore de menu par
  jeu : son emplacement exact est laissé à l'agent `ui` (défaut consigné dans `docs/questions-ouvertes.md`).

### 5. picodrive absent, BIOS absent

- **picodrive non installé** : dialogue d'installation existant, avec la raison (« … est un CD 32X : il lui faut le
  core picodrive ; genesis_plus_gx n'émule pas la 32X. Installer picodrive ? »). Refus ou échec ⇒ pas de lancement.
  **Jamais de repli silencieux sur genesis_plus_gx**, ni de bouton « lancer quand même » avec lui.
- **BIOS Mega CD absent ou d'empreinte inconnue** : contrôle de firmware par région de la 6.0, inchangé, appliqué à
  picodrive (« BIOS manquant : bios_CD_U.bin (Mega CD / Sega CD, USA) », « Importer un BIOS… », « Lancer quand
  même »). Sans BIOS le core refuse le jeu (`Missing BIOS`, constaté) et le message d'erreur de core existant
  s'affiche. Rien n'est téléchargé (ADR 0005). Les BIOS 32X sont intégrés à picodrive : rien à fournir.

### 6. Fahrenheit

Disque 1 `(Key Disc) (Sega CD)`, disque 2 `(32X CD)` : les noms diffèrent par plus que le marqueur `(Disc N)`, donc
`group_media` **ne les regroupe pas**. Décision : **aucune règle spéciale en 7.2**.

- Deux entrées : le disque 1 est un jeu Mega CD ordinaire (core préféré du Mega CD) ; le disque 2 porte la marque
  et part sur picodrive.
- Si le jeu 32X exige d'insérer d'abord le disque clé (non vérifié) : l'utilisateur écrit un `.m3u` listant les
  deux disques — la règle « un disque suffit » (§1) le marque et le lance avec picodrive — ou ajoute le disque par
  le menu « CD » en cours de jeu (2.1). Le guide d'essai le dit.
- Un regroupement automatique « disque clé + disque 32X » n'est conçu que si un essai réel en montre le besoin.

### 7. Essais et publication

Tests automatiques : classement et marque (sans core), choix du core, dialogues ; test de fumée `#[ignore]` avec le
vrai picodrive, un disque de `tests/tools/mkcd.py` et un **BIOS synthétique** qui écrit ADEN — écrit par le test
dans un dossier temporaire, jamais distribué ni placé dans `system/` : la ligne `32X startup` du journal du core
prouve le routage et l'allumage de la 32X, **pas** l'émulation d'un jeu. Pré-release `v7.2.0-rc.1` sur cette base
(Q12) ; le jeu réel (Night Trap 32X) est validé par l'utilisateur.

## Interaction avec la 7.1 (ADR 0023)

- **Système de destination** : un CD 32X est une unité `megacd` comme tout CD Mega CD (sonde d'en-tête, empreinte
  ou nom Redump) ; aucun dossier `megacd32x`. Rien à changer au rangement.
- **Unités de CD** : `.cue` + pistes ⇒ `roms/megacd/<nom>/` ; groupe `(Disc N)` entier dans un seul dossier ;
  Fahrenheit = deux unités, deux dossiers (un `.m3u` de l'utilisateur qui cite les deux en fait une seule unité).
- **Invariant utilisé** : « le rangement ne renomme jamais » (ADR 0023 §4.4) porte la clé de la marque manuelle
  (§2). Le changer exigerait de réécrire la liste `[library] cd32x` dans la même opération.
- La 7.2 est développée sur `wip-7.2`, créée depuis `wip-7.1` une fois la 7.1 livrée ; elle n'utilise aucune API du
  rangement. Fichiers touchés par les deux versions : `emu-ui/src/config_library.rs`, `library/**`,
  `emu-assets/src/cores/select.rs` (ajouts seulement).

## Conséquences

- Interfaces ajoutées (`docs/design/v7.2-sega-cd-32x.md` §2) : `emu_library::cd32x_tag`, `Game::cd32x_by_name` ;
  `emu_assets::cores::{GAME_TAG_CD32X, CorePreferences::for_tag, choose_core_tagged}` ; `LibraryConfig::cd32x`.
  `emu-runtime`, `emu-libretro`, `emu-app` : inchangés.
- Un `config.toml` et une base de la 7.1 sont repris sans changement ; les jeux Mega CD non marqués se comportent
  comme en 6.0 / 7.1.
- Les deux éditions d'un même titre (Mega CD seul, CD 32X) sont deux jeux distincts aux sauvegardes distinctes
  (`.brm` de genesis_plus_gx, `.srm` de picodrive) : sans objet entre eux.
- **Risque principal** : compatibilité réelle des six jeux avec picodrive (fonction ancienne, peu exercée),
  inconnue avant l'essai de l'utilisateur ; en cas d'échec, la 7.2 ne laisse aucune trace irréversible (pas de
  migration) et la marque peut rester comme simple information.
- Points ouverts consignés dans `docs/questions-ouvertes.md` (lignes 7.2) : emplacement de l'action de marquage,
  Fahrenheit, numéro de série, format du Night Trap de l'utilisateur.
