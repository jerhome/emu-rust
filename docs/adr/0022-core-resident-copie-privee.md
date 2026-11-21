# ADR 0022 — Recharger un core resté en mémoire : copie privée de la bibliothèque

Statut : **accepté** (2026-11-21, architecte ; défaut appliqué sans attendre l'utilisateur, ligne dans
`docs/questions-ouvertes.md`). Transversal : s'applique à **tout** core, pas seulement au DOS. Précise ADR 0008
(frontière FFI, un seul core vivant par processus) et ADR 0001 ; amende ADR 0018 (4.0). Graphe des crates inchangé,
aucune nouvelle dépendance, aucune clé `cores.toml`.

## Contexte

Défaut constaté par `qa-systems` sur `wip-4.0` : **dosbox_pure plante (SIGSEGV) au 2e chargement dans le même
processus** dès que le 1er chargement a exécuté des images. Impacts : « Redémarrer le jeu » (`emu-ui/src/app.rs`,
`restart_game` : l'`App` est détruite puis reconstruite, donc `Core` est détruit puis rechargé) et le mode
bibliothèque (`emu-ui/src/library/host.rs` : un jeu après l'autre dans le même processus). Les tests réels DOS
contournent déjà le défaut en relançant un processus par session (`emu-runtime/tests/dos_real.rs`, `zz_worker`).

Essais de l'architecte (2026-11-21, mini-hôte C hors dépôt, cores de `tests/cores/`, contenus libres de
`tests/roms/` et `tests/tools/mkdos.py`, 300-400 images par session) :

| Scénario (dosbox_pure, `hello.zip`) | Résultat |
|---|---|
| `retro_deinit` + `dlclose` + `dlopen` du **même chemin** | même poignée (bibliothèque restée en mémoire), **SIGSEGV** dans le 2e `retro_load_game` |
| `retro_deinit` + `retro_init` sans `dlclose` | **SIGSEGV** : le core ne supporte pas deinit puis init dans la même image |
| `retro_unload_game` + `retro_load_game` sans deinit (« garder le core ») | fonctionne, mais l'image obtenue pour `menu.zip` **diffère** de celle d'un processus neuf (état résiduel) |
| 2e et 3e chargement d'une **copie** du `.so` sous un autre nom | fonctionne ; images **identiques au bit près** à celles d'un processus neuf ; RSS +25 Mo au 2e chargement, +0 au 3e |

- Cause du maintien en mémoire : `libstdc++` (chargée comme dépendance de dosbox_pure, marquée `NODELETE` à cause de
  ses symboles uniques) **se lie à des instanciations de modèles C++ exportées par dosbox_pure**
  (`LD_DEBUG=bindings` : `basic_stringbuf`, `ctype::do_widen`…) ; glibc ne peut alors plus décharger dosbox_pure :
  `dlclose` réussit sans rien démapper, l'état statique survit à `retro_deinit`. Dépend de l'environnement : si
  `libstdc++` est déjà chargée avant le core (pilote GL Mesa, par exemple), le maintien peut ne pas se produire.
- **Autres cores** (même essai « deux chargements successifs avec images ») : genesis_plus_gx, snes9x, gambatte,
  mesen, hatari, puae, puae2021, mgba, picodrive, fceumm, nestopia, sameboy **passent** et sont réellement déchargés
  (`dlopen(RTLD_NOLOAD)` après `dlclose` : absent). dosbox_pure est le **seul** core resté en mémoire, et le seul qui
  plante. cap32 et vice_x64 (5.0) n'ont pas pu être essayés avec ce mini-hôte trop sommaire : à couvrir par le test
  générique ci-dessous.
- **RetroArch** décharge et recharge le même chemin (`dylib_close` / `dylib_load`) ; il ne copie le core que pour la
  2e instance du *run-ahead* (`runahead/secondary_core.c` : copie dans un fichier temporaire, précisément parce que
  `dlopen` du même chemin rend la même image). Que RetroArch ne voie pas ce plantage s'explique probablement par une
  `libstdc++` déjà chargée par l'exécutable (non vérifiable ici).

Pistes évaluées :

| Piste | Coût | Risques | Verdict |
|---|---|---|---|
| (a) copie privée de la bibliothèque sous un nom unique | moyen, entièrement dans `emu-libretro` | image ancienne non libérée (≈ 3 Mo de code + reliquats du tas) ; dossier de copies à tenir propre ; dépendances voisines du core non trouvées depuis la copie (cores du buildbot autonomes) | **retenue** |
| (b) un processus enfant par jeu | très élevé : fenêtre, contexte GL, audio, entrées et IPC vidéo à refaire | latence, deux fenêtres, ADR 0008 / 0010 à réécrire | rejetée (reste la technique des tests réels) |
| (c) garder le core chargé entre deux jeux (unload/load sans deinit) | élevé côté UI (durée de vie de `Emulator` hors de `App`) | état résiduel mesuré ; options lues au chargement non réappliquées ; changer de core violerait « un seul core par processus » | rejetée |
| (d) clé `cores.toml` `reload = "copy"` statique | faible | ne suit pas l'environnement (le maintien dépend de qui a chargé `libstdc++`, de l'OS) ; oublis pour les cores futurs | rejetée au profit d'une détection |

## Décision

1. **Détection automatique dans `emu-libretro`** : après `Library::close` dans `Drop for Core`, une sonde dit si
   l'image est **restée en mémoire** :
   - Unix (Linux, macOS) : `dlopen(chemin, RTLD_LAZY | RTLD_NOLOAD)` via `libloading::os::unix::Library::open`
     (valeur de `RTLD_NOLOAD` posée par `#[cfg(target_os)]` : Linux 0x4, macOS 0x10 ; pas de nouvelle dépendance) ;
     une poignée obtenue est refermée aussitôt ;
   - Windows : `libloading::os::windows::Library::open_already_loaded(chemin)`.
   Restée en mémoire ⇒ le chemin **d'origine** (canonique) entre dans un registre de processus
   `RESIDENT: Mutex<HashSet<PathBuf>>` (`tracing::info!`, une fois par core).
