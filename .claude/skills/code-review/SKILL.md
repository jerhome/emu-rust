---
name: code-review
description: Relit les changements depuis un point fixe (commit, branche, tag) selon deux axes, Standards (règles documentées du repo) et Spec (conformité à la spec ou au ticket), en sous-agents parallèles. S'ajoute à emu-rust:reviewer, ne le remplace jamais.
---

# Code review à deux axes

Copie locale adaptée de `code-review` (mattpocock, 1.3.1). Compare `HEAD` à un point fixe fourni par l'utilisateur.
**Elle vient EN PLUS de `emu-rust:reviewer`** (`unsafe`, FFI, réseau, archives, secrets, licences ; opus pour
les sauvegardes et données), jamais à sa place. Lecture seule : aucun commit, aucune correction.

## Processus

1. **Point fixe** : celui de l'utilisateur (SHA, branche, tag, `main`, `HEAD~5`) ; sinon demande. Vérifie
   `git rev-parse <point>` et que `git diff <point>...HEAD` n'est pas vide, avant de lancer les sous-agents. Note
   `git log <point>..HEAD --oneline`.
2. **Source de la spec**, dans l'ordre : un chemin donné ; le ticket (`docs/tickets/<version>/`) ou la spec
   (`docs/specs/`) cités dans les messages de commit ou correspondant à la branche ; la conception `docs/design/`
   et l'ADR concernée. Sinon demande ; sans spec, l'axe Spec est sauté et le rapport le dit.
3. **Sources des standards** : `CLAUDE.md`, `../agents-home/docs/rust/CONVENTIONS.md`, les ADR de `docs/adr/`
   touchées, `docs/glossaire.md`. En plus, la **base de défauts** (heuristiques, toujours un jugement, jamais une
   violation ; une règle documentée du repo l'emporte ; on saute ce que fmt et clippy imposent) : nom mystérieux ;
   code dupliqué ; envie de fonctionnalité (méthode qui fouille les données d'un autre) ; paquets de données qui
   voyagent ensemble ; obsession des primitives ; switch répétés ; chirurgie en rafale (un changement, fichiers
   éparpillés) ; changement divergent (un fichier modifié pour plusieurs raisons) ; généralité spéculative ;
   chaînes de messages ; intermédiaire inutile ; héritage refusé.
4. **Deux sous-agents en parallèle**, au premier plan (lecture seule, sans sous-agent) :
   - *Standards* : commande de diff, liste des commits, sources et base de défauts collées en entier. Consigne :
     « pour chaque fichier ou bloc, (a) toute violation d'un standard documenté, avec la règle citée (fichier +
     règle) ; (b) tout défaut de la base, nommé, avec le bloc cité ; distingue violations nettes et jugements ;
     saute ce que l'outillage impose ; sous 400 mots. »
   - *Spec* : commande de diff, commits, contenu de la spec. Consigne : « (a) exigences absentes ou partielles ;
     (b) comportement non demandé (dérive) ; (c) exigences qui semblent faites mais mal ; cite la ligne de la spec
     pour chaque constat ; sous 400 mots. »
5. **Agrège** sous `## Standards` et `## Spec`, sans fusionner ni reclasser. Termine par une ligne : nombre de
   constats par axe et le pire de chaque axe. Les constats mécaniques répétés (motif de syntaxe, API interdite)
   deviennent une proposition de contrôle automatique pour la rétro, pas une règle de plus.

Un changement peut passer un axe et échouer l'autre : les deux rapports restent séparés. La session principale
route les corrections vers les agents de crate (boucle d'échec du CLAUDE.md).
