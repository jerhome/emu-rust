# Systèmes libretro non testés (« systèmes personnalisés ») — étude pour la version 8.0

Date : 2026-11-21. Périmètre : `docs/specs/v7.1-v8.0-utilisateur.md` §8.0 (« Machines compatibles libretro non
testées, gérables depuis l'application »). **Étude seulement** : aucune ADR, aucun code, aucun découpage définitif.
Les questions de la section 9 doivent être tranchées par l'utilisateur avant toute conception.

Ce qui est marqué **constaté** a été mesuré ici ; **lu** = relevé dans le code ou une ADR ; **non vérifié** =
connaissance générale, à confirmer.

**Décision déjà prise par l'utilisateur (2026-11-21)** : les cores qui exigent le rendu matériel (contexte OpenGL,
Vulkan ou Direct3D) sont pris en charge en **9.0**, pas en 8.0. En 8.0 ils sont détectés et refusés proprement
(« rendu matériel : prévu en 9.0 »). La 8.0 ne couvre que les cores à rendu logiciel.

**Accès réseau faits pour cette étude** (aucun `.info` n'était en cache local, `tests/cores/` de ce worktree est
vide) : 7 requêtes de lecture, sans donnée personnelle —
`buildbot.libretro.com/assets/frontend/info.zip` (307 865 octets, 359 `.info`),
`buildbot.libretro.com/nightly/linux/x86_64/latest/.index-extended` (261 cores),
4 listes de dossiers de `libretro/libretro-database` par l'API GitHub (`metadat/no-intro`, `metadat/redump`,
`metadat/tosec`, `dat`), et le `.gitmodules` de `libretro-thumbnails` (124 dépôts). Aucun core téléchargé, aucun
core lancé : **rien n'a été essayé en vrai** dans cette étude.

## 1. Résumé

- **Faisable, gros chantier, à livrer en paliers.** Le moteur est déjà générique sur l'essentiel : un core inconnu se
  lance aujourd'hui par `--core <chemin>` avec un mapping générique (`PadStyle::Generic`), ses options sont lues du
  core lui-même, et un fichier `*_libretro.<ext>` déposé à la main est déjà listé comme « manuel » (ADR 0013 §5).
  Ce qui manque est autour : la **bibliothèque** ne connaît que 17 systèmes compilés dans le binaire, et l'écran
  Cores ne propose que les cores de `cores.toml`.
- **Le vrai verrou technique est le catalogue statique** : `emu_library::systems()` rend un `&'static [SystemInfo]`
  lu une fois de `systems.toml` embarqué ; 90 appels dans 20 fichiers, et `&'static SystemInfo` partout dans le scan.
  Un système ajouté par l'utilisateur exige un catalogue construit à l'exécution (embarqué + fichiers utilisateur).
- **Les `.info` suffisent pour pré-remplir, pas pour faire foi.** Sur les 257 cores Linux qui ont un `.info` :
  nom, licence et catégorie toujours présents ; extensions 249 ; `database` 226 (dont 72 multi-systèmes) ; mais
  BIOS requis avec empreinte : 40 fichiers sur 76, et plusieurs champs sont faux pour des cores que nous connaissons
  (§2.3). Règle proposée : **le `.info` propose, le core installé corrige, l'utilisateur tranche**.
- **Jaquettes et identification marchent « quand le nom correspond »**, et il correspond souvent : sur les 165 noms
  `database` cités par les `.info`, 125 ont un DAT de même nom dans libretro-database et 110 un dépôt
  libretro-thumbnails.
- **Deux risques dominent** : un core qui plante tue l'application (ADR 0022 a écarté le processus enfant par jeu) ;
  et les ROM système intégrées à des cores que personne ici n'a examinés (règle « Builtin », ADR 0005).
- **Paliers proposés** : 8.0 = consoles à cartouche / fichier unique, rendu logiciel, mapping générique ;
  8.1 = CD et multi-disques ; 8.2 = ordinateurs (clavier complet, souris) ; 8.3 = arcade et entrées exotiques ;
  9.0 = rendu matériel (décidé).

## 2. Ce que fournissent les `.info`

### 2.1 Présence des champs (constaté, 257 cores `linux/x86_64` ayant un `.info`)

4 cores de l'index n'ont **aucun** `.info` (`boom3_xp`, `fbalpha`, `nicaiemu`, `pokketstation`) ; à l'inverse 102
`.info` décrivent des cores absents de l'index Linux.

