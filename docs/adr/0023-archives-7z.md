# 0023 — Archives `.7z`

Statut : **proposée** (à valider par l'utilisateur). Détail : `docs/design/v7.5-archives-7z.md`.

## Contexte
Les `.zip` sont identifiés au scan et extraits en copie de travail au lancement ; les `.7z` ne le sont pas
(ADR 0017 et 0019 : transmis tels quels, écritures perdues). Demande utilisateur du 2026-11-24.

## Décision
- Dépendance `sevenz-rust2` (Apache-2.0, pur Rust), sans chiffrement ni codec à dépendance C ; sous réserve de
  la vérification de `reviewer` (licences, features, `unsafe`). Archive chiffrée : refusée.
- Scan : identification par CRC32 + taille lus dans l'en-tête, sans décompression ; SHA1 seulement en repli,
  décompression bornée d'une entrée.
- Lancement : extraction par le frontend en copie de travail (même chemin que zip), solide décodé en une passe,
  bornes (taille, ratio, mémoire du dictionnaire), progression et annulation, plafond disque avec éviction LRU.
- CD dans un 7z : refusé (comme zip). Cores `.7z`-capables (puae, VICE) : le frontend extrait d'abord.
- Les téléchargements (cores, BIOS) restent en zip : pas de 7z côté réseau.

## Conséquences
`.7z` jouables avec sauvegardes persistantes ; nouvelle dépendance à surveiller ; coût CPU/disque des solides sur
le Mac de test ; les tables 0017/0019 sont à mettre à jour (lignes `.7z`).
