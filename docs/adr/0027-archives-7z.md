# 0027 — Archives `.7z`

Statut : **proposée** (à valider par l'utilisateur). Ex-ADR 0023 de la branche `feat/7.5-7z`, renumérotée le
2026-11-25 à la fusion Mac + PC (décision D11 : 0023 à 0026 sont au PC). Version **7.7** (ex-7.5, décision D1),
branche `wip-7.7`. Détail : `docs/design/v7.7-archives-7z.md`.

## Contexte

Les `.zip` sont identifiés au scan et extraits en copie de travail au lancement ; les `.7z` ne le sont pas
(ADR 0017 et 0019 : transmis tels quels, écritures perdues). Demande utilisateur du 2026-11-24.

## Décision

- Dépendance `sevenz-rust2` (Apache-2.0, pur Rust), sans chiffrement ni codec à dépendance C ; sous réserve de
  la vérification de `reviewer` (licences, features, `unsafe`). Archive chiffrée : refusée.
- Scan : identification par CRC32 + taille lus dans l'en-tête, sans décompression ; SHA1 seulement en repli,
  décompression bornée d'une entrée.
- Lancement : extraction par le frontend en copie de travail (même chemin que zip), solide décodé en une passe,
  bornes (taille, ratio, mémoire du dictionnaire), progression et annulation, plafond disque (**2 Go** par défaut,
  réglable, décision utilisateur) avec éviction LRU.
- CD (cue + pistes) dans un `.zip` ou un `.7z` : plus de refus sec ; **extraction proposée** au lancement, avec la
  place et le temps annoncés, ou annulation expliquée (décision utilisateur du 2026-11-24).
- Cores `.7z`-capables (puae, VICE) : **emu-rust extrait** d'abord (décision utilisateur).
- Les téléchargements (cores, BIOS) restent en zip : pas de 7z côté réseau.
- Activation **système par système** (pas pour tous d'un coup) : voir le tableau ci-dessous.

## Effet de la fusion (2026-11-25)

Détail et contrats : design §9. Résumé des décisions d'architecture :

1. **Rangement 7.6 (ADR 0023 §4)** : un `.7z` est une unité « fichier seul » comme un `.zip`, déplacée telle quelle,
   jamais décompressée ; son système vient du listing d'en-tête (étape 3 de §4.3, même fonction que le scan). Un
   `.7z` qui contient un CD n'est jamais « Sûre » par sonde (pas de décompression au rangement). Format du journal
   inchangé (version 1). Les copies extraites suivent l'archive par `relocate_identities` (aller et annulation) :
   aucune ligne de journal nouvelle. Extraction et rangement sont exclusifs (même verrou de tâche de fond).
2. **Schéma** : aucune migration, la bibliothèque reste en **v7** ; `media` / `zip_media_agrees` servent aux `.7z`
   sans colonne nouvelle.
3. **CD 32X (ADR 0024, socle Mac)** : au scan, un CD archivé n'est pas sondé (pas de décompression) ;
   `needs_32x` vient du nom (`name_needs_32x`) ou de la marque manuelle. Après une extraction acceptée, la sonde
   `32X Initial Program` est passée sur la copie extraite **avant** le choix du core, et le résultat est écrit dans
   `game.needs_32x`.
4. **Contrat 7.5** : `firmware_advice` inchangée (elle ne dépend pas du conteneur) ; erreurs d'extraction rendues
   par `theme::error_box` avec bouton « Copier » (`copyable`) et écrites au Journal (compteur du pied de page) ;
   l'annulation par l'utilisateur n'est pas une erreur.
5. **Thème 7.3** : barre de progression et dialogue « Extraire ce CD » en `theme::`, aucun `Color32` brut.

## Effet sur chaque système livré

| Système | Effet |
|---|---|
| megadrive | change : `.7z` reconnu au scan et extrait au lancement |
| snes | change : idem |
| nes | change : idem |
| gb | change : idem |
| gbc | change : idem |
| gamegear | change : idem |
| mastersystem | change : idem |
| atarist | change : `.7z` de disquettes extrait en copie de travail (écritures gardées), regroupement comme zip |
| megacd | change : CD archivé (zip ou 7z) proposé à l'extraction au lieu du refus ; sonde 32X après extraction |
| amiga | change : `.7z` extrait par emu-rust (plus transmis à puae) ; tableau ADR 0017 à mettre à jour |
| dos | à vérifier : un `.7z` de jeu DOS suit la règle du zip (dosbox_pure lit lui-même le zip) |
| cpc | change : `.7z` extrait par emu-rust (cap32 ne lit pas d'archive) |
| c64 | change : `.7z` extrait par emu-rust (plus par VICE dans `TEMP`) ; tableau ADR 0019 à mettre à jour |
| gba | change : comme les cartouches |
| 32x | change : comme les cartouches |
| psx | change : CD archivé proposé à l'extraction (gros : place annoncée) |
| 3do | change : idem psx |

Sans `.7z` dans la bibliothèque, rien ne change. Le CD archivé n'est extrait qu'après accord, jeu par jeu.

## Conséquences

`.7z` jouables avec sauvegardes persistantes ; nouvelle dépendance à surveiller ; coût CPU/disque des solides sur
le Mac de test ; les tables 0017/0019 sont à mettre à jour (lignes `.7z`, renvoi vers 0027). Une base de la 7.7 reste
lisible par la 7.4 à la 7.6 (pas de migration).