| Champ | Présent | Remarques |
|---|---|---|
| `display_name`, `license`, `categories` | 257 | `categories` : 211 « Emulator », 24 « Game », 13 « Game engine », 9 autres (lecteur vidéo, karaoké, utilitaire…) |
| `systemname` | 252 | 161 valeurs distinctes ; texte libre (« Sega 8/16-bit (Various) », « Arcade (various) ») |
| `systemid` | 244 | 142 valeurs ; **12 identifiants désignent plusieurs `systemname`** (`mega_drive` : 3, `msx` : 3) |
| `supported_extensions` | 249 | 8 cores sans extension (jeux autonomes) ; valeurs parasites : `/`, `.`, `C4`, `V32`, `fs-uae`, `qemu_cmd_line` |
| `database` | 226 | 165 noms distincts ; **72 cores en citent plusieurs** (séparés par `\|`) ; 21 émulateurs logiciels n'en ont pas |
| `needs_fullpath` | 180 | absent pour 60 des 182 émulateurs logiciels |
| `firmware_count` | 146 (123 > 0) | compte faux pour 2 cores (`hyperscanemu`, `mednafen_saturn`) |
| `hw_render` | 192 (38 à `true`) | |
| `required_hw_api` | 31 | texte libre : « OpenGL Core >= 3.3 », « Vulkan >= 1.0 \| Direct3D >= 10.0 \| … » |
| `supports_no_game` | 255 (62 à `true`) | |
| `disk_control` | 185 (46 à `true`) | 49 cores acceptent `.m3u`, 53 une image de CD (`cue` / `chd` / `iso` / `ccd`) |
| `needs_kbd_mouse_focus` | 13 (11 à `true`) | **absent pour hatari, puae, vice** : ne permet pas de reconnaître un ordinateur |
| `is_experimental` | 144 (24 à `true`) | |
| `load_subsystem` | 169 (24 à `true`) | sous-systèmes (Super Game Boy, Sufami Turbo…) : non gérés par emu-rust |
| `savestate` | 231 | 171 `true`, 59 `false` |
| `core_options`, `input_descriptors` | 189, 182 | simples drapeaux : **la liste des options n'est pas dans le `.info`**, seul le core la donne |

Ce que lit aujourd'hui `emu_assets::cores::parse_info` (lu, `crates/emu-assets/src/cores/info.rs`) :
`display_name`, `corename`, `display_version`, `license`, `supported_extensions`, `database`, `firmwareN_*`,
md5 des `notes`, `description`. **Pas lus** : `systemname`, `systemid`, `categories`, `hw_render`,
`required_hw_api`, `is_experimental`, `supports_no_game`, `disk_control`, `needs_kbd_mouse_focus`, `savestate`.

### 2.2 BIOS (`firmware*`)

Constaté : 122 cores déclarent 778 fichiers. **179 seulement (23 %) ont un md5** dans `notes`. Parmi les 76
fichiers marqués requis (39 cores), **40 ont un md5**. 45 cores ont un md5 pour tous leurs fichiers.
`crates/emu-assets/data/firmware.toml` complète avec des sections choisies de `System.dat` (lu) ; il ne couvre que
les systèmes déjà livrés. La couverture de `System.dat` entier pour les autres systèmes est **non vérifiée**.

Beaucoup de « firmwares » ne sont pas des BIOS : scummvm déclare des thèmes et des fichiers de données, fbneo des
romsets `.zip` d'arcade, bluemsx toute une arborescence `Machines/` et `Databases/`.

### 2.3 Fiabilité, mesurée sur les cores que nous connaissons

Comparaison du `.info` du jour avec ce que les versions 1.0 à 7.0 ont établi par essais (`cores.toml`, ADR) :

| Core | Ce que dit le `.info` | Ce que nous savons |
|---|---|---|
| swanstation | `hw_render = true`, `required_hw_api = "OpenGL >= 3.0 \| …"` | tourne en rendu logiciel (`swanstation_GPU_Renderer = "Software"`, ADR 0021) : **le champ sur-déclare** |
| genesis_plus_gx, picodrive | les 12 / 3 BIOS sont tous facultatifs | le BIOS Mega CD est **requis** pour un CD (ADR 0020) |
| mednafen_psx | `scph5500/5501/5502.bin` requis | le core démarre sans, sur OpenBIOS intégré (ADR 0021) |
| puae | 4 Kickstart requis | AROS est intégré, aucun n'est requis (ADR 0017) |
| mednafen_psx | pas d'`iso` dans les extensions | le binaire l'accepte (commentaire de `cores.toml`) |
| hatari, puae, vice_x64 | `needs_kbd_mouse_focus` absent | ce sont des ordinateurs à clavier complet |
| gambatte, sameboy, mesen-s | `needs_fullpath`, `hw_render`, `disk_control` absents | — |
| genesis_plus_gx | `systemname = "Sega 8/16-bit (Various)"`, 6 `database` | 4 systèmes distincts chez nous |

Conclusion : `required` / `optional` des BIOS et `required_hw_api` sont **indicatifs**. `database`, la licence et les
extensions sont globalement justes mais incomplets.

### 2.4 Ce que le core installé dit mieux que son `.info`

