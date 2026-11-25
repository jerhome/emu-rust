# ADR 0025 — Version 8.0 : systèmes personnalisés (systèmes libretro non testés)

Statut : **proposé** (2026-11-25, architecte ; amendé le même jour après la revue indépendante
`.claude/rv-8.0.md`). À faire accepter par l'utilisateur avec ses réponses aux questions 8.0
(`docs/questions-ouvertes.md`).
Périmètre : `docs/specs/v7.1-v8.0-utilisateur.md` §8.0 et feuille de route (ligne 8.0) ; étude
`docs/essais-systemes-personnalises.md`, dont les réponses de l'utilisateur (§10, 2026-11-21) **font foi** : défaut
recommandé partout, sauf **Q15** (export / import d'une définition : **reporté**, à rappeler à la conception de la
8.1) et **Q16** (un système devenu officiel : la définition de l'utilisateur **reste en place**, emu-rust **propose
la bascule, recommandée** ; jeux, sauvegardes et touches conservés). Essais : `docs/plan-essais-8x-9x.md`.
Précise ADR 0001 (un core par processus), 0005 (règle « Builtin », amendement à écrire : lot PD1), 0013 (cores
manuels, `.info`, BIOS), 0014 (catalogue, DAT, jaquettes), 0022 (pas de processus enfant par partie), 0023
(rangement). Conception et découpage : `docs/design/v8.0-systemes-personnalises.md`.
**Graphe des crates inchangé. Aucune dépendance de crate nouvelle (la sérialisation de l'enfant d'essai reste dans
`emu-ui`, qui a déjà `toml`). Schéma de `library.sqlite` inchangé (v6).** `unsafe` nouveau : `emu-libretro` (copie
de deux structures C) et `emu-runtime` (`SetErrorMode` sous Windows, §5). Aucun hôte réseau nouveau.

## Amendements après revue (2026-11-25)

Revue indépendante : `VERDICT : À AMENDER` (1 bloquant, 5 majeurs, 15 mineurs). Changements :

- **B1 (Q16)** : une définition valide en conflit avec un système officiel (même id **ou** même `database`) reste
  **active** et masque l'officiel ; la bascule est proposée dans tous les cas, reportée avec un message si elle
  exige un déplacement de dossier ; « en erreur » est réservé aux fichiers invalides ; tant que la définition est
  active, sa liste de cores prime sur `cores.toml` ; les clés de sauvegarde de `cores.toml` sont gelées par un test
  (§10). Table des ids injective, entrées futures = réduction sans suffixe (§3).
- **M1** : liste explicite des extensions de disque de la 8.0 et palier par machine dans la table des ids (§2, §9).
- **M2** : `database` et `name` en liste blanche, ids réservés, plus de valeur sentinelle dans `database`
  (accesseurs `libretro_name()` / `core_key()`), bornes de lecture des définitions (§2).
- **M3** : durcissement de l'enfant d'essai (aiguillage avant toute initialisation, aucune boîte, sorties nulles,
  résultat borné, chien de garde, core vérifié, copie du jeu, dossiers par instance, rapports de plantage) (§5).
- **M4** : le scan ne supprime **jamais** les lignes d'un système absent du catalogue ; ordre de démarrage fixé,
  modes CLI compris (§2, §13).
- **M5** et mineurs : découpage, tests et critères corrigés dans le design ; mineurs traités dans le texte (m2, m3,
  m6-m9, m11, m12, m15 ici ; m1, m4, m5, m10, m13, m14 dans le design).

## Contexte

Relevé dans le code de `wip-7.1` (2026-11-25) :

- `emu_library::systems()` rend un `&'static [SystemInfo]` lu une fois (`OnceLock`) de `data/systems.toml` embarqué ;
  `system(id)` et `systems_for_extension` rendent des `&'static SystemInfo`. 154 appels dans 30 fichiers, dont une
  cinquantaine dans `scan.rs`, `db.rs` et `tidy/` ; `SystemInfo` n'est construit que dans `systems.rs`.
- `SystemInfo::dats` est « jamais vide » ; les DAT sont téléchargés vers `<database dir>/<source>/<database>.dat` ;
  les jaquettes libretro utilisent `database` comme nom de dépôt ; une vingtaine de lectures de `.database` dans
  `emu-ui`.
