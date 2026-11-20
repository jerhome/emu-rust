# ADR 0017 — Amiga (version 3.0) : core puae, AROS intégré, Kickstart importé, clavier Amiga, dossier des ROMs

Statut : **accepté** (2026-11-21) — re-validée contre le code de la 2.0.0-rc.2, contrat posé dans le code (tâche X0 ;
écarts et précisions : design §10). Décisions utilisateur du 2026-11-21 intégrées (revue croisée des conceptions,
corrections B1, B2, B3, I1, I5, I9, I10, M3, M7, M10 ; **core `puae` par défaut sur tous les OS, `puae2021`
sélectionnable en un clic et signalé « recommandé sur Mac »** ; **Kickstart absent ⇒ AROS intégré + avertissement
unique par jeu**). Décisions utilisateur (`docs/questions-ouvertes.md`, elles prévalent) : touche hôte
**Ctrl droit partout** (macOS compris ; ⌘ droit seulement en repli réglable) ; **Amiga gauche / droite = Alt gauche /
Alt droit (AltGr)** sur les trois OS, les touches Alt de l'Amiga passant par le clavier virtuel ; mode d'entrée par
défaut **Clavier**. Étude de faisabilité : `docs/essais-puae.md` (essais headless de puae et puae2021 sous Xvfb, sans
Kickstart). Réutilise ADR 0015 (infrastructure « ordinateur » de la 2.0) sans changement d'architecture ; précise
ADR 0005 (AROS intégré au core : amendement du 2026-11-21), 0013 (BIOS fournis par l'utilisateur), 0014 (DAT, schéma
de la bibliothèque). Conception détaillée : `docs/design/v3.0-amiga.md` — c'est elle qui définit `MediaRules`,
l'ordre d'évaluation des médias, le contrôle de firmware unique et les mécanismes génériques réutilisés par les
versions suivantes (§3.5 du design). Graphe des crates inchangé.

## Contexte

La 3.0 ajoute le Commodore Amiga en réutilisant tout ce que la 2.0 pose pour l'Atari ST : clavier complet et touche
hôte (Ctrl droit partout, macOS compris), souris capturée, clavier virtuel egui, disquettes multiples et copies de
travail, options par jeu, BIOS libre / fourni avec vérification. L'utilisateur joue au clavier Razer 60 % AZERTY
(Windows 11 et macOS Intel 12.7.6), surtout à des jeux **point-and-click à la souris** (mode *Clavier* par défaut,
décision 2.0). Deux demandes de l'utilisateur sont rattachées à la 3.0 : un bouton **« Ouvrir le dossier des ROMs »**
sur l'écran principal, et un bouton **« Importer un BIOS… »** (sélecteur de fichier natif, vérification du hash, copie
sous le nom attendu par le core).

Faits vérifiés le 2026-11-20 :