Une fois le core téléchargé, emu-rust lit déjà de lui (lu, `emu-libretro`) : les extensions réelles,
`need_fullpath` et `block_extract` (`retro_get_system_info`), le démarrage sans contenu
(`SET_SUPPORT_NO_GAME`), les types de manettes par port (`SET_CONTROLLER_INFO`), l'interface de disques
(`SET_DISK_CONTROL_*`), le rappel clavier (`SET_KEYBOARD_CALLBACK`), toutes les options, et la demande de rendu
matériel (`SET_HW_RENDER`, aujourd'hui refusée et seulement journalisée, `callbacks.rs:907`). Les noms des boutons
(`SET_INPUT_DESCRIPTORS`) sont acceptés mais **jetés** (`callbacks.rs:900`).

Répartition proposée :

- **avant installation** (choisir dans la liste) : `.info` — nom, machine, licence, catégorie, expérimental, BIOS
  annoncés, rendu matériel annoncé ;
- **après installation** : le core fait foi pour extensions, chemin complet, manettes, disques, options, clavier ;
- **jamais fourni par personne** : mapping agréable, sondes d'en-tête, règles de CD, options conseillées. C'est
  précisément ce que « non testé » veut dire.

## 3. Ce qu'emu-rust code en dur par système

| Sujet | Où (lu) | Aujourd'hui | Défaut pour un système inconnu | Réglable par l'utilisateur |
|---|---|---|---|---|
| Liste des systèmes | `emu-library/data/systems.toml`, `systems.rs` (`OnceLock`, `&'static`) | 17 systèmes embarqués ; `id` en minuscules et chiffres seulement | définition utilisateur chargée à l'exécution | nom affiché ; l'`id` est figé à la création (il nomme le dossier et les lignes de la base) |
| Dossier | règle « dossier nommé d'après l'`id` » (`systems.rs::attribute`) | `roms/<id>/` | `roms/<id>/`, **seule** règle d'attribution (§5.6) | non |
| Extensions | `systems.toml` `extensions`, `folder_extensions` ; `zip` réservé au scanner | par système | celles du core, nettoyées (minuscules, alphanumériques), toutes « sous le dossier seulement » | ajouter / retirer |
| En-têtes à retirer | `HeaderRule::{INes, SnesCopier}` | NES, SNES | aucune (hash du fichier entier) | non en 8.0 |
| Sondes d'identification | `disc.rs` (`SEGADISCSYSTEM`, `SEGA 32X` à 0x100, `PLAYSTATION` secteur 16, 3DO), `disc.rs:49-51` | Mega CD, 32X, PS1, 3DO | aucune : le dossier décide | non |
| DAT | `DatSource` (4 dossiers de libretro-database), `dat` par système | choisi à la main par système | essayer `<database>.dat` dans no-intro, redump, tosec, `dat/`, à la demande | choisir le nom libretro dans une liste ; « aucun » |
| CD | `disc`, `group_discs`, règles Redump (série, piste 01) | Mega CD, PS1, 3DO | palier 8.1 : image jamais hachée, identification par nom | case « système à CD » |
| Jeux-dossiers | `folder_games` (DOS) | DOS | non | non |
| Jaquettes | `database` = dossier libretro-thumbnails ; `screenscraper`, `igdb` (numéros) | les 17 | libretro-thumbnails si le nom existe ; ScreenScraper / IGDB désactivés (numéro inconnu) | saisir les numéros |
| Cores du système | `emu-assets/data/cores.toml` `[systems]`, surcharge `[cores.preferred]` | 17 clés | le core choisi à l'ajout | changer, ajouter une alternative |
| Style de manette, noms des boutons | `emu-ui/src/keyboard.rs::PadStyle::from_core_id` / `from_extension` (listes de cores et d'extensions) | 9 styles | `PadStyle::Generic` (disposition SNES + L2/R2/L3/R3, `generic_default`) ; noms tirés de `SET_INPUT_DESCRIPTORS` | remappage clavier et manette, déjà par profil de core |
| Type de manette (analogique…) | `cores.toml` `pad_devices`, `analog` | PS1, DOS | manette simple (`RETRO_DEVICE_JOYPAD`) | liste des types annoncés par le core |
| Ordinateur ou console | `SystemKind`, `keyboard.rs::is_computer_profile` (9 cores nommés), `app.rs::keyboard_layout_id` (repli silencieux `atari_st`) | 5 ordinateurs | console | palier 8.2 : case « ordinateur » |
| Clavier virtuel, touches | `emu-ui/data/keyboards/{atari_st,amiga,pc,cpc,c64}.toml`, `vkbd.rs:89` | 5 dispositions | palier 8.2 : disposition PC générique | non |
| Mode d'entrée par défaut | `systems.toml` `default_input` | CPC, C64 = joystick | clavier pour un ordinateur | oui |
| Bureau sans jeu | `library/home.rs:107-116` (4 boutons nommés) | ST, Amiga, CPC, C64 | palier 8.2, si le core annonce `SUPPORT_NO_GAME` | — |
| BIOS | `.info` + `firmware.toml` + `cores.toml` (`required_firmware`, `optional_firmware`, `builtin_firmware`) ; `launch_firmware.rs:195` | par core et par système | liste du `.info`, états existants (valide, absent, différent, **non vérifiable**) ; avertissement non bloquant (ADR 0013 §8) | marquer un fichier « pas nécessaire » |
| Options par défaut | `cores.toml` `options`, `options_by_extension`, `featured_options`, `layout_option` | par core | celles du core, aucune mise en avant | toutes, par jeu ou par core (existe) |
| Copie de travail, sauvegardes | `cores.toml` `media`, `media_direct`, `game_saves`, `save_scope`, `save_family` | par core | `direct`, portée par core ; « Réinitialiser » ne supprime que `.srm` et états | palier 8.2 : « copie de travail » |
| Divers par core | `vfs`, `fastforward_override`, `variable_state_size`, `m3u_launch`, `actions`, `system_caches`, `folders` | par core | tout désactivé | non |
| Rendu matériel | `callbacks.rs:907` | toujours refusé, journalisé | refus net, « prévu en 9.0 » (§5.1) | non |
| Zip mélangé | `scan.rs:310` `AMBIGUOUS_COMPUTERS` | CPC / C64 | sans objet (dossier seul) | — |

Lecture d'ensemble : **`cores.toml` porte une vingtaine de clés trouvées une à une par des essais**. Pour un core
inconnu, toutes prennent leur valeur par défaut, ce qui est le comportement de RetroArch sans réglage : correct pour
une console à cartouche, souvent insuffisant pour un ordinateur (disquette modifiée en place, clavier, lancement).

## 4. Parcours utilisateur envisagé

1. **Écran Cores → « Ajouter un système non testé… »**. La liste complète du buildbot s'affiche après « Vérifier les
   mises à jour » (aucune requête au démarrage, ADR 0013 §9 ; `info.zip` est déjà téléchargé à ce moment). Recherche
   par nom de machine ; par défaut seuls les émulateurs de machines, rendu logiciel, non expérimentaux. Les cores à
   rendu matériel sont visibles, grisés, « prévu en 9.0 ».
2. **Fiche du core** : machine(s), licence, BIOS annoncés, avertissements (expérimental, ROM intégrées inconnues,
   pas d'états sauvegardés).
3. **Installation** par le gestionnaire existant (mêmes garanties : buildbot officiel, CRC32 de l'index, SHA-256
   local). Rien de nouveau côté réseau, donc pas de nouvelle surface pour `reviewer` en dehors de la liste élargie.
4. **Système pré-rempli** : une entrée par nom `database` du core (l'utilisateur coche celles qu'il veut) ; nom,
   identifiant proposé, extensions, DAT et jaquettes « trouvés / non trouvés », BIOS. Tout est modifiable ensuite
   dans Réglages → Systèmes personnalisés.
5. **Dossier** `roms/<id>/` créé ; bouton « Ouvrir le dossier » (existe depuis la 3.0). L'utilisateur y dépose ses
   jeux ; le scan les attribue par la règle de dossier.
6. **Marque « non testé »** sur le système, ses jeux et le core ; avertissement au premier lancement.
7. **Export / import** d'une définition : un fichier texte (TOML) qui ne contient que des données — nom,
   identifiant, extensions, `database`, core(s), touches, options. **Jamais** de binaire, d'URL ni de chemin absolu ;
   à l'import, mêmes validations qu'un `.info` venu du réseau (`is_safe_relative_path`, tailles bornées), et le core
   est installé par le gestionnaire, pas par le fichier.

Emplacement des définitions (à décider par ADR) : un fichier par système sous `<data>/systems/` plutôt que dans
`config.toml` (export = copie du fichier ; la config garde ses droits `600` et ses secrets à part).

## 5. Risques

### 5.1 Rendu matériel (décidé : 9.0)

Constaté : 38 cores Linux ont `hw_render = true`, dont 31 avec `required_hw_api`. Des machines entières n'ont
**que** des cores de ce type : Dreamcast / Naomi, GameCube / Wii, 3DS, DSi, PSP, PS3, Xbox, Vectrex (`vecx`),
Model 3.

En 8.0 :

- **avant installation** : `required_hw_api` non vide ⇒ core non proposé à l'ajout, libellé « rendu matériel :
  prévu en 9.0 » ;
- **au lancement** (fait foi, le `.info` pouvant manquer ou se tromper — 4 cores sans `.info`, cores « manuels ») :
  le core appelle `SET_HW_RENDER`, emu-rust refuse ; si le chargement échoue ensuite, message explicite au lieu de
  l'échec générique actuel. Un core qui se rabat seul sur le logiciel continue normalement.
- Limite connue : un core sur-déclaré (cas swanstation, §2.3) est refusé alors qu'une option le ferait tourner en
  logiciel. Accepté pour la 8.0 ; ces cores arrivent en 9.0.

**Ce que la 8.0 doit préparer pour que la 9.0 s'y branche sans refonte :**

1. `CoreInfo` lit `hw_render` et `required_hw_api` et les garde **structurés** (liste d'API + version minimale),
   pas en texte.
2. La définition d'un système et son format d'export portent dès la 8.0 un champ de rendu (`software` en 8.0) et un
   numéro de version de format : une définition 9.0 importée dans une 8.x est refusée proprement.
3. `emu-libretro` **mémorise** la demande `SET_HW_RENDER` (type de contexte, version) au lieu de seulement la
   journaliser, et la remonte par un type public sûr ; le refus devient **un seul point** (`LaunchCheck` côté UI,
   une variante d'erreur côté runtime) que la 9.0 transformera en « accepté si l'API est disponible ».
4. Le filtre « prévu en 9.0 » de la liste est une fonction unique sur `CoreInfo`, pas une liste de noms.
5. Rien de la 8.0 ne doit supposer « image = tampon logiciel » en dehors d'`emu-runtime` (déjà vrai : ADR 0008).

### 5.2 Cores qui plantent

Lu (ADR 0022) : un core s'exécute dans le processus de l'application ; un plantage (SIGSEGV) **ferme emu-rust**. Le
processus enfant par jeu a été écarté (fenêtre, contexte GL, audio et entrées à refaire). Les tests réels utilisent
en revanche un processus enfant par core. Avec 158 émulateurs logiciels jamais essayés ici (et 24 cores de l'index
marqués expérimentaux), le plantage devient un cas normal. Pistes, de la moins chère à la plus chère :

| Piste | Effet | Coût |
|---|---|---|
| Témoin de lancement : fichier écrit avant `retro_load_game`, effacé à la sortie propre | au redémarrage : « le core X a fermé emu-rust avec le jeu Y », proposition de changer de core | faible |
| **Essai à blanc** : avant le premier lancement d'un jeu sur un core non testé, un processus enfant sans fenêtre charge le core et le jeu et fait tourner ~300 images (technique des tests, `zz_worker`) | attrape les plantages au chargement sans fermer l'application ; quelques secondes d'attente | moyen (emu-runtime, emu-app) |
| Processus enfant pour toute la partie | isole tout | très élevé ; rouvre ADR 0008 / 0010 / 0022 |

L'essai à blanc n'attrape pas un plantage en cours de partie. Dans tous les cas, écrire la SRAM plus souvent pour
les cores non testés limite la perte (à étudier). Le test générique « deux chargements successifs » d'ADR 0022 §7 ne
peut pas être exécuté par nous pour des cores inconnus : la copie privée automatique reste la seule protection au
rechargement.

### 5.3 Entrées exotiques

Lu : emu-rust annonce manette, souris et clavier (`callbacks.rs:779`) ; **ni pistolet optique ni pointeur**
(`RETRO_DEVICE_LIGHTGUN`, `RETRO_DEVICE_POINTER` : constantes seulement). Stylet de la DS, pistolets, paddles,
trackballs, claviers numériques (ColecoVision, Intellivision) : injouables ou partiellement jouables en 8.0. Le
clavier complet et la souris capturée existent mais sont réservés aux 9 cores nommés d'`is_computer_profile`.
Sous-systèmes (`load_subsystem`, 24 cores) : non gérés.

### 5.4 Multi-disques et CD

Lu : l'interface de disques est générique dans `emu-libretro` ; le regroupement `(Disk N of M)` / `(Disc N)`, les
`.m3u` générés, la copie de travail et les exceptions par core (`m3u_launch`, `group_discs = false` pour opera)
viennent d'essais. Pour un système inconnu : `.m3u` passé tel quel si le core l'accepte, sinon un jeu par disque.
Les images de CD partagent leurs extensions entre 22 à 24 familles de systèmes (constaté : `chd` 24, `cue` 23,
`iso` 22, `bin` 45) : hors du dossier du système, aucune attribution n'est possible. D'où le palier 8.1.

### 5.5 BIOS

- Jamais téléchargés (inchangé). Le `.info` dit quoi déposer et où ; l'import existant (« Importer un BIOS… »)
  vérifie le md5 **quand il est connu**.
- Sans empreinte (77 % des fichiers) : l'état « présent, non vérifiable » existe déjà (ADR 0013 §8). L'import par
  recherche d'empreinte ne peut pas reconnaître ces fichiers : l'utilisateur doit les nommer lui-même.
- Requis / facultatif peu fiables (§2.3) ⇒ avertissement avec « lancer quand même », jamais de blocage.
- **ROM intégrées aux cores** : ADR 0005 (amendement du 2026-11-21, §3) dit qu'un core dont les ROM intégrées n'ont
  ni licence libre, ni permission, ni acceptation explicite de l'utilisateur n'est pas proposé par défaut. Pour un
  core inconnu, emu-rust **ne sait pas** ce qu'il contient. Il faut une décision de l'utilisateur (question 8).

### 5.6 Stabilité des noms

- `systemid` des `.info` est inutilisable comme identifiant : absent 13 fois, partagé entre machines différentes,
  avec des `_` que le catalogue refuse.
- Le nom `database` est la clé la plus stable (DAT, jaquettes, `cores.toml`), mais 31 cores n'en ont pas et
  l'amont renomme parfois (**non vérifié**). `System.dat` nomme d'ailleurs le 3DO autrement que les `.info`
  (commentaire de `firmware.toml`).
- L'`id` d'emu-rust est persistant (base, dossiers, sauvegardes) et « ne se renomme jamais » (`systems.toml`).
  Collision à prévoir : l'utilisateur crée `pcengine`, puis une version future livre un `pcengine` officiel.
  Piste : embarquer une table « nom `database` → identifiant conseillé » pour la centaine de machines connues, de
  sorte que l'identifiant proposé soit celui qu'un futur système officiel porterait ; le système officiel remplace
  alors la définition sans déplacer un seul fichier.
- Un core retiré ou renommé sur le buildbot : le core installé reste utilisable (existant), sans mise à jour.

### 5.7 Rangement (7.1) et glisser-déposer (7.2.1)

Lu (ADR 0023 §4) : « Créer les dossiers des systèmes » crée `roms/<id>/` pour chaque système du catalogue ; le
rangement classe par système connu de la bibliothèque, sonde, DAT puis extension ; une unité inconnue reste en place ;
un dossier à créer dont le nom est l'`id` d'un système est refusé.

- Un système personnalisé qui réclamerait `bin`, `rom`, `zip` ou `iso` partout **détournerait** des jeux de
  systèmes testés. Règle proposée : ses extensions ne valent **que sous son dossier** (`folder_extensions` existe
  pour cela) ; le rangement ne propose un système personnalisé que si l'extension n'appartient à aucun autre
  système, sinon « inconnu, laissé en place ».
- Le catalogue devenant dynamique, ajouter un système change la règle de dossier du scan : un nouveau scan est
  nécessaire, et un dossier existant qui porterait déjà ce nom devient un dossier de système (à signaler à
  l'ajout).
- Supprimer un système personnalisé : ses jeux redeviennent « non classés », ses fichiers ne sont jamais touchés ;
  sauvegardes et options sont conservées (elles sont rangées par core).
- Glisser-déposer : même classement. Un fichier déposé d'extension ambiguë ne peut pas être rangé seul ; demander
  le système de destination à ce moment-là est à trancher avec la conception 7.2.1.

### 5.8 Autres

- **Sécurité** : exécuter un core non testé reste « du code natif du buildbot officiel » (risque déjà accepté,
  ADR 0013). Les cores « manuels » existent déjà ; les associer à un système personnalisé n'ajoute pas de risque
  technique mais sort du buildbot (question 12).
- **Catégories non-émulateur** (46 cores : jeux, moteurs, lecteurs) : hors de l'esprit « machines », certains sans
  contenu ni extension.
- **Arcade** (MAME, FBNeo) : romsets `.zip` passés tels quels (déjà le cas si le core liste `zip`), BIOS dans le
  dossier des ROM, aucun DAT de même nom dans libretro-database (constaté), nom du fichier = nom du jeu.
- **Assistance** : un défaut sur un système non testé n'est pas un défaut d'emu-rust ; le journal et le rapport de
  diagnostic doivent porter la marque « non testé » et la définition.

## 6. Identification et jaquettes « quand le nom correspond »

Constaté, sur les 165 noms `database` : 125 ont au moins un DAT de même nom (no-intro 59, redump 23, tosec 33,
`dat/` 50 ; 36 dans plusieurs dossiers) ; 110 ont un dépôt libretro-thumbnails ; 108 ont les deux. Sans DAT : les
MAME, FBNeo, les jeux autonomes, quelques machines rares. Sans jaquettes : en plus, Apple II, Jaguar CD, FM Towns,
PICO-8, ZX Spectrum +3.

- Trouver le bon dossier demande aujourd'hui de connaître la source (`dat = "tosec"`). Pour un système inconnu :
  essayer les 4 dossiers à la demande (4 requêtes au plus, sur action de l'utilisateur), mémoriser le résultat.
- Les DAT No-Intro et `dat/` s'importent tels quels (CRC32 / SHA1). Les DAT Redump demandent pour bien faire une
  sonde par système (série) : en 8.1, repli sur la taille + CRC32 de la piste 01 ou le nom.
- Sans DAT ni jaquette : jeu affiché sous son nom de fichier, jouable (comportement existant, ADR 0014).

## 7. Paliers proposés (non définitifs)

| Palier | Contenu | Ce que l'utilisateur obtient |
|---|---|---|
| **8.0** | Catalogue dynamique ; liste complète du buildbot ; ajout pré-rempli depuis le `.info` ; `roms/<id>/` ; consoles à **cartouche / fichier unique**, rendu logiciel ; mapping générique avec noms de boutons du core ; BIOS listés et vérifiés quand l'empreinte existe ; DAT et jaquettes par nom ; marque « non testé » ; garde-fou contre les plantages ; refus « prévu en 9.0 » ; export / import | Atari 2600 / 7800 / Lynx, PC Engine (HuCard), Neo Geo Pocket, WonderSwan, ColecoVision, Virtual Boy, SG-1000, MSX en cartouche… |
| **8.1** | Systèmes à CD et multi-disques : `.m3u`, changement de disque, regroupement, Redump générique | PC Engine CD, Neo Geo CD, Saturn (Beetle Saturn, logiciel), PC-FX, CD-i |
| **8.2** | Ordinateurs : clavier complet et clavier virtuel génériques, souris, démarrage sans jeu, copie de travail réglable | ZX Spectrum, MSX sur disquette, Atari 8 bits, X68000, PC-88 / 98, Thomson, VIC-20 / Plus-4 |
| **8.3** | Arcade (romsets, BIOS à côté des ROM), pistolet / pointeur à la souris, sous-systèmes ; isolation renforcée si retenue | MAME, FBNeo, jeux à pistolet |
| **9.0** | Rendu matériel (OpenGL, DirectX, Vulkan) — décidé | Dreamcast, GameCube, PSP, cores N64 principaux… et systèmes personnalisés à rendu matériel |

Les machines citées sont celles que les `.info` rattachent à un core sans rendu matériel ; **aucune n'a été
essayée**.

### Estimation grossière par crate, pour la 8.0 seule

| Crate / agent | Travail | Taille |
|---|---|---|
| `emu-library` (`library-assets`) | catalogue dynamique (fin du `&'static`, 90 appels dans 20 fichiers tous crates confondus), définitions utilisateur validées, scan et rangement avec systèmes ajoutés, DAT multi-dossiers | **gros** |
| `emu-assets` (`library-assets`) | `CoreInfo` étendu, liste complète filtrable, recherche du DAT et des jaquettes par nom, format de définition (export / import) ; pas de nouveau code réseau hors URL de DAT | moyen |
| `emu-libretro` (`libretro-ffi`) | mémoriser `SET_HW_RENDER` et `SET_INPUT_DESCRIPTORS`, types publics sûrs | petit |
| `emu-runtime` (`runtime-av`) | erreur « rendu matériel », essai à blanc en processus enfant, défauts génériques | petit à moyen |
| `emu-ui`, `emu-app` (`ui`) | liste et fiche des cores, assistant d'ajout, éditeur de système, marques « non testé », mapping générique nommé, export / import, messages | **gros** |
| `qa-systems` | variantes du core factice (rendu matériel demandé, plantage, descripteurs), un ou deux vrais cores libres avec homebrew | moyen |
| `reviewer` | `unsafe` nouveau (descripteurs, `SET_HW_RENDER`), import de définitions (données non fiables) | petit |

Ordre probable : contrat (catalogue dynamique + format de définition) → `emu-library` et `emu-libretro` en
parallèle → `emu-assets` → `emu-ui`. `emu-library` et `emu-assets` relèvent du même agent : à sérialiser. Le
catalogue dynamique touche tous les crates en lecture : c'est la tâche à isoler et à livrer en premier, sans
changement de comportement.

## 8. Points ouverts pour la conception

- Schéma de la bibliothèque : `game.system` est déjà du texte libre ; une migration n'est pas certaine (à vérifier).
- Où vit la table « core → système personnalisé » : `cores.toml` embarqué ne peut pas l'accueillir ; surcouche
  utilisateur de `CorePreferences` (`with_overrides` existe pour l'ordre des cores seulement).
- Noms de boutons : `SET_INPUT_DESCRIPTORS` arrive après le chargement du jeu ; l'écran de remappage hors jeu n'aura
  des noms qu'après un premier lancement.
- macOS Intel et Windows : la liste des cores diffère par plate-forme (seul l'index Linux a été lu ici).
- Portée d'une définition exportée entre plates-formes (un core absent de l'index de l'autre machine).
- Couverture de `System.dat` complet pour les empreintes de BIOS manquantes.

## 9. Questions pour l'utilisateur

Chaque question a un défaut recommandé (**en gras**). Aucune ne porte sur le rendu matériel : c'est décidé (9.0).

1. **Par où commencer ?** La 8.0 serait livrée en étapes. Quel ordre ?
   - a) **Consoles à cartouche d'abord (8.0), puis CD (8.1), puis ordinateurs (8.2), puis arcade (8.3).**
   - b) Ordinateurs avant les CD.
   - c) Arcade (MAME / FBNeo) en premier.
   - d) Tout d'un coup, une seule grosse version.

2. **Quelles machines avez-vous en tête ?** (plusieurs réponses possibles ; sert à choisir les essais)
   - a) PC Engine / TurboGrafx. b) Atari 2600 / 7800 / Lynx. c) Neo Geo, arcade. d) MSX, ZX Spectrum, autres
     ordinateurs. e) Saturn. f) Nintendo 64, Dreamcast, PSP (ce sera la 9.0). g) **Aucune en particulier : le mécanisme
     général.**

