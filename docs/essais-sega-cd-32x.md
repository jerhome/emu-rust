# Sega CD 32X (Mega CD + 32X) — étude de faisabilité pour la version 7.2

Date : 2026-11-21. Périmètre : `docs/specs/v7.1-v8.0-utilisateur.md` §7.2 (« Mega CD + 32X », ex. Night Trap 32X ;
pas urgent). **Étude seulement** : aucune ADR, aucun code, aucun découpage définitif. Suite de
`docs/essais-gba-32x-megacd.md` et d'ADR 0020 §7 (« CD 32X : picodrive seulement, non testé »).

Ce qui est marqué **constaté** a été mesuré ici ; **lu** = relevé dans une source (code du core, DAT) ;
**non vérifié** = connaissance générale, à confirmer avec un vrai disque (essai de l'utilisateur).

## 1. Résumé

- **Faisable, petit chantier.** Un seul core libretro fait tourner les CD 32X : **picodrive**, déjà installé pour
  la 32X (6.0) et déjà déclaré en alternative pour le Mega CD. Aucun core nouveau, aucun BIOS nouveau (les BIOS
  Mega CD de la 6.0 suffisent ; les BIOS 32X sont intégrés au core), aucun `unsafe`, aucun code réseau.
- **Le core ne « détecte » rien** : un CD 32X démarre comme un CD Mega CD ordinaire ; c'est le programme du disque
  qui allume la 32X en cours de route. Tout le travail est donc **côté emu-rust** : reconnaître ces six jeux et les
  envoyer à picodrive au lieu de genesis_plus_gx (qui n'émule pas la 32X).
- **Limite de test** : aucun CD 32X libre n'existe, et tout CD Mega CD exige le BIOS commercial. Un test
  automatique reste possible avec un **BIOS synthétique** écrit par un script (essai fait, §5). Le jeu réel
  (Night Trap 32X) ne peut être validé que par l'utilisateur.

## 2. Le matériel et la ludothèque

Le « Sega CD 32X » (Europe : « Mega-CD 32X ») n'est pas une machine : c'est une Mega Drive portant **à la fois** le
lecteur Mega CD et l'extension 32X. Le disque est un CD Mega CD ; son programme utilise en plus les deux SH-2 et la
vidéo de la 32X (meilleure vidéo plein écran, plus de couleurs).

Six jeux seulement, tous des films interactifs, tous présents dans le DAT Redump déjà utilisé depuis la 6.0
(`metadat/redump/Sega - Mega-CD - Sega CD.dat` de libretro-database, **lu** le 2026-11-21 : 534 entrées, 440 noms
distincts, dont **40 entrées / 22 noms** portant une étiquette 32X) :

| Jeu | Régions au DAT | Disques | Étiquette du nom |
|---|---|---|---|
| Corpse Killer | Europe, USA | 1 | `(Mega-CD 32X)`, `(Sega CD 32X)` |
| Fahrenheit | USA | 2 : disque 1 `(Key Disc) (Sega CD)`, disque 2 `(32X CD)` | `(32X CD)` |
| Night Trap | Europe, USA (RE-1, Limited Run Games) | 2 | `(Mega-CD 32X)`, `(Sega CD 32X)` |
| Slam City with Scottie Pippen | Europe, USA | 4 | idem |
| Supreme Warrior | Europe, USA | 2 | idem |
| Surgical Strike | Brésil | 1 | `(Sega CD 32X)` |

Il n'existe **pas** de DAT ni de base libretro séparée (`metadat/redump/` : Dreamcast, Mega-CD - Sega CD, Naomi,
Naomi 2, Saturn pour Sega) ; le DAT No-Intro « Sega - 32X » (cartouches) ne contient aucun de ces titres. Pour
libretro, **ce sont des jeux Mega CD** ; l'étiquette du nom est la seule marque.

## 3. Cores libretro

| Core | Mega CD | 32X | CD 32X |
|---|---|---|---|
| **picodrive** (`2.05-1890c29`, buildbot ; licence MAME non commerciale) | oui | oui | **oui** |
| genesis_plus_gx (core Mega CD par défaut d'emu-rust) | oui | **non** | non |
| genesis_plus_gx_wide | oui | non | non |
| blastem (libretro) | non | non | non |
| clownmdemu | oui (BIOS de remplacement intégré) | non | non |

Lignes picodrive et genesis_plus_gx : **lues** / **constatées** (6.0 et ci-dessous) ; les trois autres :
connaissance générale, **non vérifiées** ici, sans incidence sur la conclusion (aucun autre core libretro n'émule
la 32X). ares gère le « Mega CD 32X » comme un système à part, mais n'est pas un core libretro ; il n'apporte ici
que des indications sur les en-têtes (§4).

picodrive, faits **lus** dans la source (`libretro/picodrive`, commit `1890c29` du 2026-09-26, celui du binaire de
test) :

- `ChangeLog` 1.90 (2013) : « 32X+CD emulation has been implemented » ; 2.00 : compteurs de cycles MCD / 32X
  repris dans les sauvegardes d'état. Fonction ancienne, peu exercée (six jeux).
- La 32X est **toujours autorisée** dans la version libretro (`POPT_EN_32X` posé sans option, `libretro.c`) :
  **aucune option de core à régler**. Elle s'allume quand le programme écrit le bit ADEN du registre `$A15100`
  (`pico/32x/memory.c`, « before ADEN » → `Pico32xStartup()`), que le support soit une cartouche ou un CD ; les
  accès Mega CD restent servis quand la 32X est allumée (`PicoRead8_mcd_io`, `PicoWrite8_32x_on_io_cd`).
- Chargement d'un CD : `PicoCdCheck` cherche l'en-tête Mega CD et lit la région (octet `0x20B` : `0x64` Europe,
  `0xA1` Japon, sinon USA — même règle que genesis_plus_gx), puis `find_bios` ouvre dans le dossier système
  `bios_CD_U.bin` / `bios_CD_E.bin` / `bios_CD_J.bin` (ou les noms `us_scd2_9306`, `eu_mcd2_9306`,
  `jp_mcd2_921222`…). **Rien, au chargement, ne distingue un CD 32X.**
- BIOS : **Mega CD requis** (les trois fichiers de la 6.0, ADR 0020 §2 ; la table `required_firmware` de picodrive
  pour le Mega CD existe déjà dans `cores.toml`). BIOS 32X : intégrés au core, rien à fournir (6.0).
- Sauvegardes : la RAM interne du Mega CD (8 Ko) est exposée par `RETRO_MEMORY_SAVE_RAM` ⇒ **`.srm` écrit par le
  runtime**, comme pour une cartouche (différent de genesis_plus_gx, qui écrit lui-même ses `.brm`). Avec
  `picodrive_ramcart` activé c'est la cartouche RAM qui est exposée à la place. État sauvegardé : taille maximale
  annoncée en incluant toujours la 32X (`retro_serialize_size`), donc pas de surprise de taille en cours de jeu.
- Disques multiples : interface disque libretro et `.m3u` (comme en 6.0).
- Options utiles (déjà listées en 6.0 pour la 32X) : `picodrive_region` (Auto), `picodrive_drc` (recompilateur
  SH-2, activé), `picodrive_ramcart`, `picodrive_aspect`. Aucune option « 32X » ni « CD 32X ».

## 4. Reconnaître un CD 32X dans la bibliothèque

Aujourd'hui (6.0 / 7.0) un CD 32X est rangé dans le système `megacd` (sonde `SEGADISCSYSTEM`, DAT Redump) et lancé
avec le core préféré du Mega CD, **genesis_plus_gx**, qui ne peut pas le faire tourner. Il faut un critère :

| Critère | Fiabilité | Remarque |
|---|---|---|
| **Empreinte de la piste 01** dans le DAT Redump (taille + CRC32, mécanisme 6.0) dont le nom porte `(Sega CD 32X)`, `(Mega-CD 32X)` ou `(32X CD)` | sûre | `.cue` / `.iso` seulement ; un `.chd` n'est pas lu |
| **Étiquette dans le nom du fichier** (mêmes trois étiquettes, noms Redump) | bonne | seul critère pour un `.chd` ; dépend du nommage de l'utilisateur |
| **Numéro de série** | partielle | **lu** au DAT : tous les numéros `T-1620xF` (suffixe F : 34 entrées) appartiennent à des entrées 32X, aucune exception ; mais Fahrenheit (`4438`, partagé avec le disque Mega CD), Surgical Strike (`4435`, `157036`) et les rééditions Limited Run Games (sans numéro) y échappent. Les numéros du DAT sont ceux de la boîte ; l'en-tête du disque porterait `GM T-16201F-00` pour Corpse Killer (commentaire de la source d'ares) |
| **En-tête du disque** (secteur 0 : « SEGA 32X » ou « 32X » dans le champ console à `0x100` ou dans le titre) | **non vérifié** | impossible à contrôler ici sans image commerciale ; la sonde `disc_info` de la 6.0 ne lit que `SEGADISCSYSTEM` et la région |
| Liste des six titres en dur | sûre mais rigide | filet possible derrière les autres critères |

Proposition pour la conception : **DAT puis étiquette du nom**, la série `T-1620xF` en complément ; la sonde
d'en-tête n'est ajoutée que si un essai réel la confirme. **Demande à l'utilisateur** (essai 7.0 ou 7.1) : le
rapport de scan de son Night Trap 32X (nom du fichier, format `.cue` / `.chd`, identification obtenue), et si
possible les 512 premiers octets de la piste 01 lus par un outil de diagnostic à prévoir (titre et champ console
seulement, rien du jeu).

Deux présentations possibles (question 7.2 n° 10 pour l'utilisateur) :

- **A — sous « Mega CD », avec une marque « 32X »** (recommandé). Le jeu reste dans `megacd` (dossier
  `roms/megacd/`, DAT, jaquettes `Sega_-_Mega-CD_-_Sega_CD`, région, BIOS : tout existe) ; une marque par jeu
  impose picodrive au lancement, quel que soit le core préféré du Mega CD. Rien à ranger autrement ; conforme à
  libretro et à Redump.
- **B — système à part « Mega CD 32X »** (`roms/megacd32x/`). Plus lisible dans la liste des systèmes, mais :
  pas de base libretro ni de dossier de jaquettes propre (il faudrait pointer vers ceux du Mega CD), un 18e
  système pour six jeux, et le rangement 7.1 devrait apprendre à séparer deux systèmes dans un même DAT.

Cas particuliers à traiter dans la conception :

- **Fahrenheit** : deux disques de natures différentes (disque 1 Mega CD « Key Disc », disque 2 CD 32X). Le jeu
  32X demanderait d'insérer d'abord le disque clé (**non vérifié**). Le regroupement `(Disc N)` de la 4.0 en fera
  un seul jeu à deux disques : il doit alors partir sur picodrive dès qu'**un** disque est marqué 32X.
- Les autres jeux existent en deux éditions (Mega CD seul, CD 32X) : deux entrées distinctes dans la bibliothèque,
  la marque « 32X » les distingue.
- picodrive non installé : même dialogue que pour un core manquant (« Installer picodrive »), pas de repli
  silencieux sur genesis_plus_gx.
- Sauvegardes : un `.srm` de picodrive et un `.brm` de genesis_plus_gx ne sont pas le même fichier ; sans objet
  ici (les deux éditions sont deux jeux), à noter si l'utilisateur change de core pour le Mega CD.

## 5. Essais faits ici

Binaire `emu-rust 7.0.0-rc.1` (worktree `wip-7.0`, sans modification), core
`tests/cores/picodrive_libretro.so` (`2.05-1890c29`, SHA256 `fd4010af…1417d5`), Xvfb + llvmpipe, 120 images,
`--no-audio`. **Aucune donnée commerciale** : le disque vient de `tests/tools/mkcd.py megacd` (en-tête
`SEGADISCSYSTEM`, région USA, piste audio de silence), les « BIOS » sont des fichiers de 131 072 octets écrits pour
l'essai (vecteurs 68000 + quelques instructions, le reste à zéro).

| # | Dossier système | Résultat (**constaté**) |
|---|---|---|
| 1 | vide | `detected BIN Sega/Mega CD image with USA region`, `opening BIOS failed`, `Missing BIOS` ; emu-rust affiche « Le core a refusé le jeu : Missing BIOS », sortie propre. Le contrôle de firmware de la 6.0 avait signalé `Missing` avant le lancement. |
| 2 | `bios_CD_U.bin` synthétique dont le programme écrit ADEN (`move.b #1,$A15101` puis boucle : `13FC 0001 00A1 5101 60FE`) | jeu chargé (320×240, 60 i/s, 44,1 kHz) ; le core journalise **`32X startup`** à la 2e image, puis `32X shutdown` à la fermeture ; aucun plantage, 0 sous-alimentation audio |
| 3 | `bios_CD_U.bin` synthétique sans ADEN (boucle seule) | jeu chargé, **aucune** ligne `32X startup` |

Conclusions : (a) picodrive accepte un BIOS Mega CD quelconque (aucun contrôle d'empreinte par le core : c'est
emu-rust qui vérifie le md5) ; (b) avec un CD inséré, la 32X s'allume bien sur l'écriture d'ADEN, sans option ;
(c) le pipeline d'emu-rust (CD en place, contrôle du BIOS, profil de touches picodrive, dossier de sauvegarde)
fonctionne déjà avec picodrive sur un CD. L'image produite est noire dans les essais 2 et 3 (aucun programme
graphique) : elle ne prouve rien d'autre.

Remarque : picodrive liste la piste 1 du disque généré comme `AUDIO` dans son journal (piste de données de
32 secteurs) ; sans effet sur l'essai, à regarder si `mkcd.py` sert à un test plus poussé.

### Ce qu'on peut tester automatiquement en 7.2

- **Contenu libre** : aucun CD 32X libre ou homebrew connu (pas de recherche exhaustive ; l'inventaire fait pour
  la 6.0 ne trouvait déjà aucun disque Mega CD démarrable sans BIOS commercial, ce qui vaut a fortiori ici).
- **Test de fumée par image synthétique** (proposé) : étendre `tests/tools/mkcd.py` avec `--32x` (étiquette du
  nom, numéro de série en F, champ console) et un sous-outil `bios-stub` qui écrit le BIOS de l'essai n° 2 dans un
  dossier système **temporaire du test** (jamais dans `system/` de l'utilisateur, jamais distribué). Le test
  `#[ignore]` (core réel) vérifie : jeu marqué 32X ⇒ picodrive choisi ⇒ ligne `32X startup` dans le journal du
  core. Il prouve le routage et l'allumage de la 32X, **pas** l'émulation d'un jeu.
- **Sans core** (tests ordinaires) : classement d'un `.cue` / `.chd` à étiquette 32X, DAT réduit à quelques
  entrées fictives, choix du core, dialogue « BIOS manquant », picodrive absent.
- **Réel** : Night Trap 32X de l'utilisateur + son BIOS (sur son Mac, d'après ses indications) — vidéo, son,
  changement de disque, sauvegarde, état.

## 6. Impacts par crate (estimation, à confirmer par la conception)

| Crate | Travail | Taille |
|---|---|---|
| `emu-library` | Marque « CD 32X » par jeu : étiquette du nom DAT / du fichier, série `T-1620xF` ; sonde d'en-tête seulement si confirmée. Stockage : dérivé du titre à la lecture (aucune migration) ou colonne nouvelle (schéma v7) — à trancher. Option B : système `megacd32x` et partage du DAT | petit (A) / moyen (B) |
| `emu-assets` | `cores.toml` : règle « ce jeu exige picodrive » (clé par marque, ou entrée système pour l'option B) ; `required_firmware` de picodrive pour le Mega CD : **déjà là** ; rien à télécharger de nouveau | très petit |
| `emu-ui` | Choix du core au lancement selon la marque ; dialogue « picodrive requis » ; pastille « 32X » dans la bibliothèque ; libellé du dialogue BIOS | petit |
| `emu-runtime`, `emu-libretro` | rien d'attendu (`.srm` de la RAM interne déjà géré ; interface disque déjà gérée) | — |
| `emu-app` | rien | — |
| `tests/tools`, `qa-systems` | `mkcd.py --32x`, `bios-stub`, test de fumée, guide d'essai | petit |
| `reviewer` | rien de réseau ni d'`unsafe` ; relire que le BIOS synthétique ne quitte pas les dossiers de test | — |

Interaction avec la 7.1 (ADR 0023, rangement) : avec l'option A, rien à changer (un CD 32X se range sous
`roms/megacd/` comme tout CD Mega CD) ; avec l'option B, le rangement doit proposer `megacd32x/` pour les entrées
étiquetées du même DAT.

## 7. Points ouverts

1. Contenu réel de l'en-tête d'un CD 32X (titre, champ console, numéro de série) : à relever sur le disque de
   l'utilisateur.
2. Fahrenheit : ordre des disques au lancement (disque clé d'abord ?).
3. Compatibilité réelle des six jeux avec picodrive (vidéo, synchronisation du son) : inconnue tant qu'aucun jeu
   n'a tourné ; l'émulateur de référence pour comparer serait ares (hors libretro).
4. Format du Night Trap de l'utilisateur (`.cue` + pistes ou `.chd`) : décide du critère de reconnaissance utile.
5. Présentation A ou B : question à l'utilisateur (`docs/questions-ouvertes.md`, section 7.2).
