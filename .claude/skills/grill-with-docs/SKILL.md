---
name: grill-with-docs
description: Interview serré pour affiner un plan ou une conception d'emu-rust, qui tient à jour le glossaire et les ADR au fil de l'eau. À lancer seulement sur demande (grill, conception d'une version).
disable-model-invocation: true
---

# Grill avec docs

Copie locale adaptée de `grill-with-docs` (mattpocock, 1.3.1) : fusion de `grilling` et `domain-modeling`.
Plan d'intégration : `docs/skills-mattpocock.md`.

## Entretien

Interroge l'utilisateur jusqu'à un accord partagé. Représente la conception en **arbre de décisions** : chaque
décision ouvre celles qui en dépendent.

1. Lis d'abord `docs/reprise.md`, `docs/roadmap.md`, `docs/glossaire.md`, les ADR du domaine touché
   (`docs/adr/`, elles font autorité) et les conceptions existantes (`docs/design/`, `docs/specs/`).
2. Calcule la **frontière** : les décisions dont les prérequis sont réglés. Pose toute la frontière en **un seul
   questionnaire** (`AskUserQuestion`), jamais une liste de questions en texte. Pour chaque question : la réponse
   recommandée en premier, marquée « (défaut) », formulée pour que « oui » l'accepte.
3. Les **faits** (code, fichiers, état de la CI, licences) se cherchent seul, ou par un sous-agent en lecture seule ;
   ne demande jamais à l'utilisateur ce que tu peux vérifier. Seules les **décisions** lui reviennent. Une question
   dépendant d'une réponse encore ouverte attend le tour suivant.
4. Après les réponses, recalcule la frontière et recommence. Une question qui bloque un agent est posée tout de
   suite ; les autres sont groupées après la revue de l'utilisateur.
5. Fini quand la frontière est vide : aucune branche visitée à moitié, rien de supposé en silence. N'agis pas
   (pas de spec, pas de code) avant la confirmation de l'utilisateur.

## Modèle de domaine, pendant l'entretien

- **Termes en conflit** : si l'utilisateur emploie un terme qui contredit `docs/glossaire.md`, signale-le tout de
  suite.
- **Langage flou** : propose un terme canonique précis (une seule appellation, les autres notées « à éviter »).
- **Scénarios concrets** : invente des cas limites (jeu multi-disquette, archive, CD 32X, base migrée) pour forcer
  la précision des frontières entre concepts.
- **Croiser avec le code** : quand l'utilisateur dit comment une chose marche, vérifie dans le code ; signale la
  contradiction.
- **Glossaire au fil de l'eau** : un terme résolu est écrit aussitôt dans `docs/glossaire.md` (définition en une ou
  deux phrases, aucun détail d'implémentation, ce n'est ni une spec ni un brouillon). Ne crée aucun `GLOSSARY.md`.
- **ADR avec parcimonie** : propose une ADR seulement si le choix est (1) difficile à défaire, (2) surprenant sans
  contexte, (3) le résultat d'un vrai arbitrage. Sinon, rien. Une ADR suit la numérotation de `docs/adr/` et reste
  « proposée » jusqu'à la fin des relectures ; c'est `architecte` qui la rédige.

## Règles du repo

- Documents en français, identifiants en anglais. Aucune issue ni PR GitHub : les conclusions vont dans
  `docs/specs/<version>-grill.md` (comme `docs/specs/v10-mobile-grill.md`).
- Pas de code ni de commit. Les limites et sujets reportés sont consignés sans demander (reprise, CHANGELOG, guide).
- Un seul projet par session : emu-rust, sans digression sur les autres repos.