3. **Que montrer dans la liste des cores à ajouter ?** Le serveur en propose environ 260, dont des jeux autonomes,
   des lecteurs vidéo et des cores marqués « expérimentaux ».
   - a) **Seulement les émulateurs de machines, non expérimentaux ; une case « tout afficher » pour le reste.**
   - b) Tout, sans filtre.
   - c) Seulement les émulateurs, sans possibilité de voir le reste.

4. **Comment se passe l'ajout ?**
   - a) **Un clic : tout est pré-rempli, vous corrigez ensuite si besoin dans les réglages.**
   - b) Un assistant en plusieurs écrans qui vous fait tout valider (nom, dossier, extensions, BIOS, touches).

5. **Un core qui émule plusieurs machines** (exemple : PC Engine, SuperGrafx et PC Engine CD).
   - a) **Une entrée par machine, vous cochez celles que vous voulez (la principale est cochée).**
   - b) Toujours une seule entrée pour tout le core.
   - c) Toutes les machines ajoutées d'office.

6. **Où mettre les jeux d'un système ajouté ?**
   - a) **Uniquement dans son dossier `roms/<nom>/` : un fichier ailleurs n'est jamais attribué à un système non testé.**
   - b) Aussi reconnu partout par son extension quand elle n'appartient à aucun autre système.

