# ADR 0025 — Version 8.0 : systèmes personnalisés (systèmes libretro non testés)

Statut : **proposé** (2026-11-25, architecte). À faire relire (revue indépendante) puis accepter par l'utilisateur.
Périmètre : `docs/specs/v7.1-v8.0-utilisateur.md` §8.0 et feuille de route (ligne 8.0) ; étude
`docs/essais-systemes-personnalises.md`, dont les réponses de l'utilisateur (§10, 2026-11-21) **font foi** : défaut
recommandé partout, sauf **Q15** (export / import d'une définition : **reporté**, à rappeler à la conception de la
8.1) et **Q16** (un système devenu officiel : la définition de l'utilisateur **reste en place**, emu-rust **propose
la bascule, recommandée** ; jeux, sauvegardes et touches conservés). Essais : `docs/plan-essais-8x-9x.md`.
Précise ADR 0001 (un core par processus), 0005 (règle « Builtin »), 0013 (cores manuels, `.info`, BIOS), 0014
(catalogue, DAT, jaquettes), 0022 (pas de processus enfant par partie), 0023 (rangement). Conception et découpage :
`docs/design/v8.0-systemes-personnalises.md`.
**Graphe des crates inchangé. Aucune dépendance nouvelle. Schéma de `library.sqlite` inchangé (v6).**
`unsafe` nouveau limité à `emu-libretro` (copie de deux structures C). Aucun hôte réseau nouveau.

## Contexte

Relevé dans le code de `wip-7.1` (2026-11-25) :

- `emu_library::systems()` rend un `&'static [SystemInfo]` lu une fois (`OnceLock`) de `data/systems.toml` embarqué ;
  `system(id)` et `systems_for_extension` rendent des `&'static SystemInfo`. **154 appels dans 30 fichiers** (dont
  66 dans `emu-library` : `scan.rs` 35, `tidy/` 22, `db.rs` 9) ; `SystemInfo` n'est construit que dans `systems.rs`.
- `SystemInfo::dats` est « jamais vide » ; les DAT sont téléchargés par source (`DatSource::repo_dir` +
  `database`) ; les jaquettes libretro utilisent `database` comme nom de dépôt.
- `CorePreferences` (emu-assets) associe un **nom de base libretro** à une liste de cores (`cores.toml` embarqué,
  surcharge `[cores.preferred]`). `emu-assets` et `emu-library` ne se connaissent pas ; `emu-ui` les relie.
- `CoreInfo` (`emu-assets/src/cores/info.rs`) ne lit ni `systemname`, ni `categories`, ni `hw_render`,
  `required_hw_api`, `is_experimental`, `supports_no_game`, `needs_kbd_mouse_focus`.
- `emu-libretro` refuse `SET_HW_RENDER` et le journalise seulement (`callbacks.rs:907`) ; il accepte
  `SET_INPUT_DESCRIPTORS` et **jette** les noms de boutons (`callbacks.rs:900`). La sentinelle
  `RETRO_HW_FRAME_BUFFER_VALID` est ignorée par le rappel vidéo. `SET_ROTATION` n'est pas géré.