2. **Chargement suivant** : `Core::load` d'un chemin présent dans `RESIDENT` charge une **copie privée** :
   `<copy_dir>/<nom de fichier sans extension>-<pid>-<n>.<ext>` (`n` = compteur de processus), écrite avec
   `create_new` (jamais d'écrasement ni de lien suivi), dossier créé en `0700` sous Unix, octets copiés puis fichier
   fermé avant le chargement. **Une vraie copie, jamais un lien** (glibc et dyld reconnaissent une image déjà chargée
   par inode). La copie, si elle reste à son tour en mémoire, compte comme le même core (le registre est indexé par
   le chemin d'origine).
3. **Invariants** : `GET_LIBRETRO_PATH` (19) répond le **chemin d'origine**, jamais celui de la copie ; `core_id`,
   messages et journaux citent le chemin d'origine. Le premier chargement d'un processus est toujours direct (aucun
   coût pour les cores qui se déchargent correctement, c'est-à-dire tous les cores livrés sauf dosbox_pure).
4. **Nettoyage** : Unix : la copie est **supprimée dès que `dlopen` a réussi** (l'image reste mappée ; l'inode d'une
   image épinglée n'est jamais réutilisé tant qu'elle est mappée). Windows : supprimée au `Drop` si l'image n'est
   plus en mémoire ; sinon laissée, et chaque nouvelle copie commence par supprimer les fichiers du dossier qui ne
   sont plus ouverts (échecs ignorés : fichier en cours d'usage par cette instance ou une autre).
5. **Emplacement** : `copy_dir` est fourni par l'appelant (`CoreConfig::copy_dir`). `emu-runtime` passe
   `Some(<cache_dir>/core-copies)` (`<data>/cache/core-copies/`, même arborescence de données que `cores/`, donc un
   système de fichiers d'où l'on charge déjà des bibliothèques ; jamais `/tmp`, parfois monté `noexec`). « Vider le
   cache DOS » ne touche pas ce dossier.
6. **Échec** : core resté en mémoire et `copy_dir = None`, ou copie impossible (disque plein, droits, chargement de
   la copie refusé) ⇒ `LibretroError::CoreCopy { path, source }` ; **jamais** de repli sur l'image restée en mémoire
   (elle planterait). L'UI affiche : « Ce core ne peut pas être rechargé dans cette session (…). Quittez puis
   relancez emu-rust. »
7. **Test générique obligatoire** pour tout core livré ou ajouté : « deux chargements successifs avec images » —
   `Core::load`, `load_game`, 300 images, `drop`, puis de nouveau `Core::load`, `load_game`, 300 images ; la dernière
   image de la 2e session doit égaler celle de la 1re (même contenu libre). Chaque core tourne dans un processus
   enfant (un plantage devient un échec nommé, pas la mort du binaire de test). Plus un cas **non ignoré** sur le
   core factice : variante liée avec `-z nodelete` (Linux) dont un garde statique refuse un 2e `retro_init` dans la
   même image : passe grâce à la copie, et la sonde le voit « resté en mémoire ».

Contrat (posé par la tâche **DX1**, corps remplis par **DF2**) :

```rust
// emu-libretro/src/core.rs
pub struct CoreConfig {
    // … champs existants …
    /// Where a private copy of the library is written when the original stays resident after unloading
    /// (ADR 0022). `None`: reloading a resident core fails with `LibretroError::CoreCopy`.
    pub copy_dir: Option<PathBuf>,
}
// emu-libretro/src/error.rs (enum `#[non_exhaustive]`)
#[error("core {path} stayed in memory after unloading and no private copy could be loaded: {source}")]
CoreCopy { path: PathBuf, #[source] source: std::io::Error },
```

Aucune autre API publique : la détection et la copie sont internes à `Core::load` / `Drop for Core`.

## Conséquences

- « Redémarrer le jeu » et l'enchaînement des jeux DOS dans la bibliothèque fonctionnent sans changement de l'UI ;
  les images d'un rechargement sont celles d'un processus neuf.
- Mémoire : chaque rechargement d'un core resté en mémoire garde l'image précédente (≈ 3 Mo pour dosbox_pure) et ce
  que son `retro_deinit` ne libère pas (mesuré : +25 Mo au 1er rechargement, stable ensuite). Acceptable pour un
  usage personnel ; à surveiller si un core futur fuit davantage.
- `unsafe` nouveau dans `emu-libretro` (sonde `RTLD_NOLOAD` / `open_already_loaded`, chargement de la copie) :
  revue `reviewer` (poignée refermée, aucune exécution de code de la copie avant le chargement normal, `create_new`,
  droits du dossier, pas de course entre deux instances : noms par PID).
- Windows : une DLL copiée ne retrouve pas une DLL dépendante posée à côté de l'original (les cores du buildbot
  n'en ont pas) ; macOS Intel : la copie garde la signature ad hoc incluse dans le Mach-O.
- Les tests réels DOS peuvent garder un processus par session (isolation des empreintes) ; un test réel prouve en
  plus le rechargement dans le même processus.
- Le test générique (point 7) détecte tout core futur qui plante au rechargement. S'il reste en mémoire sans que la
  sonde le voie (plate-forme qui ment), une clé `cores.toml` forçant la copie suffira (amendement de cette ADR) ;
  s'il plante **sans** rester en mémoire (état hors de son image), la copie n'y peut rien : il faudra rouvrir la
  piste (b) pour ce core.