7. **Comment signaler « non testé » ?**
   - a) **Une étiquette discrète permanente, un avertissement au premier lancement, et un bouton « ça marche chez moi »
     qui retire l'avertissement (l'étiquette reste).**
   - b) Un avertissement à chaque lancement.
   - c) L'étiquette seule, jamais d'avertissement.

8. **Fichiers système intégrés à un core inconnu.** Certains cores contiennent des ROM système d'origine dont nous
   n'avons pas vérifié le statut (vous aviez accepté le cas du Commodore 64 une par une).
   - a) **Un message à chaque core non testé ajouté : « contenu non vérifié, usage personnel », à accepter une fois
     pour ce core.**
   - b) Une acceptation générale, une seule fois pour tous.
   - c) Aucun message.

9. **Si un core non testé plante**, emu-rust se ferme avec lui. Quelle protection ?
   - a) **Essai à blanc de quelques secondes avant le premier lancement de chaque jeu, plus un message au redémarrage
     après un plantage (« tel core a fermé l'application »).**
   - b) Seulement le message au redémarrage.
   - c) Rien de particulier.

10. **BIOS que l'application ne sait pas vérifier** (empreinte inconnue : environ trois fichiers sur quatre).
    - a) **Accepté, affiché « présent, non vérifiable ».**
    - b) Refusé tant qu'aucune empreinte n'est connue.