- **Buildbot** (`.index-extended` de https://buildbot.libretro.com/nightly/{linux/x86_64,windows/x86_64,apple/osx/x86_64}/latest/) :
  `puae_libretro` et `puae2021_libretro` présents sur les trois (nightly 2026-11-20) ; `fsuae_libretro` sur Linux
  seulement (écarté).
- **`puae`** (https://github.com/libretro/libretro-uae, commit `d366f77` du 2026-11-09, GPL-2.0, « Based on WinUAE
  5.3.1 ») : `.info` (`assets/frontend/info.zip`) `display_version = "5.3.1"`, `supports_no_game`, `disk_control`,
  `needs_fullpath`, extensions `adf|adz|dms|fdi|ipf|hdf|hdz|lha|slave|info|cue|ccd|nrg|mds|iso|chd|uae|m3u|zip|7z|rp9|/`.
  Binaire (`libretro/libretro-core.c`) : `block_extract = true` (lit lui-même zip et 7z, extraits dans `saves/`,
  **écritures perdues** à la sortie), `SET_KEYBOARD_CALLBACK` **et** interrogation `RETRO_DEVICE_KEYBOARD`,
  `SET_DISK_CONTROL_EXT_INTERFACE` si la version d'interface ≥ 1 (sinon 13), `set_initial_image = NULL`,
  `SET_SUPPORT_NO_GAME`, `SET_CONTROLLER_INFO` (ports 1-2 « joyport », 3-4 parallèle, 5-6 clavier).
  Souris : `RETRO_DEVICE_MOUSE` du port libretro 0 → port souris Amiga, port 1 → second port (option
  `puae_physicalmouse = enabled|double`). Écrit **dans les images ADF** par défaut
  (`puae_floppy_write_protection = disabled`, `puae_floppy_write_redirect = disabled` ; avec la redirection,
  `<saves>/<nom>_save.adf`, espace de noms plat). IPF toujours en lecture seule et **lu seulement avec `capsimg`**
  (bibliothèque externe, `system/capsimg.{so,dll}`), absente par défaut. `puae_floppy_multidrive = enabled` par défaut :
  jusqu'à 4 disquettes réparties dans DF0-DF3 (si le jeu gère les lecteurs externes). `.m3u` : chemins relatifs ou
  absolus, `fichier|libellé`, `#SAVEDISK:<label>` (disquette de sauvegarde créée dans `saves/`), `(MD)` dans le nom.
  WHDLoad 20.1 intégré : une archive `.lha` Retroplay se lance directement (fichiers auxiliaires dans `saves/`,
  `WHDLoad.prefs` dans `system/`).
- **`puae2021`** : même dépôt, branche figée sur PUAE 2.6.1, « less accurate version more suitable for weaker
  devices » (`.info`). Mêmes options et noms de fichiers.
- **Kickstart** : `puae_kickstart = auto | aros | <fichier de system/>` (liste remplie au démarrage avec les fichiers
  commençant par `kick`, `amiga-os`, `KS ROM`). En `auto`, le modèle (`puae_model`, `auto` = A500 pour une disquette,
  A1200 pour un disque dur / WHDLoad, tags de chemin `(A1200)`, `AGA`, `(A500+)`… : README du core) donne le nom
  attendu, cherché dans `system/` puis `system/Devs/Kickstarts/` ; noms Amiga Forever (`amiga-os-130.rom`) et TOSEC aussi acceptés ; ROM chiffrées Amiga Forever avec
  `system/rom.key`. Fichier absent ⇒ message du core « Kickstart ROM '…' not found! » et **repli sur l'AROS intégré**.
  Noms et md5 (`.info`, champ `notes`, identiques dans le README) :

  | Modèle (preset puae) | Fichier attendu | Version | Taille | md5 |
  |---|---|---|---|---|
  | A500, A500OG, A500ECS, A2000OG | `kick34005.A500` | KS 1.3 r34.005 | 262 144 | `82a21c1890cae844b3df741f2762d48d` |
  | (A500 1.2, optionnel) | `kick33180.A500` | KS 1.2 r33.180 | 262 144 | `85ad74194e87c08904327de1a9443b7a` |
  | A500+ | `kick37175.A500` | KS 2.04 r37.175 | 524 288 | `dc10d7bdd1b6f450773dfb558477c230` |
  | A600 (2.05, optionnel) | `kick37350.A600` | KS 2.05 r37.350 | 524 288 | `465646c9b6729f77eea5314d1f057951` |
  | A600, A600OG, A2000 | `kick40063.A600` | KS 3.1 r40.063 | 524 288 | `e40a5dfb3d017ba8779faba30cbd1c8e` |
  | A1200 (3.0, optionnel) | `kick39106.A1200` | KS 3.0 r39.106 | 524 288 | `b7cc148386aa631136f510cd29e42fc3` |
  | A1200, A1200OG | `kick40068.A1200` | KS 3.1 r40.068 | 524 288 | `646773759326fbac3b2311fd8c8793ee` |
  | A4030, A4040 | `kick40068.A4000` | KS 3.1 r40.068 | 524 288 | `9bdedde6a4f33555b4a270c8ca53297d` |

  (CDTV / CD32 : hors 3.0.) Le `.info` marque `kick34005.A500` et `kick40068.A1200` **non optionnels** alors que le
  core démarre sans (AROS) : le contrôle 1.0 « BIOS requis absent » ne doit pas s'appliquer tel quel.
- **Essais** (`docs/essais-puae.md`, Linux, sans Kickstart) : démarrage, 50 images/s PAL, 0 sous-alimentation audio ;
  géométrie variable en cours de partie (360×287 → 720×287 → 720×574 entrelacé), déjà suivie par le runtime
  (`SET_GEOMETRY` / `SET_SYSTEM_AV_INFO`, `av_info_changed`) ; adf, adz, zip et m3u (DF0 + DF1) vérifiés ; empreintes
  déterministes ; environnement demandé en plus de la 2.0 : 28 `GET_PERF_INTERFACE`, 64
  `SET_FASTFORWARDING_OVERRIDE`, 45 `GET_VFS_INTERFACE`, 46 `GET_LED_INTERFACE` (refus sans conséquence) ; le core
  écrit sur **stdout** des lignes `retro_vfs_file_open_impl …` (bruit, hors de notre contrôle).
- **AROS** : licence AROS Public License 1.1 (fondée sur la MPL, https://aros.sourceforge.io/license.html).
  puae **embarque** une ROM AROS m68k (`sources/src/aros.rom.c`, ROM + extension 1 Mio, SHA-256 extrait
  `1211897caea79785f2441da4b75a460255b7f85e5129e03fe0ab61a8aebfb2c1`, bibliothèques AROS du 2022-01-26 ; la mise à
  jour de 2023 a été annulée en 2025, commit `f6a2270` ; elle se présente comme « Kickstart 3.1.4 (46.12) »), sélectionnée par `puae_kickstart = aros` ou en
  repli. En amont, l'AROS m68k n'existe **qu'en nightly** sur SourceForge
  (https://aros.sourceforge.io/nightly1.html → `nightly2/<date>/Binaries/AROS-<date>-amiga-m68k-boot-iso.zip`,
  131 Mo ; seul le zip ISO contient la ROM, le `boot-floppy.lha` de 772 Ko ne contient qu'un ADF et la licence) ; les
  dossiers nightly tournent en quelques jours (2026-11-20 : 20261116, 20261118, 20261120 et un 20241001 vide) ; aucune
  release GitHub (`aros-development-team/AROS`). **Aucune version stable téléchargeable à hash épinglé.**
- **Base de données** (`libretro-database`) : `metadat/tosec/Commodore - Amiga.dat` (13 Mo, 44 057 jeux, 40 160
  `.adf`, 3 783 `.ipf`, multi-disques `(Disk N of M)`), `metadat/no-intro/Commodore - Amiga.dat` (IPF seulement),
  `dat/Commodore - Amiga.dat` (« Commodore - Amiga - WHDLoad », archives `.lha` Retroplay, crc + sha1). Vignettes :
  https://github.com/libretro-thumbnails/Commodore_-_Amiga.
- **Dialogues natifs** (crates.io, 2026-11-20) : `rfd` 0.18.0 (MIT ; Windows : `IFileDialog` ; macOS : `NSOpenPanel`
  via `objc2` ; Linux : portail XDG par `libdbus-1.so.3` chargé **à l'exécution** (aucune dépendance de compilation)
  puis repli `zenity` ; MSRV 1.92) ; `opener` 0.9.0 (MIT OR Apache-2.0 ; Windows `ShellExecuteW`, macOS `open`,
  Linux script `xdg-open` embarqué ; **WSL détecté** ⇒ `explorer.exe` avec `wslpath -w` ; fonction `reveal` derrière
  une feature `zbus`, non retenue).
- **Données de test libres** : Amiga Test Kit 1.21 (Unlicense, https://github.com/keirf/amiga-stuff/releases/tag/testkit-v1.21,
  zip SHA-256 `0c609dc991394ba9f1831496e61ce40c74993a581d8a20541ba291d601fcb959`, `AmigaTestKit.adf` SHA-256
  `abe7426c93619a7bb61ce10e3e66a4747fcaf22acd1d1876310033faa700ad28`, disquette amorçable sans OS : tests clavier,
  souris, ports joystick, lecteurs).

## Décision

### 1. Core

- Système `Commodore - Amiga` dans `emu-assets/data/cores.toml` : **`puae`** par défaut **sur tous les OS**,
  **`puae2021`** en alternative (GPL-2.0 tous deux). puae suit WinUAE 5.3.1 (précision, AGA, WHDLoad 20.1 récent) ;
  puae2021 est plus léger. **Décision utilisateur du 2026-11-21** : pas de bascule automatique selon l'OS ; l'écran
  Cores offre un sélecteur « Core utilisé » par système (il écrit `[cores.preferred]`) et y signale puae2021 comme
  **« recommandé sur Mac »** (donnée `recommended_on = ["macos"]`, mécanisme générique réutilisé en 7.0). `fsuae`
  écarté (Linux seulement).
- Défauts frontend (`[cores.puae] options`, même liste pour puae2021) : `puae_mapper_select = "---"` (le clavier
  virtuel est celui du frontend, ADR 0015 §7), `puae_mapper_mouse_toggle = "---"` (Ctrl droit est la touche hôte ;
  si l'utilisateur en change, Ctrl droit doit atteindre l'Amiga comme Amiga droite, pas basculer le pad),
  `puae_physicalmouse = "enabled"`, `puae_floppy_write_redirect = "disabled"` (copies de travail, §6). Le reste garde
  les défauts du core (modèle `auto`, MultiDrive activé, PAL).
- **Pas de `capsimg` en 3.0** : les `.ipf` sont reconnus par la bibliothèque mais le lancement affiche « IPF : placez
  `capsimg` dans le dossier système (licence Software Preservation Society, non redistribuable par emu-rust) » ;
  l'import de `capsimg` par le bouton « Importer un BIOS… » est hors 3.0 (question de conception §9).

### 2. Kickstart fourni par l'utilisateur, AROS intégré en repli

- **AROS = celui intégré à puae.** Pas d'entrée `aros` dans `free_bios.toml` en 3.0 : il n'existe en amont qu'une
  nightly de 131 Mo qui disparaît en quelques jours, donc pas de version figée ni de SHA-256 épinglable (condition
  d'ADR 0015 §2), et l'AROS intégré, redistribué par le buildbot avec le core, couvre déjà le besoin « jouer sans
  ROM propriétaire ». ADR 0005 est amendée en ce sens (2026-11-21 : AROS = intégré à puae, règle « Builtin » ;
  emu-rust ne télécharge ni n'installe aucun fichier AROS) ; la phrase d'ADR 0015 (Conséquences) « ajouter `aros` à
  `free_bios.toml` » est caduque. Le `FreeBiosManager`
  reste prêt : une entrée `aros` s'ajoutera par simple fichier de données si une archive stable apparaît.
- **Priorité Kickstart > AROS**, calculée par le frontend avant le lancement (pas de message d'erreur du core) :
  1. modèle effectif = option du jeu, sinon `puae_model` global, sinon résolution de `auto` (fonction pure
     `resolve_amiga_model` : disquette ⇒ `puae_model_fd` (défaut A500), `.hdf`/`.lha` ⇒ `puae_model_hd` (défaut A1200),
     puis tags du chemin, table du README du core) ;
  2. **contrôle de firmware unique** (design §3.5.5, posé en 3.0 pour tous les systèmes) : table
     `required_firmware` de `cores.toml`, ici **par variante = modèle** (Kickstart attendu pour ce modèle, §Contexte),
     cherché dans `system/` sous le nom puae, Amiga Forever ou TOSEC ; état `FirmwareState` habituel (md5) ;
  3. présent et valide ⇒ `puae_kickstart = "auto"` ; **absent** ⇒ **repli déclaré** (`fallback` de la table) :
     `puae_kickstart = "aros"` passé au core comme **défaut calculé** (design §3.5.4) et état
     **`FirmwareState::CoreFallback { label: "AROS (intégré au core)" }`** (jamais « manquant » ni bloquant, même
     pour les Kickstarts marqués non optionnels par le `.info`) ; le message du core « Kickstart ROM … not found! »
     ne peut alors plus apparaître (chemin `aros` sans recherche de fichier) ; s'il apparaît quand même (lancement
     `--core` sans bibliothèque), l'OSD le remplace par « Kickstart absent : AROS intégré utilisé » ; hash différent
     ⇒ dialogue 1.0 (« lancer quand même » / « utiliser AROS »).
- **Avertissement de compatibilité AROS**, une fois par jeu (mémorisé dans les options du jeu) : « Pas de Kickstart
  pour A500 : AROS est utilisé. Beaucoup de jeux Amiga ne fonctionnent pas sans Kickstart d'origine. [Importer un
  Kickstart…] [Continuer avec AROS] ». AROS est un remplaçant d'AmigaOS 3.1 : les démos et jeux qui frappent le
  matériel en direct marchent souvent, ceux qui dépendent de Kickstart 1.3 (bootblocks, trackloaders) souvent pas ;
  WHDLoad exige un vrai Kickstart (les copies auto de puae ne concernent que les fichiers présents dans `system/`).
- Aucun Kickstart n'est téléchargé (ADR 0005) ; la vérification utilise uniquement des md5 : ceux de
  `firmware.toml` (section Amiga tirée de `libretro-database/dat/System.dat`, même licence CC-BY-SA-4.0 et même
  attribution que l'existant) et, pour les fichiers absents de `System.dat`, ceux du `.info` du core installé, lus à
  l'exécution et **jamais recopiés dans nos données** (format et attribution : design §3.5.6).

### 3. Import d'un BIOS (demande utilisateur, tous systèmes)

- Bouton **« Importer un BIOS… »** dans l'écran Cores (ligne BIOS d'un core, et en tête de l'écran) et dans le
  dialogue « Kickstart / TOS manquant ». Sélection du fichier (§8) ⇒ `emu_assets::bios::identify_firmware` (taille
  ≤ 16 Mio, md5 + sha1 calculés en flux) contre le catalogue réuni des md5 connus : `firmware.toml` + `.info` des
  cores installés (`notes` « (!) <chemin> (md5): … ») + BIOS libres.
- **Fichier reconnu** ⇒ installé sous **chaque nom attendu** par un core installé ou par le catalogue pour ce md5
  (ex. `amiga-os-130.rom` ⇒ `system/kick34005.A500` ; un BIOS Game Boy ⇒ `gb_bios.bin` et `dmg_boot.bin` si les deux
  sont attendus), copie temporaire + `sync_all` + renommage atomique, jamais d'écriture hors de `system/` (noms tirés
  du catalogue, validés : pas de `..`, pas de séparateur sauf sous-dossiers déclarés par un `.info`).
- **Fichier existant** à ce nom : même hash ⇒ rien à faire ; autre fichier ⇒ **jamais écrasé en silence** : dialogue
  « Remplacer ? l'ancien sera renommé `<nom>.user-<unix>` » (même politique que `tos.img`, `ReplacePolicy`) ; un BIOS
  libre installé par emu-rust (manifeste) est remplacé sans renommage (le Kickstart prend sa place, EmuTOS reste
  réinstallable).
- **Fichier inconnu** ⇒ par défaut **refus** avec explication (md5 affiché, liste des fichiers attendus) et un bouton
  secondaire « Importer quand même (non vérifié)… » : l'utilisateur choisit le nom cible parmi les fichiers attendus
  par les cores installés ; avertissement « hash inconnu : le core peut échouer ». Le manifeste
  `system/.emu-rust-bios.toml` gagne une table `[imported."<nom>"]` (`source` = nom de fichier d'origine sans chemin,
  `md5`, `verified`) ; l'état reste `WrongHash` / `Unverified` comme pour un fichier copié à la main.
- **ROM Amiga Forever chiffrée** (en-tête `AMIROMTYPE1`) : reconnue comme telle, importée non vérifiée sous le nom
  Amiga Forever (`amiga-os-130.rom`…) qu'accepte puae, avec le message « copiez aussi `rom.key` » ; `rom.key` est
  importable de la même façon (nom fixe).
- Jamais de téléchargement ; aucun chemin d'origine n'est journalisé en dehors du niveau `debug` (vie privée).

### 4. Modèles et options par jeu

- Défaut : **`auto`** (A500 KS 1.3 512 Ko chip + 512 Ko slow pour les disquettes ; A1200 KS 3.1 2 Mo chip + 8 Mo fast
  pour WHDLoad). Le menu « Options de ce jeu » (ADR 0015 §8) montre en tête **Modèle** (A500, A500+, A600, A1200,
  A4000 — libellés du core), **Kickstart** (auto / AROS / fichiers présents), **Vidéo** (PAL / NTSC), **MultiDrive**,
  puis toutes les options du core ; changement de modèle ⇒ « Redémarrer ». Aucune option nouvelle côté frontend :
  `puae_model`, `puae_kickstart`, `puae_chipmem_size`, `puae_fastmem_size`, `puae_cpu_model`, `puae_video_standard`
  sont des options du core, stockées dans `[core_options]` du jeu.
- Les tags de chemin du core (`(A1200)`, `AGA`, `NTSC`, `(MD)`) restent actifs : le frontend ne renomme rien.

### 5. Formats

| Format | Bibliothèque | Lancement | Politique |
|---|---|---|---|
| `.adf`, `.adz`, `.dms` | oui (TOSEC) | natif | copie de travail (§6) |
| `.ipf` | oui (TOSEC, No-Intro) | natif si `capsimg` présent | direct (toujours en lecture seule) |
| `.fdi` | oui | natif | copie de travail |
| `.m3u` | oui (groupe) | `.m3u` utilisateur relu ou généré (2.0) ; `#SAVEDISK:` conservé tel quel | copie de travail |
| `.lha` (WHDLoad) | oui (DAT WHDLoad) | natif (WHDLoad intégré au core) | direct (jamais écrit ; sauvegardes dans `saves/`) |
| `.hdf`, `.hdz` | oui | natif | copie de travail, limite 2 Gio ; disque plein ⇒ copie partielle supprimée, erreur claire |
| `.zip` | oui (contenu lu sans extraction) | images de disquette ⇒ extraites par le frontend dans la copie de travail (règles 2.0) ; sinon (installation WHDLoad décompressée) passé tel quel au core | selon le contenu |
| `.7z`, `.rp9`, `.uae`, CD (`.cue`, `.iso`, `.chd`…), `.slave`, `.info`, dossier | non | `--rom` seulement, passé tel quel | direct, sans garantie (écritures perdues pour `.7z`) |

### 6. Disquettes (DF0-DF3) et écritures

- Politique `cores.toml` : `[cores.puae] media = "working_copy"`, **`media_direct = ["ipf", "lha"]`** (nouveau champ :
  extensions jamais écrites par le core, transmises sans copie ; **un seul nom partout** : clé `media_direct` de
  `cores.toml`, `CorePreferences::media_direct`, champ `MediaRules::media_direct`). Les fichiers de l'utilisateur ne sont jamais modifiés
  (règle 2.0) ; les écritures (sauvegardes de jeu sur disquette) vivent dans `saves/puae/<jeu>.disks/`.
- `#SAVEDISK:` : puae crée la disquette de sauvegarde dans **son** dossier `saves/` (`<m3u>.save<N>.adf`) : comme
  le `.m3u` généré ou copié porte le nom du jeu, le nom est unique par jeu. « Réinitialiser les disquettes »
  supprime aussi ces fichiers par le mécanisme **générique** posé en 3.0 : motifs `game_saves` de `cores.toml`
  (`["{game}.save*.adf"]`) appliqués par `reset_core_game_files` (design §3.5.3 ; réutilisé par dosbox_pure, cap32,
  VICE, genesis_plus_gx).
- **MultiDrive** (défaut du core) : jusqu'à 4 disquettes insérées en DF0-DF3 au démarrage ; l'interface libretro ne
  pilote que **DF0** (pas d'API par lecteur) : le menu Disquettes et Hôte + D changent DF0, comme en 2.0. Libellé de
  l'OSD : « DF0 : disquette 2/3 — … ». Un jeu qui refuse les lecteurs externes : option MultiDrive par jeu.
- Zips : puae a `block_extract = 1` et sait les lire, mais il les décompresse dans `saves/puae/TEMP/` vidé à la
  sortie : **les écritures sur disquette seraient perdues**. Le frontend extrait donc lui-même les zips de disquettes
  dans `<jeu>.disks/` et passe au core des `.adf` / un `.m3u` ; un zip sans image de disquette est transmis tel quel
  (le core le traite comme un dossier). **Ce n'est pas le code de la 2.0** : `prepare_media` n'extrait aujourd'hui un
  zip que si le core **ne déclare pas** l'extension `zip` (`crates/emu-runtime/src/media.rs`, `add_source`), or puae
  la déclare. La 3.0 remplace ce test par un **ordre d'évaluation unique**, écrit une seule fois dans le design
  (§3.5.1) : (1) extension de `media_direct` ⇒ transmise en place ; (2) zip ⇒ inspecté, extrait s'il contient une image
  acceptée ; (3) sinon politique du core. Les ADR 0018, 0019 et 0020 y renvoient. Le chemin 1.0
  (`ContentSpec::File`, zip transmis si `block_extract`) reste celui d'un `--rom` sans bibliothèque.
- `set_initial_image` n'existe pas dans puae : `last_disk` est restauré par `Disks::insert` après le chargement
  (déjà le chemin 2.0).

### 7. Clavier Amiga, souris, joystick

- Correspondance positionnelle 2.0 inchangée (`KeyCode` → `RETROK_*` US) ; puae traduit `RETROK_*` → touches Amiga
  (`libretro/libretro-mapper.h`). Touches propres à l'Amiga et correspondance par défaut :

  | Touche Amiga | `RETROK_*` lu par puae | Windows / Linux (Razer 60 %) | macOS |
  |---|---|---|---|
  | Amiga gauche | `LSUPER` (aussi `LMETA`, `PAGEUP`) | **Alt gauche** (`AltLeft`) — décision utilisateur | **⌥ gauche** |
  | Amiga droite | `RSUPER` (aussi `RCTRL`, `RMETA`, `PAGEDOWN`) | **Alt droit / AltGr** (`AltRight`) — décision utilisateur | **⌥ droit** |
  | Help | `INSERT` ou `HELP` | Inser (Fn) ou clavier virtuel | clavier virtuel |
  | Del | `DELETE` | Suppr (Fn + ⌫ sur le Razer) | Fn + ⌫ |
  | Ctrl | `LCTRL` | Ctrl gauche | Ctrl |
  | Alt gauche / droite de l'Amiga | `LALT` / `RALT` | **clavier virtuel seulement** (aucune touche physique par défaut) | **clavier virtuel seulement** |
  | `< >` (ISO) | `LESS` (puae lit aussi `OEM_102`) | touche `<` (`IntlBackslash` ⇒ `LESS` par la table 2.0, inchangée) | `IntlBackslash` |
  | `#` (pavé ISO) | `HASH` | — (clavier virtuel ; LCtrl + `\` = touche de sortie WHDLoad, géré par le core) | idem |
  | F1–F10, pavé `( ) / *` | `F1…F10`, `HOME`/`END` = `(`/`)` | Fn + rangée chiffres, ou clavier virtuel | idem |

  Ces correspondances sont des **surcharges par disposition** (`emu-ui/data/keyboards/amiga.toml`, table `[remap]`
  **unique, identique sur Windows, Linux et macOS** : `AltLeft = "LSUPER"`, `AltRight = "RSUPER"`) appliquées après
  la table positionnelle, modifiables dans `config.toml` (`[computer.remap.amiga]`). `LALT` / `RALT` (les Alt de
  l'Amiga) ne sont émis que par le **clavier virtuel**. La touche Windows / ⌘ n'est liée à rien. La touche hôte n'est
  jamais transmise : Ctrl droit (→ Amiga droite chez puae) n'atteint pas le core.
- **Alt côté hôte** : le `ControlLeft` factice qui précède AltGr sous Windows reste filtré (2.0). Les combinaisons
  **Alt + F4, Alt + Tab et Alt + Espace restent à Windows** (elles n'arrivent pas, ou pas entièrement, à l'Amiga) :
  signalé dans le guide d'essai ; les jeux n'utilisent pas Amiga + F4 / Tab / Espace, sinon clavier virtuel.
  **Alt + Entrée** (plein écran 1.0) est **inactif en mode ordinateur** (vérifié dans `crates/emu-ui/src/app.rs` :
  en mode ordinateur toutes les touches passent par le routeur de la touche hôte, le raccourci Alt + Entrée n'est
  évalué qu'en mode console) : Alt + Entrée arrive à l'Amiga comme Amiga gauche + Return, le plein écran est
  Hôte + F. Un test U3 fige ce comportement. Le reset clavier « Ctrl + Amiga + Amiga » reste physiquement possible
  (Ctrl gauche + Alt gauche + Alt droit) ; Hôte + R est le reset documenté.
- **Disposition** : l'Amiga démarre en clavier US (Kickstart et la plupart des jeux) ; une saisie AZERTY dans le
  Workbench demande la keymap `f` côté Amiga. Le clavier virtuel affiche donc par défaut les légendes `us`
  (option du jeu `keyboard_layout = "fr"` pour un Workbench configuré en français).
- **Souris** : port 0 libretro = port souris Amiga (`puae_physicalmouse = enabled`) ; capture et repli 2.0
  inchangés. **Joystick** : RetroPad port 0 = port joystick Amiga (`puae_joyport = joystick`, ordre `1234`) — à
  confirmer par test (Amiga Test Kit, écran « Controller Ports »). Mode par défaut d'un jeu Amiga : **Clavier**
  (comme le ST, décision utilisateur) ; Hôte + J pour le joystick au clavier (profil `KeyMap` `puae`, ZQSD + J/K).
- Clavier virtuel : disposition `amiga.toml` (A500 : Esc, F1-F10, Del, Help, deux Amiga, pavé avec `( ) / *`),
  légendes `us`, `fr`, `de`.

### 8. Bouton « Ouvrir le dossier des ROMs » et sélecteur de fichier (demandes utilisateur)

- Dépendances `emu-ui` : **`opener` 0.9** (`default-features = false`) pour ouvrir un dossier, **`rfd` 0.18**
  (features par défaut `xdg-portal` + `wayland`) pour choisir un fichier. Licences MIT / MIT OR Apache-2.0 ;
  dépendances transitives (`objc2*`, `windows-sys`, `wayland-*`) déjà couvertes par `about.toml` ou à y ajouter
  (vérifié par `platform-build`). `rfd` sur macOS lie `UniformTypeIdentifiers` (macOS 11+) : compatible avec le Mac
  12.7.6 de l'utilisateur ; `scripts/check-macos-minos.sh` reste le garde-fou (minos ≤ 12.0).
- **Ouvrir le dossier** (écran Bibliothèque, barre d'outils, et menu « Dossiers ») : ouvre `<data>/roms` (toujours
  scanné) ; s'il y a d'autres `library.roots`, petit menu avec chaque dossier. Créé s'il manque. Windows : Explorateur ;
  macOS : Finder ; Linux : gestionnaire par défaut (`xdg-open`) ; binaire Linux sous WSL2 : Explorateur Windows
  (`\\wsl.localhost\…`, fait par `opener`). Échec (pas de gestionnaire, `xdg-open` absent) ⇒ même affichage qu'en
  Docker. Appel non bloquant (processus détaché par `opener`).
- **Docker** (détecté par `EMU_RUST_CONTAINER=1`, exporté par `entrypoint.sh`, jamais par heuristique `/.dockerenv`) :
  le bouton affiche le chemin dans le conteneur (`/data/roms`), un bouton « Copier » (presse-papiers egui) et l'aide :
  `docker cp <fichier> <conteneur>:/data/roms/` ou le dossier de l'hôte si `EMU_HOST_ROMS_DIR` est défini (montage
  lié optionnel ajouté, commenté, dans `docker/compose.yaml`).
- **Sélecteur de fichier** (import de BIOS, puis « Ajouter une disquette » et dossiers de la bibliothèque si utile) :
  `rfd::FileDialog` synchrone sur le thread UI (émulation déjà en pause dans ces écrans), fenêtre parente = fenêtre
  winit. Linux natif : utilisé seulement si `DBUS_SESSION_BUS_ADDRESS` est défini ou `zenity` est dans le `PATH`
  (sinon `rfd` échouerait sans le dire : il renvoie `None` comme une annulation). Docker, ou aucun dialogue natif :
  **navigateur de fichiers egui interne** (dossiers, filtre par extension, chemin saisi à la main), point de départ
  `/data` en Docker, dossier personnel ailleurs.

### 9. Écarts avec la 2.0

| Sujet | 2.0 (ST) | 3.0 (Amiga) |
|---|---|---|
| BIOS sans ROM propriétaire | EmuTOS téléchargé (`FreeBiosManager`) | AROS **intégré au core**, aucun téléchargement |
| BIOS d'origine | `tos.img` déposé à la main | Kickstart **par modèle**, import guidé (§3), choix frontend `puae_kickstart` |
| Nouvel état firmware | `Free` | `CoreFallback`, `Builtin` (posé en 3.0, données en 5.0) |
| Contrôle avant lancement | `tos.img` présent ou EmuTOS proposé | **contrôle unique** `required_firmware` par variante (modèle), repli optionnel ; 6.0 / 7.0 : variante = région, données seulement |
| Options | core < `cores.toml` < config < jeu | + couche **défauts calculés** entre `cores.toml` et la config |
| Sauvegardes écrites par le core | — | motifs `game_saves` + `reset_core_game_files` (générique) |
| Schéma de la bibliothèque | v3 | **v4** : `dat_info` / `dat_entry` à clé `(system, source)` (plusieurs DAT par système) |
| Extensions partagées | — | `.m3u` = système de ses entrées ; `.ipf` = dossier, puis DAT, sinon non classé ; `folder_extensions` |
| Politique des médias | une par core | une par core **+ exceptions par extension** (`media_direct`), ordre d'évaluation unique (design §3.5.1) |
| Zips | extraits seulement si le core ne déclare pas `zip` | inspectés quel que soit le core : extraits s'ils contiennent des disquettes, sinon passés au core |
| DAT | TOSEC | TOSEC (disquettes) **+ DAT WHDLoad** (`dat/`, `.lha`) |
| Disposition clavier | positionnelle, langue de TOS | positionnelle **+ surcharges** `[remap]` (Alt gauche / droit = Amiga gauche / droite, même table sur les 3 OS) ; Amiga en US |
| Lecteurs | DF0 (+ DF1 rarement) | DF0-DF3 remplis par MultiDrive ; seul DF0 est changeable |
| Nouvelles dépendances | `sys-locale` | `opener`, `rfd` (UI uniquement) |

## Conséquences

- Aucun crate nouveau, graphe inchangé ; dépendances `opener` et `rfd` dans `emu-ui` seulement (pas d'`unsafe` de
  notre côté ; `rfd` charge `libdbus` à l'exécution sous Linux).
- Sans Kickstart, une partie des jeux Amiga ne démarre pas : c'est attendu et dit à l'utilisateur ; les tests
  automatiques tournent uniquement sur AROS intégré + Amiga Test Kit (aucune ROM propriétaire dans les tests ni la CI).
- L'AROS intégré dépend de la version du core : une mise à jour de puae peut le changer (empreintes de test
  ré-enregistrées, comme pour toute nightly).
- `.ipf` non jouables sans `capsimg` fourni par l'utilisateur ; `.7z` et CD hors bibliothèque.
- Les copies de travail peuvent occuper de la place (HDF jusqu'à 2 Gio) : la bibliothèque standard ne donne pas l'espace libre (pas de crate ajouté pour cela) ; une copie interrompue (disque plein) est supprimée et signalée.
- L'import de BIOS sert tous les systèmes (TOS, Kickstart, BIOS Game Boy…) et remplace le dépôt manuel dans
  `system/` sans le supprimer.
- La 3.0 porte les mécanismes génériques dont dépendent les 4.0 à 7.0 (ordre d'évaluation des médias,
  `folder_extensions`, `game_saves`, défauts calculés, contrôle de firmware par variante, `FirmwareState::Builtin`,
  schéma v4) : les versions suivantes n'ajoutent que des données ou des sources de variante.
- Migration de schéma v3 → v4 (sans perte : favoris, historique, jaquettes conservés ; les DAT sont ré-importés).
- Sous Windows, Alt + F4 / Alt + Tab / Alt + Espace ne sont pas utilisables comme combinaisons Amiga.
