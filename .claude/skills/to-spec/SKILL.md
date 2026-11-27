---
name: to-spec
description: Transforme la conversation en spec d'une version d'emu-rust, écrite dans docs/specs/ ; sans interview, par synthèse de ce qui a été décidé. À lancer seulement sur demande.
disable-model-invocation: true
---

# To spec

Copie locale adaptée de `to-spec` (mattpocock, 1.3.1). Synthétise le contexte et la connaissance du code en une spec.
**N'interviewe pas** l'utilisateur ; si une décision manque, relance `grill-with-docs`.

## Processus

1. Explore l'état du code si ce n'est pas fait. Emploie le vocabulaire de `docs/glossaire.md` et respecte les ADR de
   la zone (`docs/adr/`, elles font autorité) ainsi que les conceptions de `docs/design/`.
2. Esquisse les **coutures de test** : préfère les coutures existantes (tests par crate, `crates/*/tests/`, cores
   factices, mocks `wiremock`, empreintes d'images, captures Xvfb), la plus haute possible ; idéalement une seule.
   Vérifie avec l'utilisateur (un questionnaire `AskUserQuestion`, défaut marqué) qu'elles lui conviennent.
3. Écris la spec avec le modèle ci-dessous dans `docs/specs/<version>-spec.md`. Pas de PR, pas d'issue : le fichier
   est la source. Aucun commit sans l'accord de l'utilisateur.

## Modèle

```md
# Spec <version> — <titre>

## Problème
Le problème, du point de vue de l'utilisateur.

## Solution
La solution, du point de vue de l'utilisateur.

## Histoires d'utilisateur
Liste numérotée LONGUE : « 1. En tant que <acteur>, je veux <fonction>, afin de <bénéfice> ». Elle couvre tous les
aspects : cas nominal, erreurs, OS (Linux, Windows, macOS Intel), manette et clavier, données existantes migrées.

## Décisions d'implémentation
Modules touchés (crates), interfaces modifiées, précisions techniques de l'utilisateur, décisions d'architecture
(renvoyer aux ADR), changements de schéma (migration de la base), contrats. Ni chemins de fichiers ni extraits de
code, qui périment vite (exception : un extrait de prototype qui encode une décision mieux que la prose).

## Décisions de test
Ce qu'est un bon test (comportement externe, pas l'implémentation) ; modules testés ; antécédents dans le repo ;
tests portables (règle du CLAUDE.md) ; cas particuliers pour le guide d'essai (multi-disquette, CD 32X, sauvegardes
d'état) ; ce qui exige le reviewer opus (sauvegardes, données de l'utilisateur, déplacement de fichiers).

## Hors périmètre
## Notes
```

Les limites connues et les sujets reportés vont dans « Notes » sans demander l'accord.
