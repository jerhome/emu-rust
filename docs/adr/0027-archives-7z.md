# 0027 — Archives `.7z`

Statut : **proposée** (acceptation après relecture). Version **8.1**, branche `wip-8.1` (partie de `main`, recalée
sur `main` après la livraison de la 8.0). Détail : `docs/design/v8.1-archives-7z.md`. Décisions de l'utilisateur :
`docs/specs/v8.1-7z-grill.md` ; spec : `docs/specs/v8.1-7z-spec.md` (ces deux documents priment en cas d'écart).

Historique : ex-ADR 0023 de la branche `feat/7.5-7z`, renumérotée 0027 le 2026-11-25 à la fusion Mac + PC
(décision D11) ; prévue en 7.5, puis 7.7 (D1), **renumérotée 8.1 le 2026-11-26** (part après la 8.0) ; **recalée le
2026-11-27** sur la 8.0 (catalogue dynamique, schéma v8, ADR 0023 amendée le 2026-11-27) et sur le grill tour 1
(CD archivés en 8.1.1, minimum du cache 1000, dépendance relevée par `reviewer`).

## Contexte

Les `.zip` sont identifiés au scan et extraits au lancement (cartouches : en mémoire ou dossier temporaire ;
disquettes : copie de travail persistante) ; les `.7z` ne le sont pas (ADR 0017 et 0019 : transmis tels quels,
écritures perdues). Demande de l'utilisateur du 2026-11-24. La 8.0 a préparé le terrain : `ARCHIVE_EXTENSIONS`
(`emu-library/src/custom.rs`) contient déjà `7z`, jamais une extension de définition de système personnalisé ;
tant que la 8.1 n'est pas livrée un `.7z` n'est pas identifié.

## Décision

- **Périmètre 8.1** (grill T1-4) : **cartouches et disquettes en `.7z`**, traitées exactement comme en `.zip`.
  Un **CD archivé** (`.cue` et pistes, `.iso`, dans un `.zip` ou un `.7z`) reste **refusé** comme aujourd'hui
  (`ZIPPED_DISC` au scan, `LaunchRefusal::ArchivedDisc` au lancement, élargi au `.7z`). Son extraction proposée
  au lancement est la **8.1.1** (voir « 8.1.1 » ci-dessous).
- **Dépendance** : `sevenz-rust2 = "=0.27.0"`, `default-features = false`, `features = ["ppmd", "deflate"]`
  (Apache-2.0, pur Rust, sans code C ; dépendances `lzma-rust2` Apache-2.0, `ppmd-rust` CC0-1.0 OR MIT-0,
  `flate2` + `zlib-rs` déjà présents, `crc32fast` ; MSRV 1.93 compatible ; avis `reviewer` du 2026-11-27 :
  **go sous conditions**). Chiffrement exclu (pas de feature `aes256`) : une archive chiffrée est **refusée** avec un
  message clair, jamais de mot de passe. La feature de compression n'est qu'en dépendance de développement (archives
  de test générées). Version épinglée ; montée de version seulement par un lot dédié (fuzz minimal refait).
- **Mémoire** : toute lecture passe par `read_with_memory_limit`, borne de dictionnaire **64 Mo**.
- **`unsafe` de `lzma-rust2`** (assembleur du décodeur, non désactivable) : **fuzz minimal** du listing et de la
  décompression (corpus d'archives corrompues générées en test ordinaire, aucune panique ; courte campagne par
  `qa-systems` si l'outil est disponible), relecture de la dépendance par `reviewer`.
- **Scan** (`emu-library`) : identification par CRC32 + taille lus dans l'en-tête, sans décompression ; SHA1
  seulement en repli (DAT sans CRC, entrée sans CRC), par décompression bornée d'une entrée ; bloc solide ≤ 512 Mio
  décodés (même constante que la borne CRC Mega CD). Le scan ne remplit jamais le cache. Un lecteur d'archive
  commun zip / `.7z` remplace l'analyse propre au zip, sans dupliquer ses règles.
- **Lancement** (`emu-runtime`) : même chemin que le zip (cartouche en mémoire ou dossier temporaire selon
  `need_fullpath`, disquettes en copie de travail) ; solide décodé en une passe pour toutes les entrées voulues ;
  bornes (taille, ratio en cours de flux, dictionnaire) ; CRC revérifié ; écriture dans un `.partial` renommé à la
  fin ; progression et annulation coopérative.
- **Cache d'extraction** : plafond commun aux copies de travail, `[library] archive_cache_mb` (**défaut 2000,
  minimum 1000**, absente = défaut ; décisions du 2026-11-24 et du 2026-11-26), éviction LRU des copies **sans
  écritures de l'utilisateur** ; place libre vérifiée avant extraction (taille déclarée + 10 %).
- **puae / VICE** (Amiga, C64 ; aussi CPC, Atari ST) : **emu-rust extrait** un `.7z` qui contient des images
  acceptées (décision du 2026-11-24) ; les tables des ADR 0017 et 0019 sont mises à jour (lignes `.7z`, renvoi
  vers cette ADR) par le lot T1.
- **Systèmes personnalisés (8.0)** (grill T1-2) : un `.7z` sous `roms/<id>/` est identifié et lancé comme un
  `.zip` de ce système, sans réglage par système (règle d'ADR 0025 §9 « Archives » étendue au `.7z`).
- **Rangement** : voir « Recalage sur la 8.0 », point 3.
- Les téléchargements (cores, BIOS libres) restent en zip : pas de `.7z` côté réseau.
- **Graphe des crates inchangé** : lecture d'en-tête dans `emu-library`, extraction dans `emu-runtime` (sa propre
  dépendance, comme `zip`), `emu-ui` appelle `emu-library` pour les extensions. Aucun nouveau crate.

## Recalage sur la 8.0 (2026-11-27)

Contrats de référence : ADR 0025 (systèmes personnalisés, catalogue), ADR 0023 (rangement, **amendement du
2026-11-27**), `docs/design/v8.0-systemes-personnalises.md` (fichiers et lots PB0 à PB4). Ce paragraphe remplace
« Effet de la fusion (2026-11-25) » des versions précédentes de cette ADR.

1. **Catalogue dynamique** (ADR 0025 §1, lot PB1) : le scan et le plan de rangement lisent l'instantané du catalogue
   pris à leur départ (`&'static Catalog`) ; le lecteur d'archive ne consulte jamais le catalogue global en cours
   de route. La liste d'extensions d'un `.7z` se résout contre cet instantané, systèmes personnalisés compris.
2. **Schéma** : la base reste en **v8** (marque de scan `game.scan_pending` de la 8.0) ; **aucune migration**.
   `media` / `zip_media_agrees` servent aux `.7z` sans colonne nouvelle (nom gardé). Une base 8.1 reste lisible
   par la 8.0.
3. **Rangement (ADR 0023 §4, amendée le 2026-11-27)** : un `.7z` est une unité **« fichier seul »** comme un `.zip`,
   déplacée telle quelle, jamais décompressée ; son système vient de la liste d'extensions de l'en-tête (même
   fonction que le scan). Un `.7z` chiffré ou illisible : unité **inconnue**, laissée en place. **CD dans une
   archive** (grill Z1, T1-1) : « Sûre » seulement si les CRC32 + tailles des pistes, lus dans l'en-tête, désignent
   un jeu du DAT (Redump) d'un seul système, ou si le nom exact de l'archive est celui d'un jeu du DAT ; sinon
   « ambiguë » (l'utilisateur choisit) ; jamais « Sûre » par extension seule. Collisions : taille et CRC32 du
   fichier entier. **Journal inchangé** : le rangement ordinaire écrit toujours la version 1, la bascule la
   version 2 (ADR 0023, amendement du 2026-11-27), sans ligne nouvelle dans l'une ou l'autre ; les copies extraites
   suivent par `relocate_identities` (aller et annulation) ; l'éviction LRU n'est jamais écrite au journal.
   Extraction et rangement sont exclusifs (même verrou de tâche de fond).
4. **Ordre de démarrage** (ADR 0025 §13) : le nettoyage des `.partial` vient **après** la reprise du journal de
   rangement et la fin de bascule en attente, et **avant** le scan.
5. **Contrat 7.5** : `firmware_advice` inchangée ; erreurs d'extraction (`ArchiveError`, non exhaustif) rendues
   par `theme::error_box` avec bouton « Copier » et écrites au Journal ; l'annulation n'est pas une erreur.
6. **Thème 7.3** : barre de progression et bouton « Annuler » en `theme::`, aucun `Color32` brut.

## 8.1.1 — CD archivés (repoussé, grill T1-4)

Décision du 2026-11-24 gardée, livrée en **8.1.1** : un CD archivé (`.zip` ou `.7z`) est **proposé à
l'extraction** au lancement, place et temps annoncés, ou annulation expliquée ; cache libéré par LRU des copies
sans écritures s'il manque de la place ; plus gros que le plafond entier : refus « augmentez le cache dans les
Réglages » (T1-3) ; l'extraction reste dans le cache, jamais dans `roms/` (Z2) ; sonde `32X Initial Program` sur
la copie extraite avant le choix du core, résultat écrit par `Library::set_needs_32x`. Conception : design §9.3.

## Effet sur chaque système livré

| Système | Effet en 8.1 |
|---|---|
| megadrive, snes, nes, gb, gbc, gamegear, mastersystem, gba, 32x | change : `.7z` de cartouche reconnu au scan (CRC d'en-tête) et extrait au lancement comme un zip |
| atarist | change : `.7z` de disquettes extrait en copie de travail (écritures gardées), regroupement et ordre comme le zip |
| amiga | change : `.7z` extrait par emu-rust (plus transmis à puae) ; table de l'ADR 0017 mise à jour |
| cpc | change : `.7z` extrait par emu-rust (cap32 ne lit pas d'archive) |
| c64 | change : `.7z` extrait par emu-rust (plus par VICE dans `TEMP`) ; table de l'ADR 0019 mise à jour |
| dos | change : un `.7z` de jeu DOS suit la règle du zip (rangé « suggérée », contenu) ; à vérifier : lancement par dosbox_pure (passe le zip tel quel ; le `.7z` aussi s'il le liste, sinon extrait) |
| megacd | inchangé au lancement : CD archivé (zip ou `.7z`) **refusé** comme aujourd'hui, extraction en **8.1.1** ; change au rangement : CD dans une archive « Sûre » si pistes ou nom reconnus par le DAT, sinon « ambiguë » |
| psx | idem megacd (8.1.1 : gros CD, place annoncée) |
| 3do | idem megacd |
| systèmes personnalisés (8.0 : Atari Lynx, PC Engine, Atari 2600…) | change : `.7z` sous `roms/<id>/` identifié et lancé comme un `.zip` du système (respecte `block_extract` et `need_fullpath` du core) ; aucun réglage par système |

Sans `.7z` dans la bibliothèque, rien ne change (sauf le rangement d'un CD dans un `.zip`, voir design §9.1).

## Conséquences

`.7z` jouables avec sauvegardes persistantes ; nouvelle dépendance épinglée à surveiller (`unsafe` de
`lzma-rust2`, montées de version par lot dédié avec fuzz) ; coût CPU et disque des archives solides sur le Mac de
test (2 cœurs, 8 Go, SSD presque plein : plafond du cache) ; tables 0017 / 0019 mises à jour ; aucune migration
(schéma v8). Limite écrite dans le CHANGELOG et le guide d'essai 8.1 : CD archivé toujours refusé, extraction en
8.1.1.
