---
name: to-tickets
description: Découpe une spec, un plan ou la conversation en tickets « balle traçante », chacun avec ses dépendances, écrits en fichiers .md dans docs/tickets/<version>/. À lancer seulement sur demande.
disable-model-invocation: true
---

# To tickets

Copie locale adaptée de `to-tickets` (mattpocock, 1.3.1). Découpe en **tickets** : tranches verticales fines, chacune
déclarant les tickets qui la **bloquent**. Les tickets sont des **fichiers `.md`** (pas d'issue GitHub avant la 9
ou la 10 ; décision en rétro).

## Processus

1. **Contexte** : travaille depuis la conversation ; si l'utilisateur donne une spec (`docs/specs/...`), lis-la en
   entier.
2. **Explore le code** (si pas fait) ; titres et descriptions en vocabulaire de `docs/glossaire.md`, ADR respectées.
   Cherche un **pré-remaniement** : « rends le changement facile, puis fais le changement facile ».
3. **Tranches verticales** :
   - chaque tranche traverse un chemin étroit mais COMPLET (données, logique, interface, tests) ; pas une couche ;
   - une tranche terminée est démontrable ou vérifiable seule ;
   - taille : tient dans un contexte d'agent neuf, **dans le périmètre d'un seul agent de crate** quand c'est
     possible (`libretro-ffi`, `runtime-av`, `library-assets`, `ui`, `platform-build`) ; sinon, indique l'agent
     de chaque morceau et la couture entre eux (interface posée d'abord par `architecte`) ;
   - le pré-remaniement passe en premier.
   Un **gros remaniement mécanique** (renommage, changement de type partagé) fait exception : séquence
   *étendre–migrer–contracter* (ajouter la nouvelle forme à côté de l'ancienne, migrer par lots, supprimer l'ancienne).
4. **Questionne l'utilisateur** : présente la liste numérotée (titre, bloqué par, ce que ça livre, agent) et pose
   en **un seul questionnaire** `AskUserQuestion` (défaut marqué) : granularité, dépendances, fusions ou
   découpes. Itère jusqu'à l'accord.
5. **Écris** un fichier par ticket : `docs/tickets/<version>/<NN>-<slug>.md`, numérotés dans l'ordre des
   dépendances (bloquants d'abord). Jamais un fichier unique. Ni chemins de fichiers ni extraits de code.

## Modèle de ticket

```md
# <NN> : <titre>

**À construire :** le comportement de bout en bout, vu de l'utilisateur.

**Agent :** <agent de crate>, vérification : qa-systems ; reviewer (opus si sauvegardes / données) : oui | non

**Bloqué par :** numéros et titres, ou « Aucun (démarrable tout de suite) ».

**Statut :** prêt

- [ ] Critère d'acceptation 1
- [ ] Critère d'acceptation 2 (test portable Linux / Windows / macOS)
```

Travaille la **frontière** : tout ticket dont les bloquants sont finis ; les tickets d'agents différents sans
fichiers communs partent en parallèle (cible : 3 agents actifs). Ne touche à aucune autre doc que les tickets.
