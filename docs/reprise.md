# Reprise — état et prochaines tâches

À lire en début de session (surtout en changeant de machine : la mémoire de Claude est locale à chaque poste).
Mis à jour à chaque fin de session.

## État au 2026-11-24, 23 h (fin de session sur le Mac)

- `main` = **7.4.0** : 7.1, 7.2, 7.3 et 7.4 fusionnées ; CI verte sur le code 7.4.0. CHANGELOG 7.1.0 → 7.4.0
  « non publiée (tag à valider) ». La CI vérifie la version du workspace contre les marqueurs « ✅ vX.Y » de la
  roadmap (`scripts/check-version-vs-roadmap.sh`).
- **Aucun tag** : v7.3.0 et v7.4.0 attendent la validation de l'utilisateur.
- 7.1, 7.2 et 7.3 essayées et validées par l'utilisateur sur macOS ; 7.4 (Night Trap USA Sega CD 32X en picodrive,
  badge 32X) à confirmer avec la `.app` 7.4.0.
- Branche `feat/7.5-7z` poussée : conception `docs/design/v7.5-archives-7z.md`, ADR 0023 proposée, décisions de
  l'utilisateur (CD archivés : proposer l'extraction en exposant le risque ou annuler en expliquant ; cache 2 Go
  LRU réglable ; emu-rust extrait aussi pour puae / VICE).
- Branche distante obsolète `feat/7.4-sega-cd-32x` (avant rebase) et branches déjà fusionnées : supprimables avec
  l'accord de l'utilisateur.

## Prochaine session (PC du bureau, Windows + WSL2) — dans cet ordre

1. Créer le cron de suivi (CLAUDE.md, « Suivi pendant un développement »).
2. **Demander les caractéristiques du PC** et les inscrire dans CLAUDE.md (« Cibles ») : modèle exact du CPU,
   cœurs / threads, RAM, disque libre, version de Windows, limites WSL2 (`.wslconfig`), écran utilisé
   (définition, fréquence), sortie audio (fréquence), manettes. Déjà connu : portable Intel Iris Xe,
   OpenGL 3.3 sous Windows, Vulkan mal supporté (PEAK sur Steam injouable en Vulkan, bon en DirectX) →
   rendu matériel Windows par défaut en D3D11 / OpenGL, jamais Vulkan.
3. Tags v7.3.0 / v7.4.0 à faire valider ; retour de l'utilisateur sur la 7.4.
4. 7.5 : T0 `reviewer` (go / no-go sur `sevenz-rust2` : licence, pur Rust, `unsafe`, paniques), puis la suite du
   design.
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
   Idée 8.x : core libretro « jgenesis » (32X, PC Engine) ; `reviewer` : GO sous conditions, 32X bloquant (BIOS Sega
   embarqués dans `s32x-core`) ; portage existant RobLoach/libretro-jgenesis — détails dans la roadmap.

## Budget

Hebdomadaire ~88 % utilisé le 2026-11-24 à 22 h 40 : garder ~5 % pour le travail pro de l'utilisateur jusqu'au
2026-11-25 19 h. Demander `/usage` aux points de suivi.
