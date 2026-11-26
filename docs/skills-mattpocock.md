# Intégration des skills mattpocock — plan version par version

Validé par l'utilisateur le 2026-11-26. Chaque rétro de version décide, skill par skill : **garder**, **adapter** ou
**abandonner** (tableau apport / coût / décision dans la rétro fusionnée). Si une rétro dit non pour un skill, on revient
au processus d'avant pour ce skill seulement.

**Préalable** : copies locales des 6 skills du flux (grill-with-docs, to-spec, to-tickets, implement, code-review,
retro) dans `.claude/skills/` de ce repo, adaptées à nos règles : français, sans PR, délégation aux agents de crate,
push par lots.

| Version | Point de départ | Skills ajoutés | Tickets | Pourquoi là |
|---|---|---|---|---|
| 8.0 systèmes personnalisés | conception acceptée, PX0 fait | tdd (PB0 puis lots de logique), code-review (en plus de `reviewer`), retro fusionnée en fin de version | lots du design (`.md`) | exécution seulement, rien à casser |
| 8.1 archives 7z | ADR 0027 proposée, points ouverts | + grill-with-docs (points ouverts) + to-spec + implement (orchestration des lots) | `.md` | petite version à conception à finir |
| 8.2 interface en anglais | rien de conçu | flux complet : grill → to-spec → to-tickets → implement → code-review → retro | to-tickets en `.md` | 1re version entièrement en flux, peu risquée |
| 8.3 à 8.6 | esquissés | flux complet | `.md` | le flux devient la norme si les rétros le valident |
| 9.0 rendu matériel | ADR 0026 acceptée | flux à partir de to-spec | issues GitHub ? à trancher en rétro | grosse version : décision sur les issues |
| 10 mobile | grill tour 1 fait (`docs/specs/v10-mobile-grill.md`) | flux complet depuis grill-with-docs | `.md` ou issues selon la rétro de la 9.0 | version neuve, flux de bout en bout |

Règles :

- Un ajout à la fois par version, pour savoir ce que chaque skill apporte.
- Nos agents restent les exécutants : les skills cadrent le travail, sans remplacer les agents de crate ni `reviewer`
  (`unsafe`, sécurité, licences).
- La rétro du skill retro est fusionnée avec la nôtre (questionnaire, mesures) dans un seul skill local
  (`.claude/skills/retro/`), dans ce repo seulement ; tout l'hybride mattpocock reste dans ce repo, rien dans
  agents-home. Le skill applique aussi les règles communes de rétro d'agents-home (date « Dernière rétro », amélioration
  des agents des projets touchés), qui restent là-bas tant qu'elles ne dépendent d'aucun langage.