11. **BIOS annoncé « requis » mais absent.** L'information du serveur est souvent fausse.
    - a) **Avertir, avec un bouton « lancer quand même ».**
    - b) Bloquer le lancement.

12. **Cores déposés à la main** (fichier qui ne vient pas du serveur officiel) : utilisables pour un système ajouté ?
    - a) **Oui, avec l'étiquette « manuel, jamais vérifié ».**
    - b) Non : serveur officiel uniquement.

13. **Touches par défaut d'un système ajouté.**
    - a) **Une disposition générique unique, avec les noms de boutons donnés par le core ; remappage libre ensuite.**
    - b) Choisir un modèle à l'ajout (« comme la Super Nintendo », « comme la Mega Drive », « comme la PlayStation »).

14. **Jaquettes et noms des jeux.**
    - a) **Recherche automatique quand le nom de la machine est connu de libretro, sur votre action (bouton), comme
      aujourd'hui ; sinon nom du fichier.**
    - b) Vous choisissez vous-même la machine correspondante dans une liste à l'ajout.
    - c) Pas de jaquettes pour les systèmes non testés.

15. **Partager une définition de système** (fichier texte : nom, dossier, extensions, core, touches, options).
    - a) **Export et import par fichier dès la 8.0 ; jamais de téléchargement de définitions.**
    - b) Plus tard.

