# 0027 — Archives `.7z`

Statut : **acceptée** le 2026-11-27 après trois tours de relecture. Version **8.1**, branche `wip-8.1` (partie de `main`, recalée
sur `main` après la livraison de la 8.0). Détail : `docs/design/v8.1-archives-7z.md`. Décisions de l'utilisateur :
`docs/specs/v8.1-7z-grill.md` ; spec : `docs/specs/v8.1-7z-spec.md`, alignée le 2026-11-27 sur les
amendements après relecture (design §0.1) ; ces deux documents priment en cas d'écart.

Historique : ex-ADR 0023 de la branche `feat/7.5-7z`, renumérotée 0027 le 2026-11-25 à la fusion Mac + PC
(décision D11) ; prévue en 7.5, puis 7.7 (D1), **renumérotée 8.1 le 2026-11-26** (part après la 8.0) ; **recalée le
2026-11-27** sur la 8.0 (catalogue dynamique, schéma v8, ADR 0023 amendée le 2026-11-27) et sur le grill tour 1
(CD archivés en 8.1.1, minimum du cache 1000, dépendance relevée par `reviewer`) ; **amendée le 2026-11-27 après
relecture** (`reviewer` : 8 majeurs ; décisions de l'utilisateur sur le DOS et l'éviction, choix techniques de la
session principale ; détail au design §0.1).

## Contexte

Les `.zip` sont identifiés au scan et extraits au lancement (cartouches : en mémoire ou dossier temporaire ;
disquettes : copie de travail persistante) ; les `.7z` ne le sont pas (ADR 0017 et 0019 : transmis tels quels,
écritures perdues). Demande de l'utilisateur du 2026-11-24. La 8.0 a préparé le terrain : `ARCHIVE_EXTENSIONS`
(`emu-library/src/custom.rs`) contient déjà `7z`, jamais une extension de définition de système personnalisé ;
tant que la 8.1 n'est pas livrée un `.7z` n'est pas identifié.

## Décision

