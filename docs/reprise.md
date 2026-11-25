# Reprise — état et prochaines tâches

À lire en début de session (surtout en changeant de machine : la mémoire de Claude est locale à chaque poste).
Mis à jour à chaque fin de session. **En début de session : `git fetch` puis lire ce fichier** (règle commune
d'agents-home, ajoutée après la divergence du 2026-11-25).

## État au 2026-11-25, 23 h (fin de session sur le PC du bureau)

- **Divergence réparée.** Le 2026-11-25, le PC a travaillé sans pull sur une ancienne base (sa 7.1 = rangement des
  ROMs, sa 7.2 = CD 32X). Fusion faite sur la base du Mac : `main` = **7.5.0** (05ff67d, puis correctif CI 2ce5b3e).
  Décisions D1-D15, plan et notes : `docs/design/v7.5-finition-pc.md` (contrat) et ADR 0023 à 0026 (amendements de
  fusion).
- **7.5 = finition PC** sur la 7.4 : journal (page ouverte depuis le pied de page), pied de page sur toutes les
  pages, messages copiables (presse-papiers réel, corrige le « Copier » de la 7.3), `emu-rust-cli.exe` sous Windows,
  icône Mac produite par `scripts/gen-icons.sh`, écran Cores, bandeau des fichiers d'origine vérifiés (md5),
  alias BIOS PS1, message « buildbot en cours de mise à jour », disquettes `MANIACD0/1` regroupées, compléments
  CD 32X (marque manuelle touche X / clic droit). Schéma v7 inchangé.
- **Aucun tag** : v7.3.0, v7.4.0 et v7.5.0 attendent l'essai de l'utilisateur (guide `docs/guide-essai-7.5.md`,
  questions F1-F4 à confirmer).
- **7.6 = rangement des ROMs** (ligne PC) : en cours sur `wip-7.6` (rangement dans emu-library + écrans) ;
  branchement sur la 7.5 dans la branche locale `wip-7.6-merge` (PC). Point ouvert : `TidyBusy` intermittent dans
  les tests (verrou du journal, ~5 % sur overlayfs) en diagnostic.
- **En avance** : conceptions 8.0 (ADR 0025 acceptée) et 9.0 (ADR 0026 acceptée) recalées sur la fusion ; 7z
  renuméroté **7.7 / ADR 0027** sur la branche `wip-7.7` (ex-`feat/7.5-7z`, locale au PC, non poussée) ; anglais en 7.8 ;
  8.1 jgenesis ; paliers des systèmes personnalisés 8.2 (CD), 8.3 (ordinateurs), 8.4 (arcade).
- Branches distantes : `wip-7.1`, `wip-7.2` (ligne PC d'avant fusion, gardées pour l'historique), `wip-7.5` ;
  `feat/7.2-manette`, `feat/7.4-sega-cd-32x`, `feat/7.5-7z` : supprimables avec l'accord de l'utilisateur.

## Prochaine session — dans cet ordre

1. `git fetch`, lire ce fichier, créer le cron de suivi.
2. Retour de l'utilisateur sur l'essai de la 7.5 (Windows et Mac : terminal, Copier, icône de la `.app`, Night Trap
   32X, manette SNES, deux instances) ; tags v7.3.0 / v7.4.0 / v7.5.0 à faire valider.
3. 7.6 : finir le branchement, diagnostic `TidyBusy`, vérification complète, revue, push.
4. **Demander les caractéristiques du PC** et les inscrire dans CLAUDE.md (« Cibles ») — non fait le 2026-11-25.
5. Dettes à déléguer : `Library::clear_cover` / `clear_all_covers` + `CoverCache::remove` dans emu-library
   (remplacer le contournement de T8) ; réponse `Loaded` périmée dans `covers.rs` (génération) ; avertissement
   cargo `main.rs` dans deux binaires d'emu-app ; captures Xvfb avant / après de la 7.3 (T7, dans Docker) ;
   page Réglages (défilement), barre d'outils qui passe sur deux lignes à 1280 px, focus initial de la
   bibliothèque ; idée : regrouper les disques d'un jeu répartis dans plusieurs dossiers.
6. **Rappel demandé par l'utilisateur : skills AI Hero (Matt Pocock)** — lire https://www.aihero.dev/skills-catalog,
   proposer 2 ou 3 skills au plus (discipline de conception : interroger une conception, améliorer une
   architecture existante — la revue « Ousterhout »), l'utilisateur choisit, `reviewer` relit, installation en
   copie modifiable dans `.claude/skills/` adaptée à nos règles (français, pas de PR, délégation aux agents).
   Pas le paquet entier.
7. Proposée et bien accueillie : revue « Ousterhout » par `reviewer` (modules superficiels, contournements
   tactiques, fuites d'information) → liste de dettes.
8. Sur le Mac, quand l'utilisateur est sur l'écran Retina seul : interface HiDPI ×2, mise à l'échelle des jeux en
   16:10, performances à ~4 Mpx (SNES et Night Trap 32X plein écran), déplacement de fenêtre entre écrans.
9. Ensuite : 7.6 interface en anglais ; 8.0 systèmes libretro non testés ; idée non planifiée après 8.0 : édition web
   (WASM, 8/16 bits) ou son dans le mode noVNC (voir la roadmap).
   8.1 : core libretro « jgenesis » au choix (fork jerhome) (32X, PC Engine) ; `reviewer` : GO sous conditions, 32X bloquant (BIOS Sega
   embarqués dans `s32x-core`) ; portage existant RobLoach/libretro-jgenesis — détails dans la roadmap.

## Budget

Hebdomadaire ~88 % utilisé le 2026-11-24 à 22 h 40 : garder ~5 % pour le travail pro de l'utilisateur jusqu'au
2026-11-25 19 h. Demander `/usage` aux points de suivi.