16. **Le jour où emu-rust prend officiellement en charge une machine que vous aviez ajoutée.**
    - a) **La version officielle remplace la vôtre ; vos jeux, sauvegardes et réglages de touches sont conservés.**
    - b) La vôtre reste en place ; emu-rust propose la bascule.

17. **Rangement automatique (7.1) et glisser-déposer (7.2.1)** d'un fichier qui pourrait appartenir à un système
    ajouté.
    - a) **Proposé seulement si son extension n'appartient à aucun autre système ; sinon laissé en place.**
    - b) Toujours demander le système de destination.
    - c) Jamais : les systèmes ajoutés se rangent à la main.

## 10. Réponses de l'utilisateur (2026-11-21, questionnaire)

Défaut recommandé retenu pour toutes les questions, sauf deux :

- **Q15, partage d'une définition** : **plus tard**, pas en 8.0. L'utilisateur demande qu'on **le lui rappelle**, par
  exemple au moment de concevoir la 8.1.
- **Q16, prise en charge officielle d'une machine déjà ajoutée** : la définition de l'utilisateur **reste en place**.
  emu-rust **propose la bascule vers la version officielle, en indiquant qu'elle est recommandée**. Jeux, sauvegardes et
  touches sont conservés lors de la bascule.

| Question | Réponse retenue |
|---|---|
| Q1 | Ordre : cartouches (8.0), CD (8.1), ordinateurs (8.2), arcade (8.3) |
| Q2 | Aucune machine en particulier : le mécanisme général |
| Q3 | Liste : émulateurs non expérimentaux, avec la case « tout afficher » |
| Q4 | Ajout en un clic, tout pré-rempli, corrigeable ensuite |
| Q5 | Core multi-machines : une entrée par machine, la principale cochée |
| Q6 | Jeux uniquement dans `roms/<nom>/` |
| Q7 | Étiquette « non testé » permanente, avertissement au premier lancement, bouton « ça marche chez moi » |
| Q8 | ROM intégrées : message « contenu non vérifié, usage personnel », accepté une fois par core |
| Q9 | Essai à blanc avant le premier lancement de chaque jeu, et message au redémarrage après un plantage |
| Q10 | BIOS sans empreinte connue : accepté, affiché « présent, non vérifiable » |
| Q11 | BIOS requis absent : avertissement avec « lancer quand même » |
| Q12 | Cores manuels acceptés, avec l'étiquette « manuel, jamais vérifié » |
| Q13 | Touches : disposition générique, noms de boutons donnés par le core |
| Q14 | Jaquettes : recherche sur action si le nom libretro est connu, sinon nom du fichier |
| Q17 | Rangement et glisser-déposer : proposés seulement si l'extension n'appartient à aucun autre système |