- `CorePreferences` (emu-assets) associe une clé de système à une liste de cores (`cores.toml` embarqué, surcharge
  `[cores.preferred]` indexée par id de système). Les propriétés par core (`media`, `save_scope`, `game_saves`…)
  s'appliquent quel que soit le système. `emu-assets` et `emu-library` ne se connaissent pas ; `emu-ui` les relie.
- `CoreInfo` ne lit ni `systemname`, ni `categories`, ni `hw_render`, `required_hw_api`, `is_experimental`,
  `supports_no_game`, `needs_kbd_mouse_focus`. `CoreManager::infos()` rend déjà tous les `.info` du dernier
  `info.zip`.
- `emu-libretro` refuse `SET_HW_RENDER` et le journalise seulement ; il accepte `SET_INPUT_DESCRIPTORS` et **jette**
  les noms de boutons. La sentinelle `RETRO_HW_FRAME_BUFFER_VALID` est ignorée. `SET_ROTATION` n'est pas géré.
- `PadStyle::from_core_id` donne `Generic` à tout core inconnu ; la clé de profil de touches est l'id du core.
- Un plantage de core ferme l'application (ADR 0022 a écarté le processus enfant **par partie**). Les tests réels
  lancent déjà un processus enfant par core. `emu-app` sait relancer son propre exécutable (ADR 0023 §3) ; en
  sous-système graphique Windows, ses chemins d'erreur ouvrent une boîte de message.
- `DISC_EXTENSIONS` (`emu-library/src/disc.rs`) ne contient que `cue`, `chd`, `iso`.
- Le rangement (ADR 0023) crée `roms/<id>/` pour chaque système du catalogue et classe par bibliothèque, sonde,
  DAT, contenu d'archive, nom, puis extension propre à un seul système.

## Décision

### 1. Catalogue dynamique : instantanés immuables publiés à l'exécution

Le catalogue devient une valeur `Catalog` immuable (systèmes officiels embarqués, puis systèmes personnalisés).
L'application en publie un nouvel **instantané** à chaque changement ; `catalog()` rend l'instantané courant.

| Option | Effet | Coût | Verdict |
|---|---|---|---|
| (a) `Arc<Catalog>`, `Arc<SystemInfo>` partout | propre, mémoire rendue | 154 appels, 30 fichiers, durées de vie à revoir ; conflits avec tout lot parallèle | rejetée |
| (b) `&Catalog` passé explicitement à toute fonction | le plus testable | signatures de tous les crates hauts ; même coût que (a) | rejetée pour la 8.0 (évolution possible ensuite) |
| (c) catalogue lu au démarrage, redémarrage exigé après un ajout | aucun changement de type | contraire à l'ajout « en un clic » (Q4) | rejetée |
| **(d) instantanés `&'static` publiés par fuite bornée** | `systems()`, `system()`, `systems_for_extension()` **gardent leur signature** | interne à `emu-library` ; 20 à 50 Kio par publication | **retenue** |

- `install_catalog(Catalog) -> &'static Catalog` place l'instantané dans un `Box::leak` et publie le pointeur
  (`RwLock<&'static Catalog>`). Un instantané n'est jamais modifié ni libéré. **Rien n'est publié si le catalogue
  est égal au courant** ; un avertissement est journalisé au-delà de 1 000 publications dans un processus.