- **Périmètre 8.1** (grill T1-4) : **cartouches et disquettes en `.7z`**, traitées exactement comme en `.zip`.
  Un **CD archivé** (`.cue` et pistes, `.iso`, dans un `.zip` ou un `.7z`) reste **refusé** au scan et au
  lancement (`ZIPPED_DISC` au scan, `LaunchRefusal::ArchivedDisc` au lancement, élargi au `.7z`) ; il est en
  revanche **rangé** (voir « Recalage sur la 8.0 », point 3). Son extraction proposée au lancement est la
  **8.1.1** (voir « 8.1.1 » ci-dessous). **Volumes** `.7z.001`, `.7z.002`… : non pris en charge, ignorés au scan
  (leur extension n'est pas `7z`).
- **Dépendance** : `sevenz-rust2 = "=0.27.0"`, `default-features = false`, `features = ["ppmd", "deflate"]`
  (Apache-2.0, pur Rust, sans code C ; dépendances `lzma-rust2` Apache-2.0, `ppmd-rust` CC0-1.0 OR MIT-0,
  `crc32fast` ; MSRV 1.93 compatible ; avis `reviewer` du 2026-11-27 : **go sous conditions**). À **vérifier au lot
  T1** par `cargo tree` / `cargo metadata` : arbre réel et licences, backend de la feature `deflate` (`flate2` +
  `zlib-rs` déjà présents par `zip`, ou autre), MSRV et `resolver = "3"` du workspace. Chiffrement exclu (pas de
  feature `aes256`) : une archive chiffrée est **refusée** avec un message clair, jamais de mot de passe. La
  feature de compression n'est qu'en dépendance de développement (archives de test générées). Version épinglée ;
  montée de version seulement par un lot dédié (fuzz minimal refait).
- **Mémoire et bornes d'entrée** : avant toute ouverture par `sevenz-rust2`, la **taille du fichier d'archive** est
  bornée (constante, design §2) et la taille de l'en-tête suivant annoncée par l'en-tête de départ aussi ; toute
  lecture passe ensuite par `read_with_memory_limit`, borne **64 Mo**. Le lot T1 vérifie dans le code de
  `sevenz-rust2` ce que cette borne couvre (en-tête compressé, nombre de fichiers annoncé, mémoire PPMd,
  dictionnaire LZMA / LZMA2) ; ce qu'elle ne couvre pas est contrôlé par emu-rust sur les propriétés des codeurs
  **avant** décodage. Ces cas entrent au fuzz minimal.
- **`unsafe` de `lzma-rust2`** (assembleur du décodeur, non désactivable) : **fuzz minimal** du listing et de la
  décompression (corpus d'archives corrompues générées en test ordinaire, aucune panique ; courte campagne par
  `qa-systems` si l'outil est disponible), relecture de la dépendance par `reviewer`.
- **Scan** (`emu-library`) : identification par CRC32 + taille lus dans l'en-tête, sans décompression ; SHA1
  seulement en repli (DAT sans CRC, entrée sans CRC), par décompression bornée d'une entrée ; bloc solide ≤ 512 Mio
  décodés (même constante que la borne CRC Mega CD). Le scan ne remplit jamais le cache. Un lecteur d'archive
  commun zip / `.7z` remplace l'analyse propre au zip, sans dupliquer ses règles. Une entrée chiffrée (codeur AES
  du dossier, champ `encrypted` du listing) ou un en-tête chiffré ⇒ archive non identifiée. Liens (Unix
  `S_IFLNK` dans les 16 bits hauts des attributs, point de réparation Windows `0x400`), dossiers et anti-items
  ignorés.
- **Lancement** (`emu-runtime`) : même chemin que le zip (cartouche en mémoire ou dossier temporaire selon
  `need_fullpath`, disquettes en copie de travail) ; solide décodé en une passe pour toutes les entrées voulues ;
  **bornes absolues seulement** (aucun ratio de compression) : taille déclarée par entrée, total
  `MAX_COPY_BYTES`, arrêt dès que les octets décodés d'une entrée dépassent sa taille déclarée, taille du fichier
  d'archive bornée avant ouverture ; CRC revérifié ; écriture par l'écriture atomique existante
  (`write_atomic_with`, temporaire `.<nom>.tmp-<pid>` renommé à la fin, supprimé en cas d'erreur ou
  d'annulation) ; progression et annulation coopérative. **Pas de vérification préalable de la place libre** :
  un disque plein pendant l'écriture donne une erreur claire (`DiskFull`), temporaire nettoyé.
- **Transmis tel quel** : un `.7z` n'est passé tel quel au core que si celui-ci liste `7z` dans
  `valid_extensions` ; `block_extract` ne vaut **pas** pour le `.7z` (il ne vaut que pour le zip, règle
  inchangée). Sinon : extrait comme une cartouche ou des disquettes en zip. Disquettes : un `.7z` qui contient des
  images acceptées est toujours extrait (ordre du design 3.0 §3.5.1, ADR 0017 / 0019).
- **DOS** (décision de l'utilisateur du 2026-11-27) : un `.7z` de jeu DOS est passé **tel quel** à `dosbox_pure`
  si le core installé liste `7z` dans `valid_extensions` ; sinon le lancement est **refusé** avec « convertissez
  en `.zip` ». **Jamais d'extraction partielle** d'un jeu DOS. Au scan et au rangement, règle du zip DOS
  inchangée.
- **Cache d'extraction** : plafond commun aux copies de travail, `[library] archive_cache_mb` (**défaut 2000,
  minimum 1000**, absente = défaut ; décisions du 2026-11-24 et du 2026-11-26). **Éviction LRU** (décision de
  l'utilisateur du 2026-11-27) **seulement** des dossiers de copies créés par la 8.1 **depuis un `.7z`** (origine
  marquée dans un manifeste **v2**, écrit seulement pour un dossier dont toutes les entrées viennent d'un `.7z` ;
  tout autre dossier garde le manifeste v1 **octet pour octet**, lisible par la 7.6 et la 8.0) et **intacts**
  (taille et CRC32 de chaque entrée égaux à l'en-tête, aucun fichier en plus que le manifeste et `<jeu>.m3u`).
  Un **verrou par jeu** `<saves>/<core>/.<jeu>.disks.lock`, hors du dossier de copies et jamais supprimé, est
  pris en partagé par le lanceur avant toute inspection et pendant toute la session (deux instances du même jeu
  restent possibles, comme aujourd'hui), en exclusif par l'évinceur pendant vérification et suppression
  (critère et détails au design §3.1) ; l'évinceur tire le verrou du nom de jeu écrit dans le manifeste v2, jamais
  du nom du dossier (v2 sans ce nom, ou verrou impossible à ouvrir : dossier gardé) ; un lanceur sans verrou ne
  lance que depuis un dossier v1, sinon il extrait dans le dossier temporaire privé. Plafond commun à tous les
  cores. Les copies des versions 7.x et 8.0 et toute
  copie tirée d'un `.zip` ne sont **jamais supprimées** : le contrat de `media.rs` (« les anciennes copies et la
  progression qu'elles portent ne sont jamais modifiées ni supprimées ») reste vrai pour elles. Plafond
  impossible à tenir avec les seules copies évinçables : l'extraction a lieu quand même, avertissement au Journal.
  Critère et suppression : relecture `reviewer` **opus** (données de l'utilisateur).
- **puae / VICE** (Amiga, C64 ; aussi CPC, Atari ST) : **emu-rust extrait** un `.7z` qui contient des images
  acceptées (décision du 2026-11-24) ; les tables des ADR 0017 (l.314 « `.7z` et CD hors bibliothèque »), 0018
  (DOS) et 0019 (l.82, extraction de VICE dans `TEMP`) sont mises à jour (lignes `.7z`, renvoi vers cette ADR)
  par le lot T1.
- **Systèmes personnalisés (8.0)** (grill T1-2) : un `.7z` sous `roms/<id>/` est identifié et lancé comme un
  `.zip` de ce système, sans réglage par système (règle d'ADR 0025 §9 « Archives » étendue au `.7z`), avec la
  règle « transmis tel quel » ci-dessus.
- **Garde-fous de la 8.0** (ADR 0025 §5) : le **témoin de lancement** est créé **après** l'extraction (une
  extraction longue ou annulée n'est jamais prise pour un plantage du core) ; le comportement de l'enfant
  `--dry-run-child` face à un `.7z` est fixé par le lot T1 (design §3.2).
- **Rangement** : voir « Recalage sur la 8.0 », point 3.
- Les téléchargements (cores, BIOS libres) restent en zip : pas de `.7z` côté réseau.
- **Graphe des crates inchangé** : lecture d'en-tête dans `emu-library`, extraction dans `emu-runtime` (sa propre
  dépendance, comme `zip`), `emu-ui` appelle `emu-library` pour les extensions. Aucun nouveau crate.

## Recalage sur la 8.0 (2026-11-27)

Contrats de référence : ADR 0025 (systèmes personnalisés, catalogue), ADR 0023 (rangement, **amendement du
2026-11-27**), `docs/design/v8.0-systemes-personnalises.md` (fichiers et lots PB0 à PB4). Ce paragraphe remplace
« Effet de la fusion (2026-11-25) » des versions précédentes de cette ADR. Base de code relue : `wip-8.0`
`e3c1de0` (PB2 non relu, PB3 et PB4 à faire) ; fichiers et symboles **revérifiés par le lot T1** sur `main` après
la livraison de la 8.0.

1. **Catalogue dynamique** (ADR 0025 §1, lot PB1) : le scan et le plan de rangement lisent l'instantané du catalogue
   pris à leur départ (`&'static Catalog`) ; le lecteur d'archive ne consulte jamais le catalogue global en cours
   de route. La liste d'extensions d'un `.7z` se résout contre cet instantané, systèmes personnalisés compris.
2. **Schéma** : la base reste en **v8** (marque de scan `game.scan_pending` de la 8.0) ; **aucune migration**.
   `media` / `zip_media_agrees` servent aux `.7z` sans colonne nouvelle (nom gardé). Une base 8.1 reste lisible
   par la 8.0, **mais** un scan fait par la 8.0 ne voit pas les `.7z` et retire leurs lignes (favoris et jaquettes
   compris) : limite écrite dans le CHANGELOG et le guide d'essai 8.1.
3. **Rangement (ADR 0023 §4, amendée le 2026-11-27)** : un `.7z` est une unité **« fichier seul »** comme un `.zip`,
   déplacée telle quelle, jamais décompressée ; son système vient de la liste d'extensions de l'en-tête (même
   fonction que le scan). Un `.7z` chiffré (en-tête ou données) ou illisible : unité **inconnue**, laissée en
   place. **CD dans une archive** (grill Z1, T1-1 ; `.zip` et `.7z`, confirmé par l'utilisateur le 2026-11-27) :
   « Sûre » seulement si les CRC32 + tailles des pistes `.bin` (le `.cue` est ignoré), lus dans l'en-tête,
   désignent un jeu du DAT (Redump) d'un seul système, ou si le nom exact de l'archive est celui d'un jeu du DAT
   d'un seul système ; sinon « ambiguë » (l'utilisateur choisit) ; jamais « Sûre » par extension seule. Cette
   règle **amende ADR 0023 §4.3** (étape 3, « une image CD dans un zip reste refusée ») et ajoute l'exception
   « nom exact ⇒ Sûre » pour un CD archivé : amendement écrit par le **lot T1**, pas avant. Collisions : taille et
   CRC32 du fichier entier. **Journal inchangé** : le rangement ordinaire écrit toujours la version 1, la bascule
   la version 2 (ADR 0023, amendement du 2026-11-27), sans ligne nouvelle dans l'une ou l'autre (revérifié par le
   lot T1 sur le code livré) ; les copies extraites suivent par `relocate_identities` (aller et annulation) ;
   l'éviction LRU n'est jamais écrite au journal. Extraction et rangement sont exclusifs (même verrou de tâche de
   fond).
4. **Ordre de démarrage** (ADR 0025 §13) : **inchangé**. Pas d'étape de nettoyage global : l'écriture atomique
   existante ne laisse un temporaire `.<nom>.tmp-<pid>` qu'après un arrêt brutal ; il est remplacé à la prochaine
   écriture du même nom par un processus de même pid, et rend son dossier non intact (jamais évincé).
5. **Contrat 7.5** : `firmware_advice` inchangée ; erreurs d'extraction (`ArchiveError`, non exhaustif) rendues
   par `theme::error_box` avec bouton « Copier » et écrites au Journal ; l'annulation n'est pas une erreur.
6. **Thème 7.3** : barre de progression et bouton « Annuler » en `theme::`, aucun `Color32` brut.

## 8.1.1 — CD archivés (repoussé, grill T1-4)

Décision du 2026-11-24 gardée, livrée en **8.1.1** : un CD archivé (`.zip` ou `.7z`) est **proposé à
l'extraction** au lancement, place et temps annoncés, ou annulation expliquée ; cache libéré par LRU des copies
évinçables (même critère qu'en 8.1) s'il manque de la place ; plus gros que le plafond entier : refus « augmentez
le cache dans les Réglages » (T1-3) ; l'extraction reste dans le cache, jamais dans `roms/` (Z2) ; sonde
`32X Initial Program` sur la copie extraite avant le choix du core, résultat écrit par `Library::set_needs_32x`.
Conception : design §9.3 (borne de taille d'archive à revoir pour les gros CD).

## Effet sur chaque système livré

| Système | Effet en 8.1 |
|---|---|
| megadrive, snes, nes, gb, gbc, gamegear, mastersystem, gba, 32x | change : `.7z` de cartouche reconnu au scan (CRC d'en-tête) et extrait au lancement comme un zip (passé tel quel seulement si le core liste `7z`) |
| atarist | change : `.7z` de disquettes extrait en copie de travail (écritures gardées), regroupement et ordre comme le zip |
| amiga | change : `.7z` extrait par emu-rust (plus transmis à puae) ; table de l'ADR 0017 mise à jour |
| cpc | change : `.7z` extrait par emu-rust (cap32 ne lit pas d'archive) |
| c64 | change : `.7z` extrait par emu-rust (plus par VICE dans `TEMP`) ; table de l'ADR 0019 mise à jour |
| dos | change : un `.7z` de jeu DOS suit la règle du zip au scan et au rangement (« suggérée », contenu) ; au lancement, passé tel quel si `dosbox_pure` liste `7z`, sinon refusé (« convertissez en `.zip` ») ; jamais extrait ; table de l'ADR 0018 mise à jour |
| megacd | inchangé au lancement : CD archivé (zip ou `.7z`) **refusé** comme aujourd'hui, extraction en **8.1.1** ; change au rangement : CD dans une archive « Sûre » si pistes `.bin` ou nom reconnus par le DAT, sinon « ambiguë » |
| psx | idem megacd (8.1.1 : gros CD, place annoncée) |
| 3do | idem megacd |
| systèmes personnalisés (8.0 : Atari Lynx, PC Engine, Atari 2600…) | change : `.7z` sous `roms/<id>/` identifié et lancé comme un `.zip` du système (passé tel quel seulement si le core liste `7z`, `need_fullpath` respecté) ; aucun réglage par système |

Sans `.7z` dans la bibliothèque, rien ne change (sauf le rangement d'un CD dans un `.zip`, voir design §9.1).

## Conséquences

`.7z` de cartouches et de disquettes jouables, sauvegardes persistantes pour les disquettes ; nouvelle dépendance
épinglée à surveiller (`unsafe` de `lzma-rust2`, montées de version par lot dédié avec fuzz) ; coût CPU et disque
des archives solides sur le Mac de test (2 cœurs, 8 Go, SSD presque plein : plafond du cache, éviction limitée aux
copies `.7z` intactes) ; tables 0017 / 0018 / 0019 et ADR 0023 §4.3 amendées au lot T1 ; aucune migration
(schéma v8). Limites écrites dans le CHANGELOG et le guide d'essai 8.1 : CD archivé toujours refusé au lancement
(extraction en 8.1.1) ; `.7z` DOS refusé si `dosbox_pure` ne le lit pas ; volumes `.7z.001` ignorés ; un scan par
la 8.0 retire les lignes des `.7z` ; pas de vérification préalable de la place libre ; plafond du cache dépassable
par les copies non évinçables.