- `PadStyle::from_core_id` donne `Generic` à tout core inconnu ; la clé de profil de touches est l'id du core
  (suffixée quand le système n'est pas le principal du core).
- Un plantage de core ferme l'application (ADR 0022 a écarté le processus enfant **par partie**). Les tests réels
  lancent déjà un processus enfant par core (`zz_worker`). `emu-app` sait relancer son propre exécutable
  (`current_exe`, ADR 0023 §3).
- Le rangement (ADR 0023) crée `roms/<id>/` pour chaque système du catalogue et classe par bibliothèque, sonde,
  DAT, contenu d'archive, nom, puis extension propre à un seul système.

## Décision

### 1. Catalogue dynamique : instantanés immuables publiés à l'exécution

Le catalogue devient une valeur `Catalog` immuable (systèmes officiels embarqués, puis systèmes personnalisés).
L'application en publie un nouvel **instantané** à chaque changement ; `catalog()` rend l'instantané courant.

| Option | Effet | Coût | Verdict |
|---|---|---|---|
| (a) `Arc<Catalog>`, `Arc<SystemInfo>` partout | propre, mémoire rendue | 154 appels, 30 fichiers, toutes les durées de vie à revoir ; conflits avec tout lot parallèle | rejetée |
| (b) `&Catalog` passé explicitement à toute fonction | le plus testable | signatures de tous les crates hauts ; même coût que (a) | rejetée pour la 8.0 (évolution possible ensuite) |
| (c) catalogue lu au démarrage, redémarrage exigé après un ajout | aucun changement de type | contraire à l'ajout « en un clic » (Q4) | rejetée |
| **(d) instantanés `&'static` publiés par fuite bornée** | `systems()`, `system()`, `systems_for_extension()` **gardent leur signature** | interne à `emu-library` ; quelques Kio perdus par modification | **retenue** |

- `install_catalog(Catalog) -> &'static Catalog` place l'instantané dans un `Box::leak` et publie le pointeur
  (verrou `RwLock<&'static Catalog>` ; lecture sans copie). Un instantané n'est **jamais** modifié ni libéré : toute
  référence obtenue reste valide. Coût : un catalogue (≈ 1 Kio par système) par modification de l'utilisateur ;
  un compteur journalise un avertissement au-delà de 1 000 publications dans un processus.
- **Cohérence** : toute opération longue (scan, import des DAT, rangement, jaquettes) prend **un** instantané au
  départ et ne lit que lui (`&'static Catalog` capturé dans son état) ; l'interface refuse de publier un catalogue
  pendant qu'une telle opération tourne (même règle que « rangement refusé pendant un scan », ADR 0023 §4.5).
- **Migration** : seuls les appels **internes** d'`emu-library` (≈ 66) passent à l'instantané capturé ; les appels
  d'`emu-ui` et d'`emu-app` (lecture ponctuelle dans une image de l'interface) ne changent pas. Elle est livrée
  **seule, sans changement de comportement**, avant tout le reste (lot PB1 du design).
- **Tests** : les tests unitaires n'appellent jamais `install_catalog` (état global) ; ils construisent un `Catalog`
  et appellent les fonctions internes qui le prennent en paramètre. Un fichier de test d'intégration qui publie un
  catalogue est seul dans son processus et ses cas sont sérialisés.

### 2. Définitions utilisateur : un fichier TOML par système sous `<data>/systems/`

- **Emplacement** : `<data>/systems/<id>.toml` (nouveau `DataDirs::custom_systems`). Pas dans `config.toml` : la
  config garde ses secrets et ses droits `0600`, sa taille est bornée à 1 Mio, et un fichier par système rend
  l'export futur (Q15, reporté) trivial et la suppression atomique. Les fichiers retirés vont dans
  `<data>/systems/.retired/<id>-<unix>.toml` (jamais effacés par l'application).
- **Propriétaire du format** : `emu-library` (le catalogue est à lui). La liste des cores y figure en texte
  (syntaxe d'id de core vérifiée) ; `emu-ui` la convertit en `CoreId` et la passe à `emu-assets`
  (`CorePreferences::with_custom_systems`). Aucune dépendance nouvelle entre crates.
- **Format 1** (seule version écrite en 8.0) :

  ```toml
  format = 1                 # > 1 : « créée par une version plus récente », refusée, fichier intact
  render = "software"        # seule valeur acceptée en 8.0 ; la 9.0 ajoutera "hardware"

  [system]
  id = "pcengine"            # [a-z0-9]{1,24}, = nom du fichier, figé à la création
  name = "PC Engine"         # affiché, modifiable
  database = "NEC - PC Engine - TurboGrafx 16"   # nom libretro ; absent si le core n'en donne pas
  extensions = ["pce"]       # toutes valables SOUS roms/<id>/ SEULEMENT (Q6)
  dat = ["no-intro"]         # sources trouvées (§9), peut être vide
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

- **Validation** (fichier local, mais modifiable à la main : traité comme une donnée non fiable) : taille ≤ 64 Kio,
  `deny_unknown_fields`, id valide et égal au nom du fichier, nom et `database` sans caractère de contrôle (≤ 64 et
  128 caractères, `database` sans `/`), 1 à 64 extensions `[a-z0-9]{1,8}`, **jamais** `zip`, `7z`, `m3u`, `m3u8` ni
  une extension de CD (`DISC_EXTENSIONS`, palier 8.1), 1 à 8 cores, chemins de `ignored_firmware` relatifs sûrs
  (même règle que les `.info`). Un id ou une `database` déjà pris par un **autre** système personnalisé ⇒ erreur.
- **Fichier invalide** : le système apparaît « en erreur » dans Réglages → Systèmes personnalisés (raison lisible),
  il n'entre pas au catalogue, et **le scan protège les lignes de la bibliothèque** de cet id (comme une racine
  externe débranchée) : une faute de frappe ne fait perdre ni favoris ni historique.
- **État propre à l'utilisateur, non exportable**, dans `config.toml` `[custom_systems]` :
  `content_accepted` (cores dont l'avertissement du §7 est accepté) et `works_for_me` (paires `<id>@<core>`, §5).

### 3. Identifiant : table « nom libretro → id », règle pour les versions futures

- `emu-library/data/libretro_ids.toml` (embarqué) associe chaque nom `database` connu des `.info` (≈ 165, relevés par
  l'étude) à un id. Il contient aussi les 17 systèmes officiels avec **leur** id (vérifié par test). La table n'est
  modifiée **qu'en ajout** ; une entrée n'est jamais renommée.
- `proposed_id(database)` : l'id de la table ; à défaut, une réduction déterministe du nom (minuscules ASCII et
  chiffres de la partie après le fabricant, 24 caractères), suffixée `2`, `3`… si elle est prise.
- Un système qui a une `database` reçoit **toujours** `proposed_id` : l'id n'est pas modifiable par l'utilisateur.
  Un système sans `database` (31 cores) reçoit un id dérivé de son nom, modifiable à la création seulement.
- **Règle pour les versions suivantes** : un système officiel ajouté plus tard prend l'id que donne
  `proposed_id(database)` dans la version qui l'ajoute. Ainsi un système officiel et le système personnalisé
  qu'il remplace ont **le même id** (même dossier `roms/<id>/`, mêmes lignes de la base) : la bascule du §10 ne
  déplace aucun fichier.

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
**court et sans fenêtre**, technique déjà employée par les tests :

- **Enfant** : l'exécutable lui-même (`current_exe`) avec un argument caché `--dry-run-child <requête>` (absent de
  l'aide). Requête et résultat sont deux petits fichiers TOML dans `<data>/cache/dry-run/<pid>-<n>/` ; l'enfant
  n'ouvre ni fenêtre, ni contexte GL, ni sortie audio, et écrit sauvegardes et états dans ce dossier temporaire
  (retiré ensuite) : **un essai ne touche jamais les sauvegardes de l'utilisateur**. Le dossier `system/` est le
  vrai (BIOS).
- **Sonde** (sans jeu) : à l'ajout, après l'installation du core. Charge le core, lit `retro_get_system_info`, les
  drapeaux de l'environnement. Un plantage ici ⇒ ajout refusé (« ce core se ferme au chargement »).
- **Essai à blanc** (Q9) : avant le **premier lancement de chaque jeu** sur un core non testé, 300 images sans
  entrée ni rythme. Mémorisé dans `<data>/cache/dry-runs.toml` par (système, core, SHA-256 du core, chemin du jeu) :
  une mise à jour du core refait l'essai. Délai : 30 s, puis l'enfant est tué ⇒ « essai trop long », lancement
  permis (un core lent n'est pas un core qui plante). Plantage (signal, code de sortie anormal) ⇒ boîte « Ce core
  s'est fermé pendant l'essai avec ce jeu » : « Changer de core » (défaut) ou « Lancer quand même » (question
  ouverte 8.0-3). L'essai rapporte aussi la demande de rendu matériel et les noms de boutons.
- **Témoin de lancement** (tous les cores, officiels compris) : avant `load_game`, `emu-ui` crée
  `<data>/cache/launch-<pid>.marker` (core, système, jeu, « non testé ») et le **verrouille** (`File::lock`, comme
  le verrou du rangement) ; retiré à la sortie propre du jeu. Au démarrage, un témoin **verrouillable** est
  orphelin (processus mort) : message « emu-rust s'est fermé brutalement pendant (jeu) avec (core) » et, pour un
  core non testé, proposition de changer de core ; puis le témoin est retiré.
- **SRAM** : pour un système personnalisé, la mémoire de sauvegarde est écrite toutes les **60 s** si elle a changé
  (`Emulator::flush_sram`, écriture atomique existante), pour limiter la perte en cas de plantage.
- Limites assumées : l'essai à blanc n'attrape pas un plantage en cours de partie ; le test générique « deux
  chargements » d'ADR 0022 §7 ne peut pas être fait pour un core inconnu (la copie privée automatique reste la
  protection au rechargement).

### 6. BIOS

- Jamais téléchargés (inchangé). Liste = `firmwareN_*` du `.info` du core, md5 des `notes`, complétés par
  `firmware.toml`. États existants (ADR 0013 §8) : **valide**, **absent**, **différent**, **présent, non
  vérifiable** (empreinte inconnue : accepté, Q10).
- **Requis absent** : avertissement avec « Lancer quand même » (Q11), plus « Ne plus avertir pour ce fichier »
  (ajouté à `ignored_firmware` de la définition). Jamais de blocage : requis / facultatif est peu fiable.
- **Import** : « Importer un BIOS… » reconnaît un fichier dont le md5 est connu ; sinon il propose la liste des
  fichiers **attendus et absents** des systèmes personnalisés, copie sous le nom choisi (chemin relatif sûr du
  `.info`) et le marque « non vérifiable ». Les arborescences de « firmware » (`Machines/` de bluemsx, données de
  scummvm) ne s'importent pas : « Ouvrir le dossier système » suffit.

### 7. ROM intégrées : avertissement accepté une fois par core (amende ADR 0005, règle « Builtin »)

Un core **absent de `cores.toml`** n'a jamais été examiné : emu-rust ignore s'il contient des ROM système d'origine.
Avant son installation pour un système personnalisé (ou son association, pour un core déjà installé ou manuel) :
« Ce core n'a pas été examiné par emu-rust ; il peut contenir des fichiers système d'origine dont le statut n'est
pas vérifié. Usage personnel uniquement. » — **Accepter** (mémorisé dans `content_accepted`, Q8) ou Annuler.
Les cores de `cores.toml` (examinés) n'affichent rien. emu-rust n'ouvre, n'extrait ni ne redistribue rien
(inchangé). L'ADR 0005 §3 se lit désormais : « … n'est pas proposé par défaut **sans acceptation explicite de
l'utilisateur, demandée une fois par core** ».

### 8. Rendu matériel : refus « prévu en 9.0 », préparé pour la 9.0

- **Avant installation** : `CoreInfo::render` structure `hw_render` et `required_hw_api` (liste d'API et version
  minimale, pas du texte). Un core qui annonce le rendu matériel est visible, grisé, « Rendu matériel : prévu en
  9.0 ». Le filtre est **une fonction** sur `CoreInfo` (`addability`), jamais une liste de noms.
- **Au lancement et à l'essai à blanc** (fait foi : `.info` absent ou faux) : `emu-libretro` **mémorise** la demande
  `SET_HW_RENDER` (type de contexte, version) et continue de la refuser ; `Core::hw_render_request()` l'expose par
  un type sûr. Si le chargement échoue après une demande, ou si le core envoie la sentinelle
  `RETRO_HW_FRAME_BUFFER_VALID`, `emu-runtime` rend `RuntimeError::HardwareRenderRequired` ; l'interface affiche
  « Ce core a besoin du rendu matériel : prévu en 9.0 ». Un core qui se rabat seul sur le logiciel continue.
- **Un seul point de refus** : la variante d'erreur du runtime et une variante de `LaunchCheck` ; la 9.0 les
  changera en « accepté si l'API est disponible ». `render = "software"` dans la définition ; une définition
  `format > 1` ou `render` inconnu est refusée proprement.
- Limite connue (acceptée) : un core qui sur-déclare (swanstation) est refusé bien qu'une option le ferait tourner.

### 9. Liste des cores, ajout, paliers

- **Liste** (Q3) : écran Cores → « Ajouter un système non testé… », à partir des `.info` du dernier `info.zip` et de
  l'index de la plate-forme courante (téléchargés par « Vérifier les mises à jour » ; aucune requête au démarrage,
  ADR 0013 §9). Par défaut : catégorie « Emulator », non expérimental ; case « Tout afficher ».
- **Ajoutable en 8.0** (`CoreInfo::addability`) : un core qui accepte **au moins une extension de fichier unique**
  (ni CD, ni archive, ni liste). Sinon grisé : rendu matériel ⇒ « prévu en 9.0 » ; CD seulement ⇒ « prévu en
  8.1 » ; arcade (base `MAME` / `FBNeo` / `Arcade`, ou seule extension `zip`) ⇒ « prévu en 8.3 » ; pas un
  émulateur ou aucune extension ⇒ non proposé. Un core qui annonce clavier et souris (`needs_kbd_mouse_focus`) est
  ajoutable avec l'avertissement « clavier complet et souris : prévus en 8.2 » (question 8.0-2 ; MSX en cartouche
  est au plan d'essais 8.0).
- **Ajout en un clic** (Q4, Q5) : une case par nom `database` du core, la première cochée ; « Ajouter » enchaîne
  avertissement du §7 si besoin, installation (gestionnaire existant, mêmes garanties), sonde, écriture des
  définitions, publication du catalogue, création de `roms/<id>/`, scan incrémental, puis « Déposez vos jeux dans
  `roms/<id>/` » avec « Ouvrir le dossier ». Tout se corrige ensuite dans Réglages → Systèmes personnalisés.
  Extensions retenues : celles du core moins CD, archives et listes. Une `database` déjà prise par un système
  officiel n'est pas proposée (« déjà pris en charge »).
- **Cores manuels** (Q12) : un core déposé à la main peut servir un système personnalisé, étiqueté « manuel, jamais
  vérifié » ; préremplissage par le `.info` en cache de même id s'il existe, sinon par la sonde seule.
- **DAT et jaquettes par nom** (Q14) : bouton « Chercher la base d'identification » : au plus 3 requêtes
  (`no-intro`, `tosec`, `dat/` de libretro-database, même hôte et même code que les DAT officiels ; Redump attendra
  la 8.1) ; les sources trouvées vont dans `dat`. Jaquettes libretro-thumbnails par le nom `database`, sur action,
  comme aujourd'hui ; ScreenScraper / IGDB seulement si l'utilisateur saisit les numéros. Sans `database` : ni DAT
  ni jaquettes, jeu affiché sous son nom de fichier (ADR 0014). `SystemInfo::dats` **peut être vide** pour un
  système personnalisé (invariant assoupli ; les officiels restent non vides).
- **Marque « non testé »** (Q7) : étiquette permanente sur le système, ses jeux et le core ; avertissement au
  premier lancement d'un jeu de la paire (système, core) ; bouton « Ça marche chez moi » qui retire l'avertissement
  (`works_for_me`), l'étiquette reste. Journal et rapport de diagnostic portent la marque et la définition.
- **Suppression** : la définition part dans `.retired/` ; fichiers, sauvegardes, options, core et `roms/<id>/` ne
  sont pas touchés ; les DAT importés de ce système sont oubliés (`Library::forget_system`) ; les lignes de ses jeux
  disparaissent au scan suivant (question 8.0-1). Recréer le système avec le même core retrouve les sauvegardes.

### 10. Bascule vers un système officiel (Q16)

- **Détection** (`Catalog::load`) : une définition dont l'id **et** la `database` sont ceux d'un système officiel
  (cas normal grâce au §3) **reste active** ; l'officiel est tenu en réserve, et `CatalogLoad::pending_switches` le
  signale. Une seule des deux égalités (fichier modifié à la main) ⇒ définition « en erreur » (§2).
- **Proposition** : bandeau dans la bibliothèque et ligne dans Réglages : « emu-rust prend désormais en charge
  <système> officiellement. Basculer (recommandé) » ; aucun délai, aucune bascule automatique.
- **Bascule** : (1) la définition part dans `.retired/` ; (2) profil de touches `<core>@<id>` copié vers la clé du
  profil officiel s'il n'existe pas ; (3) si le core de l'utilisateur n'est pas le premier core officiel,
  `[cores.preferred]` le garde en tête (ses sauvegardes, rangées par core, restent visibles), le core recommandé
  étant proposé comme alternative (question 8.0-4) ; (4) publication du catalogue, scan. Même id ⇒ même dossier,
  mêmes lignes (favoris, historique, jaquettes) : **aucun fichier déplacé**.
- La 8.0 livre et teste ce mécanisme avec un catalogue officiel synthétique : aucune version 8.0 ne déclenche de
  bascule réelle (une `database` officielle n'est jamais proposée à l'ajout).

### 11. Touches : disposition générique, noms de boutons du core (Q13)

- Système personnalisé ⇒ `PadStyle::Generic` **quel que soit le core** (même un core connu, genesis_plus_gx pour
  SG-1000) ; clé de profil `<core>@<id>` (deux systèmes d'un même core, bluemsx pour MSX et ColecoVision, ont deux
  profils).
- `emu-libretro` copie `SET_INPUT_DESCRIPTORS` (≤ 256 entrées, texte ≤ 64 caractères nettoyé) ;
  `Core::input_descriptors()` l'expose. L'interface les affiche dans le remappage et l'aide de touches ; ils sont
  gardés dans `<data>/cache/input-descriptors/<core>.toml` après l'essai à blanc ou une partie, donc disponibles
  hors jeu avant le premier vrai lancement. Sans descripteur : noms génériques (A, B, X, Y…).

### 12. Rangement (7.1) et glisser-déposer (7.2.1) (Q6, Q17)

- **Scan** : les extensions d'un système personnalisé ne valent **que sous `roms/<id>/`** (toutes « folder-only ») ;
  hors de ce dossier, l'attribution des fichiers aux systèmes officiels est **strictement inchangée** (test de
  non-régression sur toutes les extensions officielles, avec et sans systèmes personnalisés).
- « Créer les dossiers des systèmes » crée aussi ceux des systèmes personnalisés ; le refus « dossier à créer dont le
  nom est l'id d'un système » suit le catalogue courant. À l'ajout, un `roms/<id>/` existant est signalé avec son
  nombre de fichiers (ils deviennent des jeux de ce système).
- **Rangement** : un système personnalisé n'est candidat que si l'extension de l'unité n'appartient à **aucun autre
  système** (officiel ou personnalisé) ; l'unité est alors **Suggérée** (décochée, raison « système non testé »),
  jamais Sûre ; sinon les candidats sont ceux d'aujourd'hui, sans système personnalisé, et une unité sans candidat
  reste en place. Une unité déjà connue de la bibliothèque garde son système (inchangé).
- **Glisser-déposer (7.2.1, non conçu)** : sa conception devra appeler le même classement, avec l'instantané du
  catalogue pris au dépôt ; même règle pour les systèmes personnalisés.

### 13. Schéma et données

- **Aucune migration** : `game.system` et `dat_info(system, source)` sont du texte ; les ids personnalisés y entrent
  tels quels. La marque « non testé » est dérivée du catalogue, pas stockée. `SCHEMA_VERSION` reste 6.
- Fichiers nouveaux sous `<data>` : `systems/` (définitions, `.retired/`), `cache/dry-run/` (temporaire),
  `cache/dry-runs.toml`, `cache/launch-<pid>.marker`, `cache/input-descriptors/`. Écritures atomiques (temporaire
  puis renommage), comme `config.toml`.

## Conséquences

- La 8.0 se livre en trois temps sans casser l'existant : catalogue dynamique sans changement de comportement,
  définitions et garde-fous, puis interface. Les versions suivantes (8.1 CD, 8.2 ordinateurs, 8.3 arcade, 9.0)
  étendent le format (`format`, `render`) et `addability` sans refonte.
- Fuite mémoire volontaire et bornée des instantanés de catalogue (quelques Kio par modification).
- Un core non testé peut toujours fermer l'application **en cours de partie** ; le témoin le dit au redémarrage, la
  SRAM écrite toutes les 60 s limite la perte. L'isolation complète reste écartée (ADR 0022).
- Revue `reviewer` obligatoire : `unsafe` des deux copies (`SET_HW_RENDER`, `SET_INPUT_DESCRIPTORS`) ; lancement de
  l'enfant (arguments, fichiers de requête, dossier temporaire, délai, mise à mort) ; analyse des définitions et des
  nouveaux champs des `.info` (données non fiables) ; avertissement « contenu non vérifié » (licences).
- Assistance : un défaut sur un système personnalisé n'est pas un défaut d'emu-rust ; la marque figure partout où
  l'utilisateur rapporte un problème (journal, diagnostic).
- Q15 (export / import) : le format porte déjà `format` et ne contient ni binaire, ni URL, ni chemin absolu ; à
  rappeler à l'utilisateur à la conception de la 8.1.
- Rotation d'écran (`SET_ROTATION`), deux écrans et stylet (Nintendo DS), sous-systèmes, pistolets : non gérés en
  8.0.