- **Cohérence** : scan, rangement, import des DAT et jaquettes capturent **un** instantané au départ et ne lisent que
  lui. Les méthodes publiques de `Library` (`db.rs`) lisent `catalog()` à chaque appel. La publication n'est
  **différée** que pendant un scan ou un rangement (ils écrivent des systèmes en base) : elle attend leur fin (file
  d'une publication, la dernière gagne) ; l'import des DAT et les jaquettes ne la bloquent pas.
- **Migration** : seuls les appels internes de `scan.rs`, `db.rs` et `tidy/` passent à l'instantané capturé ;
  `attribute(ext, folders)` garde sa signature en enveloppe (tests existants intacts) ; les appels d'`emu-ui` et
  d'`emu-app` ne changent pas. Livrée **seule, sans changement de comportement** (lot PB1).
- **Tests** : aucun test unitaire n'appelle `install_catalog` (dans aucun crate) ; un test qui a besoin d'un système
  personnalisé construit un `Catalog` et appelle une fonction qui le prend en paramètre, ou vit dans un fichier de
  test d'intégration seul dans son processus, cas sérialisés.

### 2. Définitions utilisateur : un fichier TOML par système sous `<data>/systems/`

- **Emplacement** : `<data>/systems/<id>.toml` (`DataDirs::custom_systems`). Pas dans `config.toml` : secrets et
  droits `0600` à part, taille bornée à 1 Mio, et un fichier par système rend l'export futur (Q15) trivial et la
  suppression atomique. Fichiers retirés : `<data>/systems/.retired/<id>-<unix>.toml`, jamais effacés.
- **Propriétaire du format** : `emu-library`. La liste des cores y figure en texte (syntaxe d'id de core vérifiée) ;
  `emu-ui` la convertit en `CoreId` et la passe à `emu-assets` (`CorePreferences::with_custom_systems`).
- **Format 1** (seule version écrite en 8.0) :

  ```toml
  format = 1                 # > 1 : « créée par une version plus récente », refusée, fichier intact
  render = "software"        # seule valeur acceptée en 8.0 ; la 9.0 ajoutera "hardware"

  [system]
  id = "pcengine"            # [a-z0-9]{1,24}, = nom du fichier, figé à la création
  name = "PC Engine"         # affiché, modifiable
  database = "NEC - PC Engine - TurboGrafx 16"   # nom libretro ; absent si le core n'en donne pas
  extensions = ["pce"]       # toutes valables SOUS roms/<id>/ SEULEMENT (Q6)
  dat = ["no-intro"]         # sources trouvées (§9), peut être vide ; no-intro, tosec, libretro en 8.0
  screenscraper = 31         # facultatif
  igdb = 86                  # facultatif

  [cores]
  list = ["mednafen_pce_fast"]        # le premier est le core par défaut
  ignored_firmware = []               # fichiers « ne plus avertir » (§6)

  [origin]
  core = "mednafen_pce_fast"
  info_name = "NEC - PC Engine / CD (Beetle PCE FAST)"
  created_by = "8.0.0"
  created_unix = 1764028800
  ```

- **Lecture bornée** : au plus 256 fichiers `*.toml` lus dans `<data>/systems/` (au-delà : ignorés, signalé) ;
  fichiers **réguliers** seulement (`symlink_metadata` : ni lien, ni FIFO, ni périphérique) ; lecture par
  `take(64 Kio + 1)`, plus grand ⇒ `TooLarge`.
- **Validation** (fichier local mais modifiable à la main : donnée non fiable) : `deny_unknown_fields` ;
  - `id` : `[a-z0-9]{1,24}`, égal au nom du fichier, ni nom réservé de Windows (`con`, `prn`, `aux`, `nul`,
    `com1`-`com9`, `lpt1`-`lpt9`), ni nom d'un dossier de `<data>` (`roms`, `system`, `saves`, `states`, `cores`,
    `cache`, `logs`, `covers`, `database`, `rangement`, `systems`, `assets`, `gameoptions`) ;
  - `name` (≤ 64) et `database` (≤ 128) : **liste blanche** — lettres, chiffres, espace, `-`, `_`, `+`, `.`, `,`,
    `'`, `&`, `!`, `(`, `)` ; jamais `/ \ : * ? " < > |`, ni `..`, ni caractère de contrôle ou bidi, ni espace ou
    point en début ou fin (le nom `database` devient un nom de fichier local sous Windows) ;
  - 1 à 64 extensions `[a-z0-9]{1,8}`, jamais une extension d'archive, de liste ou de disque de la liste 8.0 (§9) ;
  - `dat` ⊂ {`no-intro`, `tosec`, `libretro`} (Redump : 8.1) ; 1 à 8 cores, `origin.core` compris, à la syntaxe
    d'id de core ; `info_name` ≤ 128 et `created_by` ≤ 32 caractères, même nettoyage ; `ignored_firmware` ≤ 64
    chemins relatifs sûrs (copie de la règle `is_safe_relative_path` d'`emu-assets` dans `emu-library`, égalité des
    deux vérifiée par un test d'`emu-ui`) ;
  - un id ou une `database` déjà pris par un **autre** système personnalisé ⇒ erreur ; un conflit avec un système
    **officiel** n'est **pas** une erreur (§10).
- **Fichier invalide** : le système apparaît « en erreur » dans Réglages → Systèmes personnalisés (raison lisible,
  id = nom du fichier) et n'entre pas au catalogue. Ses jeux ne sont pas perdus : le scan ne touche jamais aux
  lignes d'un système absent du catalogue (§13). Dossier `<data>/systems/` illisible : idem pour tous.
- **État propre à l'utilisateur, non exportable**, dans `config.toml` `[custom_systems]` : `content_accepted`
  (cores dont l'avertissement du §7 est accepté) et `works_for_me` (paires `<id>@<core>`, §9).

### 3. Identifiant : table « nom libretro → id et palier », règle pour les versions futures

- `emu-library/data/libretro_ids.toml` (embarqué) associe chacun des ≈ 165 noms `database` relevés par l'étude à un
  **id** et un **palier** (`cartridge`, `disc`, `computer`, `arcade`). Les 17 systèmes officiels y ont **leur** id.
  La table est **injective** (test), relue par l’architecte avant le tag (les ids nomment des dossiers
  définitifs ; liste montrée à l'utilisateur, question 8.0-12) et modifiée **en ajout seulement** ; une entrée
  n'est jamais renommée.
- Réduction déterministe d'un nom : minuscules ASCII et chiffres de la partie après le fabricant, 24 caractères.
  **Règle d'ajout** : toute entrée ajoutée après la 8.0 a pour id la réduction **sans suffixe** de son nom
  (test : « entrée ajoutée = réduction ») ; les collisions de réduction entre les 165 noms sont tranchées dans la
  table dès la 8.0.
- `proposed_id(database)` : l'id de la table ; à défaut la réduction, suffixée `2`, `3`… si elle est prise.
- Un système qui a une `database` reçoit **toujours** `proposed_id` (id non modifiable). Un système sans
  `database` (31 cores) reçoit un id dérivé de son nom, modifiable à la création seulement.
- **Règle pour les versions suivantes** : un système officiel ajouté plus tard prend l'id de la table (ou la
  réduction de son nom). Le cas normal d'une bascule est donc « même id, même `database` », sans déplacement ; les
  autres cas sont traités au §10.

### 4. Règle « le `.info` propose, le core installé corrige, l'utilisateur tranche »

| Champ | Proposé par le `.info` | Corrigé par le core installé (sonde, §5) | Tranché par l'utilisateur |
|---|---|---|---|
| Nom, machine(s) | `display_name`, `systemname`, `database` (une entrée par nom, Q5) | — | nom affiché |
| Extensions | `supported_extensions` | `retro_get_system_info` **remplace** la liste | ajouter / retirer |
| Rendu matériel | `hw_render`, `required_hw_api` (filtre de la liste) | demande `SET_HW_RENDER` (refus, §8) | non |
| BIOS | `firmwareN_*`, md5 des `notes`, `firmware.toml` | — | « ne plus avertir » par fichier |
| Démarrage sans jeu, manettes, disques | drapeaux | `SET_SUPPORT_NO_GAME`, `SET_CONTROLLER_INFO` | — |
| Noms des boutons | — | `SET_INPUT_DESCRIPTORS` (§11) | remappage |

Les valeurs retenues sont **écrites dans la définition** à la création ; ensuite le fichier fait foi. Une mise à
jour du core ne change jamais une définition en silence ; le bouton « Recalculer depuis le core » rejoue la règle.

### 5. Garde-fou contre les plantages : sonde et essai à blanc en processus enfant, témoin de lancement

ADR 0022 a écarté le processus enfant **pour toute la partie** ; il n'est pas rouvert. La 8.0 ajoute un enfant
**court et sans fenêtre**, technique déjà employée par les tests.

**Enfant d'essai** : l'exécutable lui-même (`current_exe`) avec l'argument caché `--dry-run-child <requête>`.

- **Aiguillage en premier** : `emu-app` reconnaît cet argument avant toute autre initialisation (préparation des
  dossiers, configuration, fichier journal, relance avec console, `ctrlc`) et n'ouvre **jamais** de boîte de
  message : toute erreur devient un code de sortie.
- **Sorties** : `stdin`, `stdout`, `stderr` de l'enfant à `Stdio::null()` (un core bavard ne remplit rien).
- **Requête et résultat** : deux fichiers TOML dans `<data>/cache/dry-run/<pid>-<n>/`, dossier créé par
  `create_new` (droits `0700` sous Unix) avec un fichier verrou ; au démarrage, seuls les dossiers dont le verrou
  est libre (instance morte) sont nettoyés. Le résultat est lu borné (64 Kio), `deny_unknown_fields`, puis
  re-borné par le parent (mêmes bornes que les descripteurs) ; **code 0 sans résultat lisible = plantage**.
  Structures miroirs sérialisables dans `emu-ui` (`emu-runtime` et `emu-libretro` restent sans `serde`). Chemin du
  core ou du jeu non UTF-8 : pas d'essai, avertissement « non testé » seul.
- **Core vérifié** : l'enfant charge le fichier que `plan_launch` a vérifié (SHA-256, ADR 0013) et marqué « en
  usage » pendant l'essai.
- **Jeu** : copié dans le dossier de l'essai s'il fait au plus 256 Mio (l'essai ne peut pas modifier l'original,
  même pour une image de disquette) ; plus grand : pas d'essai, avertissement seul. Sauvegardes, états et copies de
  travail vont dans ce dossier, retiré ensuite : **un essai ne touche jamais les données de l'utilisateur** ;
  `system/` est le vrai (BIOS).
- **Délai et chien de garde** : le parent tue l'enfant après 30 s ; l'enfant se termine seul après 35 s (fil
  dédié), même si le parent est mort.
- **Rapports de plantage** : sous Windows, l'enfant désactive la boîte d'erreur du système (`SetErrorMode`, dans
  `emu-runtime`, `// SAFETY:`) ; sous macOS, le dialogue « a quitté de façon inattendue » ne se supprime pas :
  limite notée, vérifiée aux essais n° 7 et 9.

**Sonde** (sans jeu) : à l'ajout, après l'installation. Lit `retro_get_system_info` et les drapeaux de
l'environnement. Un plantage ⇒ ajout refusé (« ce core se ferme au chargement »).

**Essai à blanc** (Q9) : avant le **premier lancement de chaque jeu** sur un core non testé, 300 images sans
entrée ni rythme. Mémorisé dans `<data>/cache/dry-runs.toml` (≤ 2 000 entrées, les plus anciennes retirées) par
(système, core, SHA-256 du core, chemin du jeu) : une mise à jour du core refait l'essai. Plantage **ou délai
dépassé** (un core incapable de 300 images en 30 s gèlerait l'interface) ⇒ même boîte : « Changer de core »
(défaut) ou « Lancer quand même » (question 8.0-3). L'essai rapporte aussi la demande de rendu matériel et les
noms de boutons. Texte : « l'essai vérifie le démarrage seulement ».

**Témoin de lancement** (tous les cores, officiels compris) : avant `load_game`, `emu-ui` crée
`<data>/cache/launch-<pid>.tmp`, le **verrouille** (`File::lock`), y écrit core, système, jeu, « non testé », puis
le renomme en `launch-<pid>.marker` (le verrou suit le fichier ouvert) : une autre instance ne voit jamais un
témoin non verrouillé en cours de création. Retiré à la sortie propre du jeu. Au démarrage, un `.marker`
**verrouillable** est orphelin : message neutre « emu-rust s'est arrêté pendant (jeu) avec (core) — plantage, arrêt
forcé ou coupure » et, pour un core non testé, proposition de changer de core ; puis retrait. Un échec de création
du témoin ne bloque jamais un lancement (avertissement au journal).

**SRAM** : pour un système personnalisé, la mémoire de sauvegarde est comparée toutes les **60 s** à la dernière
version écrite (copie en mémoire, quelques centaines de Kio au plus) et écrite seulement si elle a changé
(`Emulator::flush_sram`, écriture atomique). Ajout de l'architecte, non demandé, au coût négligeable.

Limites assumées : l'essai à blanc n'attrape pas un plantage en cours de partie ; le test « deux chargements »
d'ADR 0022 §7 ne peut pas être fait pour un core inconnu (la copie privée automatique reste la protection).

### 6. BIOS

- Jamais téléchargés (inchangé). Liste = `firmwareN_*` du `.info`, md5 des `notes`, complétés par `firmware.toml`.
  États (ADR 0013 §8) : **valide**, **absent**, **différent**, **présent, non vérifiable** (accepté, Q10).
- **Requis absent** : avertissement avec « Lancer quand même » (Q11), plus « Ne plus avertir pour ce fichier »
  (`ignored_firmware`). Jamais de blocage : requis / facultatif est peu fiable.
- **Import** : « Importer un BIOS… » reconnaît un md5 connu ; sinon il propose les fichiers **attendus et absents**
  des systèmes personnalisés, copie sous le nom choisi (chemin relatif sûr du `.info`), état « non vérifiable ».
  Les arborescences (`Machines/` de bluemsx, données de scummvm) ne s'importent pas : « Ouvrir le dossier système ».

### 7. ROM intégrées : avertissement accepté une fois par core (amende ADR 0005, règle « Builtin »)

Un core **absent de `cores.toml`** n'a jamais été examiné. Avant son installation pour un système personnalisé (ou
son association, core déjà installé ou manuel) : « Ce core n'a pas été examiné par emu-rust ; il peut contenir des
fichiers système d'origine dont le statut n'est pas vérifié. Usage personnel uniquement. » — **Accepter**
(`content_accepted`, Q8) ou Annuler. Les cores de `cores.toml` n'affichent rien : ils servent déjà des systèmes
officiels sous la règle « Builtin » ; les deux cas « à vérifier » de l'ADR 0005 §3 (mgba, picodrive) restent
traités là (écart justifié à la remarque m12). L'ADR 0005 §3 sera amendée (lot PD1) : « … n'est pas proposé par
défaut **sans acceptation explicite de l'utilisateur, demandée une fois par core** ».

### 8. Rendu matériel : refus « prévu en 9.0 », préparé pour la 9.0

- **Avant installation** : `CoreInfo::render` structure `hw_render` et `required_hw_api` (liste d'API, version
  minimale). `hw_render = true` ⇒ grisé « Rendu matériel : prévu en 9.0 », **même sans `required_hw_api`**
  (7 cores) ; `hw_render` absent ⇒ inconnu, ajoutable (le lancement fait foi). Le filtre est **une fonction**
  (`addability`), jamais une liste de noms. Textes venus du réseau (`systemname`, API inconnue) bornés (128, 64)
  et nettoyés avant affichage.
- **Au lancement et à l'essai à blanc** (fait foi) : `emu-libretro` **mémorise** la demande `SET_HW_RENDER` et la
  refuse toujours ; `Core::hw_render_request()` l'expose. `emu-runtime` rend `RuntimeError::HardwareRenderRequired`
  si le chargement échoue après une demande, si le core envoie la sentinelle `RETRO_HW_FRAME_BUFFER_VALID`, ou si,
  après une demande, 60 images de suite arrivent sans tampon logiciel. Message : « Ce core a besoin du rendu
  matériel : prévu en 9.0 ». Un core qui se rabat sur le logiciel continue.
- **Un seul point de refus** : cette variante d'erreur et une variante de `LaunchCheck`, que la 9.0 changera en
  « accepté si l'API est disponible ». `format > 1` ou `render` inconnu : définition refusée proprement.
- Limite acceptée : un core qui sur-déclare (swanstation) est refusé bien qu'une option le ferait tourner.

### 9. Liste des cores, ajout, paliers

- **Liste** (Q3) : écran Cores → « Ajouter un système non testé… », d'après `CoreManager::infos()` et l'index de la
  plate-forme courante (téléchargés par « Vérifier les mises à jour » ; aucune requête au démarrage). Par défaut :
  catégorie « Emulator », non expérimental ; case « Tout afficher » (question 8.0-8 pour les non-émulateurs).
- **Extensions de la 8.0** (liste explicite, distincte de `DISC_EXTENSIONS`, testée sur des extraits réels de
  `.info`) : disque = `cue`, `chd`, `iso`, `ccd`, `toc`, `gdi`, `cdi`, `mds`, `mdf`, `nrg`, `pbp`, `cso`, `img`,
  `bin` ; archive = `zip`, `7z` ; liste = `m3u`, `m3u8`. **`bin` est gardé** quand le core n'accepte aucune autre
  extension de disque (cartouche Atari 2600 de stella) ; retiré sinon.
- **Ajoutable en 8.0** (`CoreInfo::addability`) : au moins une extension restante. Sinon grisé : rendu matériel ⇒
  « prévu en 9.0 » ; disque seulement ⇒ « prévu en 8.1 » ; arcade (palier `arcade` de la table, ou seule extension
  `zip`) ⇒ « prévu en 8.3 » ; aucune extension ⇒ non proposé. Ordinateur (`needs_kbd_mouse_focus` ou palier
  `computer`) : ajoutable avec « clavier complet et souris : prévus en 8.2 » (question 8.0-2).
- **Ajout en un clic** (Q4, Q5) : une case par nom `database` du core, la première cochée ; une machine de palier
  `disc` (« PC Engine CD ») d'un core mixte est **grisée « prévu en 8.1 »** (question 8.0-6). « Ajouter » enchaîne :
  avertissement du §7 si besoin, installation (gestionnaire existant), sonde, écriture des définitions, publication
  du catalogue, création de `roms/<id>/`, scan incrémental, puis « Déposez vos jeux dans `roms/<id>/` » avec
  « Ouvrir le dossier ». Une `database` d'un système officiel n'est pas proposée (« déjà pris en charge »).
- **Archives** : un `.zip` sous `roms/<id>/` dont une entrée a une extension du système est un jeu de ce système
  (règle existante du scanner) ; le lancement respecte `block_extract` et `need_fullpath` du core (existant).
- **Cores manuels** (Q12) : utilisables, étiquette « manuel, jamais vérifié » ; préremplissage par le `.info` de
  même id s'il existe, sinon par la sonde seule.
- **DAT et jaquettes par nom** (Q14) : « Chercher la base d'identification » : au plus 3 requêtes (`no-intro`,
  `tosec`, `dat/` de libretro-database ; même hôte, même code) ; les sources trouvées vont dans `dat`. Jaquettes
  libretro-thumbnails par le nom libretro, sur action ; ScreenScraper / IGDB si l'utilisateur saisit les numéros.
  Sans nom libretro : **aucune requête ni aucun fichier** de DAT ou de jaquette (test), jeu sous son nom de
  fichier. `SystemInfo::dats` peut être vide pour un système personnalisé.
- **Marque « non testé »** (Q7) : étiquette permanente sur le système, ses jeux et le core ; avertissement au
  **premier lancement de chaque jeu** de la paire (système, core), au moment de l'essai à blanc, jusqu'à « Ça marche
  chez moi » (`works_for_me`) ; l’étiquette reste (question 8.0-9). Journal et diagnostic portent la marque.
- **Suppression** : définition dans `.retired/` ; fichiers, sauvegardes, options, core et `roms/<id>/` intacts ;
  `Library::forget_system` retire **explicitement** les lignes de ses jeux et ses DAT (question 8.0-1). Recréer le
  système avec le même core retrouve les sauvegardes.

### 10. Bascule vers un système officiel (Q16)

- **Conflit** : une définition **valide** dont l'id **ou** la `database` est celle d'un système officiel **reste
  active**, l'officiel est **masqué** (absent du catalogue publié) et `CatalogLoad::pending_switches` le signale.
  Tant que la définition est active, **sa liste de cores prime** sur celle de `cores.toml` pour sa clé
  (`with_custom_systems` remplace la clé quand l'officiel est masqué) : rien ne change en silence avant la bascule.
- **Proposition** dans tous les cas : bandeau de la bibliothèque et ligne des Réglages : « emu-rust prend désormais
  en charge (système) officiellement. Basculer (recommandé) » ; jamais de bascule automatique.
- **Bascule sans déplacement** (même id, `database` égale, différente ou absente) : (1) définition dans
  `.retired/` ; (2) profil de touches `<core>@<id>` copié vers la clé officielle s'il n'existe pas ; (3) si le core
  de l'utilisateur n'est pas le premier core officiel, `[cores.preferred]` le garde en tête (ses sauvegardes,
  rangées par core, restent visibles), le core recommandé étant proposé en alternative (question 8.0-4) ;
  (4) publication, scan. Même id ⇒ même dossier, mêmes lignes : **aucun fichier déplacé**.
- **Bascule avec déplacement** (même `database`, id différent : suffixe, ou réduction devenue autre) : la 8.0 la
  **reporte** avec le message « bascule impossible sans déplacer `roms/<id perso>/` ; vos jeux restent sur votre
  définition » ; la définition reste active (question 8.0-5). La version qui rendra ce cas possible le traitera
  par le rangement (ADR 0023).
- **Sauvegardes d'un même core** : les propriétés par core de `cores.toml` qui fixent le chemin des sauvegardes
  (`media`, `save_scope`, `save_family`, `game_saves`) s'appliquent déjà au système personnalisé ; une bascule avec
  le même core ne les change donc pas. Le risque est qu'une **version future** les ajoute à un core déjà utilisé :
  un test d'`emu-assets` fige ces clés par core (instantané) ; le modifier impose à la version concernée une
  migration des sauvegardes (rangement ADR 0023), règle rappelée dans le design.
- La 8.0 livre et teste ces cas avec un catalogue officiel et un `cores.toml` synthétiques ; aucune version 8.0 ne
  déclenche de bascule réelle.

### 11. Touches : disposition générique, noms de boutons du core (Q13)

- Système personnalisé ⇒ `PadStyle::Generic` **quel que soit le core** ; clé de profil `<core>@<id>`.
- `emu-libretro` copie `SET_INPUT_DESCRIPTORS` (≤ 256 entrées, texte ≤ 64 caractères nettoyé) ;
  `Core::input_descriptors()` l'expose. Affichés dans le remappage et l'aide de touches ; gardés dans
  `<data>/cache/input-descriptors/<core>.toml` après l'essai à blanc ou une partie. Sans descripteur : A, B, X, Y…

### 12. Rangement (7.1) et glisser-déposer (7.2.1) (Q6, Q17)

- **Scan** : les extensions d'un système personnalisé ne valent que sous `roms/<id>/` ; hors de ce dossier,
  l'attribution **et le rapport de scan** (liste des non classés) sont strictement inchangés pour les fichiers des
  systèmes officiels (tests de non-régression). Seul effet visible accepté : un fichier hors dossier d'une
  extension **propre** à un système personnalisé apparaît désormais « non classé » au rapport.
- « Créer les dossiers des systèmes » crée aussi ceux des systèmes personnalisés ; le refus « dossier à créer dont le
  nom est l'id d'un système » suit le catalogue courant. Un `roms/<id>/` existant est signalé à l'ajout.
- **Rangement** : un système personnalisé n'est candidat que si l'extension n'appartient à **aucun autre système** ;
  l'unité est alors **Suggérée** (décochée, raison « système non testé ») ; sinon candidats d'aujourd'hui, sans
  système personnalisé. Une unité déjà connue garde son système.
- **Glisser-déposer (7.2.1)** : sa conception appellera le même classement, instantané pris au dépôt.

### 13. Schéma, données, ordre de démarrage

- **Aucune migration** : `game.system` et `dat_info(system, source)` sont du texte. La marque « non testé » est
  dérivée du catalogue. `SCHEMA_VERSION` reste 6.
- **Le scan ne supprime ni ne modifie jamais les lignes dont le système est absent de l'instantané** (définition
  en erreur, dossier illisible, système masqué) ; elles ne sont pas affichées. Seul `forget_system` les retire.
- **Ordre de démarrage**, interface et modes ligne de commande (`library`, `cores`) : chargement et publication du
  catalogue, **puis** reprise du journal de rangement, construction des préférences de cores, scan.
- Fichiers nouveaux sous `<data>` : `systems/` (+ `.retired/`), `cache/dry-run/`, `cache/dry-runs.toml`,
  `cache/launch-<pid>.marker`, `cache/input-descriptors/`. Écritures atomiques.

## Conséquences

- Livraison en trois temps : catalogue dynamique sans changement de comportement, définitions et garde-fous, puis
  interface. 8.1, 8.2, 8.3 et 9.0 étendent `format`, `render`, la table des paliers et `addability`.
- Fuite volontaire et bornée des instantanés (20 à 50 Kio par modification réelle).
- Un core non testé peut fermer l'application **en cours de partie** ; le témoin le dit, la SRAM limite la perte.
- Revue `reviewer` : `unsafe` des copies FFI et de `SetErrorMode` ; enfant d'essai (dès sa livraison) ; analyse des
  définitions et des `.info` ; avertissement « contenu non vérifié ».
- Assistance : un défaut sur un système personnalisé n'est pas un défaut d'emu-rust ; la marque figure partout.
- Q15 (export / import) : le format le permet déjà (ni binaire, ni URL, ni chemin absolu) ; à rappeler en 8.1.
- Non gérés en 8.0 : rotation (`SET_ROTATION`), deux écrans et stylet (DS), sous-systèmes, pistolets.
