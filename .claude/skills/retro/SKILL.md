---
name: retro
description: Rétrospective d'emu-rust fusionnée avec la rétro d'agents-home : constats par catégorie, questionnaire avec défauts, mesures, tableau skill par skill (garder / adapter / abandonner), date de rétro, amélioration des agents. À lancer seulement sur demande, en fin de version.
disable-model-invocation: true
---

# Rétro

Copie locale adaptée de `retro` (mattpocock, 1.3.1), fusionnée avec notre rétro (`../agents-home/AGENTS.md`,
sections « Agents » et « Règles communes »). But : améliorer l'**environnement** de l'agent (docs, contrôles, agents),
pas le code. Dans ce repo seulement.

## Étapes

1. **Dates** : `date` pour toute heure écrite. Lis « Dernière rétro » dans `../agents-home/CLAUDE.local.md` : la
   rétro couvre la période depuis cette date. Lis `docs/reprise.md`, `docs/skills-mattpocock.md`, le
   journal de session ou la liste des agents lancés tenue dans la reprise, `git log --since=<date> --all`.
2. **Sources primaires** : la session demandée (par défaut la courante), CI rouges, défauts d'essai réel, retours
   des agents.
3. **Candidats d'amélioration**, par catégorie :
   - *Navigation* : un fichier a-t-il été long à trouver ? un pointeur aiderait-il ?
   - *Contrôles automatiques* : une erreur de l'agent aurait-elle été attrapée par un lint, un test, une étape de CI
     (lire d'abord la commande de vérification et la CI du repo) ? Une faute **mécanique** appelle un contrôle
     déterministe (clippy, hook, CI) plutôt qu'une règle ; réserve les règles aux **jugements**.
   - *Standards* : une règle à donner à `reviewer` ou à retirer ? (le relecteur impose les standards, pas
     l'implémenteur).
   - *CLAUDE.md et AGENTS.md* : consignes à déplacer vers un standard ou un contrôle ; consignes sans effet à
     retirer (fichiers courts, pointeurs seulement).
   - *Économie d'outils* : appels coûteux, builds lourds, quota consommé (relevé de début et de fin).
   - *Accès à l'information* : un fait crucial manquait-il à l'agent ?
   - *Flux et agents* : étapes sautées, travail en double, idées d'accélération, disque, CI.
4. **Tableau skill par skill** (obligatoire) : pour chaque skill du flux employé dans la version (grill-with-docs,
   to-spec, to-tickets, implement, tdd, code-review, retro), une ligne **apport** (ce qu'il a trouvé ou évité) /
   **coût** (temps, quota, friction) / décision **garder · adapter · abandonner**, selon le plan
   `docs/skills-mattpocock.md`. Un « abandonner » ramène ce skill seul au processus d'avant.
5. **Questionnaire** : toutes les décisions à l'utilisateur dans **un seul** `AskUserQuestion`, chacune avec un
   défaut marqué, présenté après sa revue ; seule une question qui bloque un agent est posée tout de suite.
   Candidats classés par gravité.
6. **Après les décisions** (commit libre des docs de rétro, accord permanent noté dans CLAUDE.md ; accord
   demandé pour le reste) :
   - écrire les mesures adoptées (CLAUDE.md, fiches d'agents, hooks, CI) ;
   - **améliorer les agents** des projets **touchés** depuis la rétro précédente (au moins un commit, ou un agent
     lancé) et ceux de base d'agents-home si concernés ; ne toucher à aucun autre projet ;
   - mettre à jour `docs/skills-mattpocock.md` (décisions, prochaine version) et `docs/reprise.md` ;
   - inscrire dans `../agents-home/CLAUDE.local.md` : « Dernière rétro : AAAA-MM-JJ HH:MM » (heure lue avec `date`).
7. **Fin de session** : `docs/reprise.md` à jour et poussé, branches en cours poussées (push par lots), pour que
   l'autre poste les voie.

## Référence

- L'agent d'implémentation subit la plus forte pression de contexte (exploration, code, débogage) ; le relecteur
  reçoit un diff. Les standards se font donc imposer en revue.
- Pour les skills, utiliser le skill `writing-for-agents` pour le style d'écriture des consignes.
- Fichiers : `CLAUDE.md` en pointeurs seulement ; docs de référence pointées ; skills pour les commandes
  lancées par l'utilisateur.
- Mesure de la rétro précédente non appliquée ? La signaler en tête.
