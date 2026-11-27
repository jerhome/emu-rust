---
name: implement
description: Orchestre l'implémentation d'une spec ou de tickets d'emu-rust en déléguant chaque lot aux agents de crate, puis qa-systems, reviewer et la boucle d'échec. La session principale ne code pas. À lancer seulement sur demande.
disable-model-invocation: true
---

# Implement

Copie locale adaptée de `implement` (mattpocock, 1.3.1). Ici, la session principale **orchestre** et ne modifie aucun
code, même d'une ligne : elle délègue à l'agent du périmètre (`emu-rust:<agent>`, modèle adapté, opus pour le délicat).

## Avant de commencer

- Énonce le titre du ticket (`docs/tickets/<version>/<NN>-...md`) ou de la spec. Référence ambiguë : demande.
- `git fetch`, lis `docs/reprise.md`. Travaille dans le worktree de la version, jamais ailleurs.
- Rappelle à chaque agent, dans sa consigne : l'environnement (`. ~/.local/udev/env.sh`, `CARGO_INCREMENTAL=0`,
  `CARGO_TARGET_DIR` du worktree, jamais de nouveau `target`), les fichiers des autres agents actifs, le disque libre
  (`df -h /workspace`, sous 15 Go ne pas compiler). Le critère de « terminé » du ticket.
- Un seul build lourd à la fois ; 3 agents actifs en moyenne, sur des fichiers différents.

## Boucle

1. **Lot** : lance l'agent du périmètre (`libretro-ffi`, `runtime-av`, `library-assets`, `ui`, `platform-build`) sur
   le ticket ; `architecte` d'abord si une interface entre crates est à poser. Un agent ne sort jamais de son crate :
   il décrit le besoin dans son résumé.
2. **TDD** quand c'est possible, aux coutures convenues dans la spec : test rouge, puis vert (skill `tdd`). Tests
   portables Linux / Windows / macOS (règle du CLAUDE.md).
3. **Vérification** : `qa-systems` lance la vérification complète
   (`cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`)
   et rend un verdict. Les tests ciblés pendant le lot, la suite complète une fois à la fin.
4. **Boucle d'échec** (CLAUDE.md) : `À CORRIGER` → `triage` → agents désignés dans l'ordre → `qa-systems` ; au plus
   **3 cycles** sur le même échec, puis demander à l'utilisateur.
5. **Relecture** : `reviewer` sur tout `unsafe`, réseau, téléchargement, archive, identifiants ; **opus** pour
   les sauvegardes, données de l'utilisateur et déplacements de fichiers. Puis le skill `code-review` (en plus, jamais
   à la place). Au 3e tour de relecture : seuls bloquants et majeurs sont corrigés, les mineurs sont notés.
6. **Clôture du ticket** : coche les critères dans le fichier `.md`, note les limites connues, indique le prochain
   ticket de la frontière.

## Git et livraison

- Commits sur la branche de la version, **seulement avec l'accord de l'utilisateur** ; les sous-agents ne commitent
  jamais. Identité locale `jerhome` vérifiée (`git config --local user.email`).
- **Push par lots** (étape, pré-release, lot vérifié), après la vérification locale complète, pas après chaque commit.
  Nouvelle étape de CI : vérifiée localement (actionlint, pwsh en Docker) ou sur une branche avant `main`.
- Pas de PR : quand tout est validé, la session principale fusionne dans `main` et pousse. Version du workspace
  vérifiée à la fusion. **Aucun tag sans l'accord explicite de l'utilisateur.**
- Essai réel : fiche en artifact avec le build testé et son empreinte ; on n'y écrit pas pendant l'essai.
  Défaut trouvé à l'essai : taguer la version essayée, puis corriger en x.y.1.
