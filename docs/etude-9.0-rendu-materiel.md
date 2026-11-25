# Étude préliminaire 9.0 — rendu matériel

Date : 2026-11-22 (architecte). Statut : **étude**, ni ADR ni découpage définitif avant les réponses de l'utilisateur
(§9, reprises dans `docs/questions-ouvertes.md`). Aucun code touché, rien n'a été compilé ni exécuté.

Décisions déjà prises (utilisateur, 2026-11-21) : 9.0 = rendu matériel, **OpenGL d'abord, puis DirectX 11**, Vulkan au
second plan (tests lavapipe, essai réel par un camarade) ; la 8.0 refuse les cores à rendu matériel (« prévu en 9.0 ») ;
compteur FPS / temps par image pour comparer OpenGL et DirectX sur son Intel Iris Xe.

**Périmètre (consigne de l'utilisateur, 2026-11-22)** : le rendu matériel vise **Windows natif** (cible principale,
Iris Xe : OpenGL puis Direct3D 11), puis **macOS Intel** (OpenGL ≤ 4.1), puis **Linux natif**. **WSL / WSLg, Docker et
noVNC sont hors périmètre : rendu logiciel seulement, cores à rendu matériel refusés comme en 8.0.** Llvmpipe et
lavapipe ne servent qu'aux tests automatiques (§5).

Fiabilité des sources, notée dans tout le document :

- **[V]** vérifié le 2026-11-22 dans `libretro.h`, `libretro_d3d11.h`, `libretro_vulkan.h` (libretro-common, `master`)
  ou dans les `.info` de libretro-super (`master`) ;
- **[C]** constaté dans notre code ou nos ADR ;
- **[M]** de mémoire, **à vérifier** par un essai avant de s'engager dessus.

## 1. Résumé

- Notre pile (ADR 0002, 0008, 0010) a été choisie pour ce jour : pas d'`eframe`, contexte GL possédé par `emu-runtime`,
  tout sur le thread principal. **Le graphe des crates ne change pas.**
- Recommandation centrale : le core dessine dans **son propre contexte OpenGL, partagé avec celui de l'interface**
  (même fenêtre, groupe de partage), dans un FBO dont la texture de couleur est ensuite affichée par notre
  `FrameRenderer` existant. L'état GL du core et celui d'egui ne se mélangent jamais.
- Premier système recommandé : **PlayStation en haute résolution avec swanstation**, déjà livré en logiciel (7.0) :
  aucun nouveau système, BIOS, format de disque ni mapping à créer, et le même core sait faire OpenGL, Direct3D 11 et
  Vulkan, donc c'est le meilleur banc de comparaison.
- **DirectX 11 est un second chantier presque aussi gros que le premier**, pas une option à cocher : egui n'a pas de
  moteur D3D11 officiel et `wgpu` ne donne pas de périphérique D3D11. Chemin réaliste : l'interface reste en OpenGL,
  le core dessine en D3D11, l'image est passée à OpenGL (copie, puis partage direct si le pilote le permet). Ce code
  ne peut être ni exécuté ni testé dans le conteneur de Claude.
- Le compteur existe déjà en partie (panneau « Stats » : FPS, image, audio). Il faut y ajouter les temps par image
  (core, GPU, présentation) et un **mode mesure** qui écrit un rapport.

## 2. L'API libretro du rendu matériel

### 2.1 Commandes d'environnement

| Commande | N° | Rôle | Sens |
|---|---|---|---|
| `SET_HW_RENDER` | 14 [V] | Le core demande une API (type, version) et donne ses fonctions `context_reset` / `context_destroy` ; le frontend remplit `get_current_framebuffer` et `get_proc_address` dans la structure du core. « Should be called in `retro_load_game()` » [V] | entrée / sortie |
| `GET_PREFERRED_HW_RENDER` | 56 [V] | Le frontend écrit son API préférée (`retro_hw_context_type`). Renvoie `true` s'il sait aussi en fournir d'autres, `false` s'il ne fournit que celle-là [V] | sortie |
| `SET_HW_SHARED_CONTEXT` | 44 \| EXPERIMENTAL [V] | Le core demande un contexte GL séparé de celui du frontend (état GL non pollué). Donnée ignorée | — |
| `GET_HW_RENDER_INTERFACE` | 41 \| EXPERIMENTAL [V] | Après `context_reset` : structure propre à l'API (Vulkan, D3D9 à 12). Sans objet pour OpenGL | sortie |
| `SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE` | 43 \| EXPERIMENTAL [V] | Entre `SET_HW_RENDER` et `context_reset` : le core participe à la création du périphérique. Existe pour Vulkan, D3D12 et D3D11 [V] | entrée |
| `GET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_SUPPORT` | 73 \| EXPERIMENTAL [V] | Version de négociation gérée par le frontend | sortie |
| `SET_SYSTEM_AV_INFO` (32), `SET_GEOMETRY` (37) [V] | | Changement de taille en cours de jeu : la taille maximale fixe celle du FBO | entrée |

Aujourd'hui [C] : `SET_HW_RENDER` renvoie `false` et est journalisée (`crates/emu-libretro/src/callbacks.rs:907`),
`GET_PREFERRED_HW_RENDER` est déclinée (`:956`). swanstation en `Auto` fait 15 demandes puis se rabat sur le logiciel
(ADR 0021) : il essaie une liste de types et de versions, que la 9.0 devra journaliser pour savoir quoi servir.

### 2.2 `retro_hw_render_callback` [V]

| Champ | Rempli par | Remarque |
|---|---|---|
| `context_type` | core | voir §2.3 |
| `version_major`, `version_minor` | core | pour `OPENGL_CORE` et GLES 3.1+ |
| `context_reset` | core | appelé par nous, contexte courant, après création ou perte du contexte. Les ressources GL du core sont alors invalides |
| `context_destroy` | core | appelé par nous avant destruction contrôlée ; peut être `NULL` |
| `get_current_framebuffer` | **nous** | renvoie l'identifiant du FBO où dessiner. Le core doit le redemander à chaque image ; beaucoup le gardent en cache [M] : garder un identifiant stable |
| `get_proc_address` | **nous** | renvoie l'adresse d'une fonction GL par son nom |
| `depth`, `stencil` | core | attacher un tampon 24/8 au FBO |
| `bottom_left_origin` | core | origine en bas à gauche : retourner l'image à l'affichage |
| `cache_context`, `debug_context` | core | éviter de recréer le contexte ; contexte de débogage |

Déroulement : `retro_load_game` (le core appelle `SET_HW_RENDER`) → nous créons le contexte et le FBO → `context_reset`
→ `retro_run` : le core dessine puis appelle `video_refresh` avec `RETRO_HW_FRAME_BUFFER_VALID` (`(void*)-1`) [V], ou
`NULL` pour répéter l'image → à l'arrêt : `context_destroy`, puis `retro_unload_game` [M pour l'ordre exact, celui de
RetroArch]. `width` / `height` de `video_refresh` désignent la partie utile du FBO.

Cas à traiter : un core qui rappelle `SET_HW_RENDER` en cours de jeu (changement de moteur dans ses options) [M].
Défaut proposé pour la 9.0 : refuser et afficher « redémarrez le jeu ».

### 2.3 Types de contexte [V]

| Valeur | Type | 9.0 | Remarque |
|---|---|---|---|
| 1 | `OPENGL` (profil de compatibilité, 2.x et plus) | oui, sauf macOS au-delà de 2.1 | macOS n'a pas de profil de compatibilité au-dessus de 2.1 |
| 3 | `OPENGL_CORE` (version demandée) | **oui** | macOS : jusqu'à 4.1 |
| 2, 4, 5 | `OPENGLES2`, `OPENGLES3`, `OPENGLES_VERSION` | non | aucun core visé n'en a besoin sur un ordinateur |
| 6 | `VULKAN` | palier Vulkan | interface et négociation dédiées (`libretro_vulkan.h`) |
| 7 | `D3D11` | palier DirectX | `retro_hw_render_interface_d3d11` |
| 8, 9, 10 | `D3D10`, `D3D12`, `D3D9` | non | |

### 2.4 Direct3D 11 [V]

`GET_HW_RENDER_INTERFACE` rend `retro_hw_render_interface_d3d11` : `device`, `context` (contexte immédiat),
`featureLevel`, `D3DCompile`. **C'est le frontend qui crée le périphérique.**

- Version 1 (celle de tous les cores existants) : l'image est la **vue de ressource de shader que le core laisse liée
  à l'emplacement 0 du pixel shader** au moment de `video_refresh`. Ne fonctionne que si le frontend dessine sur le
  thread du core : c'est notre cas (ADR 0010).
- Version 2 (récente, obtenue par négociation) : verrou de contexte, `set_texture`, index de synchronisation. À ignorer
  au début : la version 1 suffit à un frontend à un seul thread.

### 2.5 Vulkan [V pour la structure, M pour le détail]

`retro_hw_render_interface_vulkan` (instance, périphérique, file, `set_image`, verrou de file, index de
synchronisation) et négociation v1 / v2 où le core peut créer lui-même l'instance et le périphérique. Nettement plus
lourd que GL ; rien de commun avec le chemin GL côté frontend.

## 3. Intégration dans notre pile

### 3.1 Ce qui existe [C]

- `emu-ui` possède la boucle `winit` ; `emu-runtime::GlWindow` possède le contexte (`glutin`, OpenGL 3.3 core, jusqu'à
  4.1) et tous les appels `glow` ; `egui_glow` reçoit `GlWindow::gl()`.
- Une image : passe egui → `upload_frame(FrameRgba)` → `draw_frame` (remet tout l'état GL qu'il utilise) → peinture
  egui → `capture` éventuelle (`glReadPixels` du tampon arrière) → `swap_buffers`.
- Tout est sur le thread principal ; la cadence est pilotée par l'audio, la vsync est une option (ADR 0010, qui
  annonçait déjà : « compatible avec le rendu matériel sans contexte partagé »).
- Un seul `Core` par processus, `!Send`, callbacks C globaux enveloppés dans `catch_unwind` (ADR 0008).

### 3.2 Options pour le contexte OpenGL

| | A. Contexte unique (celui de l'interface) | **B. Contexte du core dans le groupe de partage (recommandé)** | C. Contexte du core isolé + relecture |
|---|---|---|---|
| Principe | Le core dessine dans un FBO de notre contexte 3.3 core | Second contexte `glutin` créé à la demande du core (type, version, profil), partagé avec le nôtre ; FBO créé dans le contexte du core, sa texture de couleur est lue par le nôtre | Contexte sans partage ; `glReadPixels` à chaque image puis chemin logiciel actuel |
| Type et version servis | seulement ce que vaut notre contexte (core 3.3 à 4.x) ; pas de profil de compatibilité | **ce que le core demande** (compatibilité ou core, 3.0 à 4.6 selon le pilote) | idem B |
| État GL | le core laisse n'importe quoi (PBO lié, `UNPACK_*`, samplers, masques…) : egui et `draw_frame` doivent tout remettre, source de défauts discrets | **isolé** : rien à remettre | isolé |
| Coût par image | nul | deux changements de contexte + un `glFlush` ; négligeable [M] | relecture : plusieurs ms en haute résolution, annule une partie du gain |
| `SET_HW_SHARED_CONTEXT` | à refuser | satisfait d'office | satisfait |
| Changement de core, « Redémarrer » (ADR 0022) | ressources du core à purger dans notre contexte | on détruit le contexte du core, l'interface n'est pas touchée | idem B |
| macOS | core ≤ 4.1 | core ≤ 4.1 (partage entre profils différents impossible [M]) | idem |

Points techniques de B, à valider par un prototype en tout début de 9.0 :

- les FBO et VAO **ne sont pas partagés** entre contextes, les textures oui : FBO créé dans le contexte du core,
  texture relue chez nous après `glFlush` côté core ;
- le contexte du core est rendu courant sur **la même surface de fenêtre** (même configuration de pixels) : pas besoin
  de surface cachée ; il ne dessine que dans son FBO ;
- une garde RAII rend le contexte de l'interface courant à la sortie de `retro_run`, même en cas de panique : tout
  appel `glow` existant suppose ce contexte ;
- `get_proc_address` passe par `glutin` (`Display::get_proc_address`), comme notre chargeur `glow` actuel ;
- taille du FBO = `max_width` × `max_height` de l'`av_info`, recréé sur `SET_SYSTEM_AV_INFO`.

C reste utile comme **repli** (pilote qui refuse le partage) et sert de toute façon aux tests headless et à Vulkan.

### 3.3 Répartition par crate (graphe inchangé)

| Crate | Travail | `unsafe` |
|---|---|---|
| `emu-libretro` | Lire et mémoriser la demande (`HwRenderRequest` : type, version, profondeur, stencil, origine) ; trampolines C `get_current_framebuffer` / `get_proc_address` vers un trait sûr installé par le runtime ; appels gardés de `context_reset` / `context_destroy` ; répondre à `GET_PREFERRED_HW_RENDER` et `SET_HW_SHARED_CONTEXT` ; `video_refresh` avec `RETRO_HW_FRAME_BUFFER_VALID` exposé comme une variante sûre de `VideoFrame` | oui |
| `emu-runtime` | Contexte du core, FBO, affichage de la texture (retournement, sous-rectangle), relecture à la demande (`FrameRgba`), contexte headless pour les tests, mesures de temps | oui |
| `emu-ui` | Réglage « rendu » par core, résolution interne (options de core déjà gérées), panneau Stats étendu, messages (« non disponible sur ce système ») | non |
| `emu-library`, `emu-assets` | `hw_render` / `required_hw_api` structurés (prévu par l'étude 8.0, §5.1), `cores.toml` : API par core et par OS, nouveaux systèmes des paliers suivants | non |

L'étude 8.0 (§5.1, points 1 à 4) demande déjà à la 8.0 de mémoriser la demande `SET_HW_RENDER` et de faire du refus
un point unique : si la 9.0 passe avant la 8.0, ce travail est fait en 9.0 et la 8.0 en hérite.

### 3.4 Captures, sauvegardes d'état, tests

- Capture d'écran de la fenêtre : inchangée (`GlWindow::capture` lit le tampon arrière, image du core comprise).
- Image du jeu seule (vignette, test) : `glReadPixels` du FBO **à la demande**, jamais à chaque image.
- `Emulator::frame()` n'a plus d'image en mémoire en mode matériel : l'API devra dire « image en texture » ; à poser
  dans l'ADR.

### 3.5 Direct3D 11 sous Windows : ce que ça implique vraiment

| Option | Principe | Pour | Contre |
|---|---|---|---|
| **D1. Pont D3D11 → OpenGL (recommandé pour commencer)** | Interface en OpenGL comme aujourd'hui. Nous créons un périphérique D3D11 (crate `windows`), le core y dessine, nous copions sa texture vers une texture GL : d'abord par relecture (`Map` + `glTexSubImage`), ensuite par `WGL_NV_DX_interop2` si le pilote Intel l'expose [M] | Aucun changement de l'interface ; isolé dans `emu-runtime` sous `#[cfg(windows)]` ; réutilise l'affichage du §3.2 | La copie coûte quelques ms en haute résolution et **fausse la comparaison** si on ne mesure pas à part le temps du core (§7) ; l'interop dépend du pilote |
| D2. Tout en D3D11 | Chaîne d'échange DXGI, egui dessiné en D3D11 par un crate tiers (`egui-directx11` [M] : communautaire, compatibilité avec egui 0.37 à vérifier) ou par un moteur maison | Zéro copie, mesure honnête | Deuxième moteur d'interface à maintenir, `FrameRenderer` à réécrire en HLSL, captures et tests d'interface à doubler |
| D3. Interface sur ANGLE | `egui_glow` en OpenGL ES traduit en D3D11 par ANGLE ; partage de texture par les extensions ANGLE [M] | Zéro copie, un seul code d'interface | Livrer `libEGL.dll` / `libGLESv2.dll` (provenance, taille), nos shaders en ES, `GlWindow` refuse aujourd'hui les contextes ES [C] |
| D4. `wgpu` | Passer l'interface à `egui-wgpu` | — | **Ne répond pas au besoin** : `wgpu` n'a pas de moteur D3D11 et les cores libretro exigent un `ID3D11Device` fourni par le frontend [M pour l'absence de D3D11 dans wgpu] |

Franchise :

- le code D3D11 est du COM en `unsafe`, **non exécutable dans le conteneur de Claude** : contrôles possibles =
  `clippy --target x86_64-pc-windows-gnu`, CI Windows avec le périphérique logiciel WARP [M], essais de l'utilisateur ;
- cores qui l'annoncent [V, `.info`] : swanstation, flycast, ppsspp, dolphin, pcsx2. mupen64plus_next, mednafen_psx_hw,
  kronos, citra : **OpenGL seulement** (ou Vulkan) ;
- le gain de D3D11 sur un Iris Xe n'est pas acquis : c'est précisément ce que le compteur doit trancher. D'où l'ordre
  retenu (OpenGL d'abord, mesure, puis décision).

## 4. Cores concernés

Colonne « API » : champ `required_hw_api` des `.info` [V]. Le reste est **[M]** sauf mention ; les estimations de
performance sur Iris Xe sont des ordres de grandeur à mesurer, pas des promesses.

| Système | Core | API annoncée [V] | macOS Intel (GL 4.1 core) | Gain concret | Iris Xe (estimation) | BIOS / fichiers |
|---|---|---|---|---|---|---|
| PlayStation | **swanstation** (déjà livré en logiciel) | OpenGL ≥ 3.0 \| Core ≥ 3.1 \| Vulkan ≥ 1.0 \| Direct3D ≥ 11.0 | oui, à vérifier | résolution interne ×2 à ×8, PGXP (géométrie stable), filtrage | large marge à ×4 | comme en 7.0 |
| PlayStation | mednafen_psx_hw | Core ≥ 3.3 \| Vulkan ≥ 1.0 | oui, à vérifier | idem, plus précis, plus lourd | correct à ×2-×4 | BIOS **requis** (pas de remplacement) |
| Nintendo 64 | **mupen64plus_next** | Core ≥ 3.3 \| GLES ≥ 2.0 | oui, à vérifier | système **nouveau** ; pas de chemin logiciel raisonnable | bon à ×2 | aucun |
| Nintendo 64 | parallel_n64 | OpenGL ≥ 3.0 \| GLES ≥ 2.0 \| Vulkan ≥ 1.0 | **non** (profil de compatibilité) | rendu fidèle par Vulkan (parallel-RDP) | Vulkan : hors de nos essais | aucun |
| Dreamcast | **flycast** | OpenGL ≥ 3.0 \| Core ≥ 3.1 \| GLES ≥ 2.0 \| Vulkan ≥ 1.1 \| Direct3D ≥ 11.0 | oui, sans transparence triée par pixel (demande 4.3) | système **nouveau** | bon à ×2 | BIOS facultatif (remplacement intégré) [M] |
| Saturn | kronos | Core ≥ 4.3 | **non** | système nouveau | lourd (shaders de calcul) | BIOS recommandé |
| Saturn | yabasanshiro | OpenGL ≥ 3.3 \| GLES ≥ 3.0 | douteux | système nouveau | moyen | — |
| Saturn | mednafen_saturn | **aucune** (`hw_render = false`) | oui | **n'exige pas la 9.0** ; très gourmand en processeur | dépend du processeur | BIOS requis |
| PSP | ppsspp | OpenGL ≥ 3.0 \| Core ≥ 3.1 \| GLES ≥ 2.0 \| Vulkan ≥ 1.0 \| Direct3D11 ≥ 11.0 | oui, à vérifier | système nouveau, ×2-×3 | bon | fichiers d'accompagnement libres à télécharger (à vérifier, ADR 0005) |
| Nintendo DS | melondsds, desmume | Core ≥ 3.2 ; OpenGL ≥ 2.0 \| Core ≥ 3.1 | — | **rendu logiciel par défaut** : n'exige pas la 9.0 ; le matériel ajoute la haute résolution | bon | facultatifs |
| 3DS | citra | Core ≥ 3.3 | douteux | système nouveau ; projet amont arrêté | moyen | — |
| GameCube / Wii | dolphin | Vulkan ≥ 1.0 \| Direct3D ≥ 10.0 \| Core ≥ 3.3 \| GLES ≥ 3.0 | limité | système nouveau ; core libretro en retard sur l'amont | GameCube jouable, Wii variable | dossier `Sys` à fournir |
| PlayStation 2 | pcsx2 | Direct3D ≥ 11.0 \| Core ≥ 3.3 \| OpenGL ≥ 3.0 | douteux | système nouveau | lourd | BIOS requis, de l'utilisateur |
| Vectrex | vecx | Core ≥ 3.3 | oui | petit système ; chemin logiciel à vérifier | sans objet | aucun |
| PC DOS | dosbox_pure | — (`hw_render = false`) | — | accélération 3dfx Voodoo par OpenGL, en option [M, à vérifier] | — | — |

Lecture :

- **trois cores couvrent l'essentiel avec un contexte OpenGL core ≤ 4.1, donc toutes nos cibles** : swanstation,
  mupen64plus_next, flycast ;
- les `.info` sur-déclarent (swanstation tourne en logiciel, ADR 0021 ; étude 8.0 §2.3) : seule la demande réelle
  `SET_HW_RENDER` fait foi ;
- Saturn (mednafen_saturn) et DS ne dépendent pas du rendu matériel : ils peuvent être décidés à part (Q9) ;
- macOS : OpenGL est obsolète chez Apple mais présent sur Intel ; l'essai reste à la charge de l'utilisateur (Mac
  Intel, Monterey).

## 5. Tests

| Sujet | Proposition | État du conteneur [C] |
|---|---|---|
| Core factice à rendu matériel | Étendre le core C d'ADR 0009 : il demande un contexte, efface et dessine des rectangles alignés par `get_proc_address`. Résultat **exact** quel que soit le pilote : teste le cycle de vie (`context_reset` / `destroy`, FBO, origine, changement de taille, rechargement ADR 0022) sans dépendre d'un vrai core | compilateur C présent |
| Contexte GL sans écran | Mesa llvmpipe : contexte EGL sans surface (`glutin`, plate-forme « device » ou « surfaceless » [M]) ; repli : fenêtre cachée sous `Xvfb`, déjà utilisé pour les captures | `libEGL_mesa`, `libGLX_mesa` 22.3.6, `Xvfb` présents ; `glxinfo` / `eglinfo` absents |
| OSMesa | **à écarter** : retiré des Mesa récents [M] | absent |
| Empreintes d'images de vrais cores | Le rendu 3D n'est **pas identique au bit près** d'un pilote ou d'une version de Mesa à l'autre : comparer avec tolérance (image réduite, seuil par pixel), version de Mesa notée dans le test ; les empreintes exactes restent réservées au core factice et au rendu logiciel | — |
| Vrais cores | swanstation en matériel sur llvmpipe à ×1 et ×2, quelques centaines d'images, ROMs homebrew déjà dans `tests/` ; lent mais faisable [M] | — |
| Vulkan | lavapipe (`mesa-vulkan-drivers`) + relecture ; à ajouter à l'image de Claude et à la CI | **absent** (ni `libvulkan` ni ICD) |
| CI Linux | `xvfb` et `libgl1-mesa-dri` déjà installés (`ci.yml`) : llvmpipe disponible | — |
| CI Windows | pas de pilote GL moderne sur les machines de CI [M] : tests GL matériels **ignorés** sous Windows, ou DLL Mesa ajoutées ; D3D11 testable par WARP [M] | — |
| CI macOS | contexte 4.1 par le moteur logiciel d'Apple, à vérifier [M] ; sinon tests ignorés et essai par l'utilisateur | — |

Conséquence : sous Windows, la cible principale, la vérification automatique du chemin matériel sera **faible** ;
l'essai de l'utilisateur et le mode mesure (§7) comptent davantage que pour les versions précédentes.

## 6. Risques

| Risque | Gravité | Parade |
|---|---|---|
| WSL / WSLg, Docker, noVNC | — | **hors périmètre** : rendu logiciel seulement, cores à rendu matériel refusés comme en 8.0 (détection du moteur à préciser dans l'ADR : mode Docker, `GL_RENDERER` logiciel) |
| macOS : 4.1 core au plus, pas de profil de compatibilité, OpenGL obsolète | moyenne | ne proposer par OS que les cores servis (`cores.toml`) ; message « non disponible sur macOS » ; pas de build ni d'essai macOS possibles ici |
| Pilote OpenGL Intel sous Windows (réputation inégale avec les émulateurs) | moyenne | mesurer d'abord (mode mesure) ; DirectX 11 avancé si mauvais (décision déjà prise) |
| `unsafe` FFI nouveau : écriture dans la structure du core, appel de ses fonctions, trampolines sans donnée utilisateur, contexte courant implicite | **haute** | invariants écrits dans l'ADR ; garde RAII de contexte ; `catch_unwind` ; `context_destroy` toujours avant le déchargement ; revue `reviewer` obligatoire |
| Cores qui appellent les callbacks depuis un autre thread (rendu en thread) : aujourd'hui ignorés (ADR 0008) | moyenne | relever par core lors des essais ; forcer l'option « sans thread de rendu » dans `cores.toml` |
| Core lourd qui ne tient pas la cadence sur le thread de l'interface (ADR 0010) | moyenne | mesurer ; l'ADR 0010 prévoit déjà qu'une ADR pourra introduire un thread d'émulation, **pas en 9.0** |
| Rechargement d'un core resté en mémoire (ADR 0022) avec des ressources GL vivantes | moyenne | détruire le contexte du core à chaque arrêt (option B) ; test dédié avec le core factice |
| D3D11 écrit sans pouvoir l'exécuter ici | **haute** | palier séparé, petit, derrière un réglage ; WARP en CI ; boucle d'essais avec l'utilisateur |
| Empreintes d'images instables d'une version de Mesa à l'autre | faible | tolérance + core factice exact |
| Taille des téléchargements et fichiers d'accompagnement (ppsspp, dolphin) | faible | à traiter au palier concerné, ADR 0005 |

## 7. Compteur de performance

Existant [C] : panneau « Stats » (`emu-ui/src/ui/osd.rs`, `StatsView`) : FPS, numéro d'image, tampon audio, sous-alimentations.

À ajouter, utile dès le rendu logiciel et livrable **avant** le reste de la 9.0 :

| Mesure | Comment |
|---|---|
| Temps de `retro_run` (ms, moyenne et pire cas) | horloge autour de l'appel |
| Temps GPU du core | requête `GL_TIME_ELAPSED` (dans OpenGL 3.3 core) ; équivalent D3D11 au palier DirectX |
| Temps de présentation (copie éventuelle, composition, `swap_buffers`) | horloge |
| Régularité | 1 % des images les plus lentes, nombre d'images au-delà de 16,7 ms |
| Contexte | API, `GL_RENDERER`, version du pilote, core, résolution interne, vsync |

**Mode mesure** : action « Mesurer 60 s » (et option de ligne de commande) qui écrit un petit rapport texte dans le
dossier de données, à copier-coller. C'est ce rapport, sur le même jeu et la même scène, qui compare OpenGL et
DirectX 11 ; il sépare le temps du core du temps de présentation, sinon le pont D1 pénaliserait DirectX à tort.

## 8. Paliers proposés

| Palier | Contenu | Cibles | Dépend de |
|---|---|---|---|
| **9.0** | Socle OpenGL (option B, repli C) ; **PlayStation haute résolution avec swanstation** (réglage, pcsx_rearmed reste le défaut) ; compteur étendu et mode mesure ; core factice à rendu matériel ; contexte headless ; `GET_PREFERRED_HW_RENDER` | Windows, macOS Intel, Linux natif | rien de la 8.0 |
| 9.1 | Premier système nouveau en OpenGL : **Dreamcast (flycast)** par défaut, ou Nintendo 64 (Q2) ; l'autre en 9.1.x. Systèmes personnalisés à rendu matériel OpenGL si la 8.0 est livrée | les trois | 9.0 ; 8.0 pour les systèmes personnalisés |
| 9.2 | **DirectX 11** sous Windows, pont D1 : swanstation puis flycast ; comparaison par le mode mesure | Windows | 9.0. **Peut passer avant la 9.1** si la mesure OpenGL sur Iris Xe est mauvaise |
| 9.3 | PSP (ppsspp), Saturn (choix du core après essais) | les trois, Saturn selon le core | 9.1 |
| 9.4 | Vulkan : tests lavapipe, essai réel par un camarade (le rappeler à l'utilisateur) | Windows, Linux | 9.0 |
| hors 9.x | GameCube / Wii, 3DS, PlayStation 2 | — | décision ultérieure |

Pourquoi PlayStation d'abord : rien d'autre que le rendu ne change (système, BIOS, disques, manette, tests et jeu
d'essai de l'utilisateur existent), donc tout défaut vient du socle ; et swanstation servira de banc identique pour
OpenGL, DirectX 11 et Vulkan.

Ébauche de découpage de la 9.0 (indicative, à figer par l'ADR) :

| Étape | Agent | Tâche | Parallèle |
|---|---|---|---|
| HX0 | `architecte` | ADR « rendu matériel OpenGL » (amende 0002, 0008, 0010), contrat public (`HwRenderRequest`, trait hôte, image en texture) | — |
| HP0 | `runtime-av` | prototype jetable : deux contextes partagés + FBO sur llvmpipe, puis essai Windows par l'utilisateur | avant HX0 si possible |
| HF1 | `libretro-ffi` | commandes d'environnement, trampolines, cycle de vie, core factice à rendu matériel | avec HR1, HU1 |
| HR1 | `runtime-av` | contexte du core, FBO, affichage, relecture, headless, mesures | avec HF1, HU1 |
| HU1 | `ui` | panneau Stats étendu, mode mesure, réglages | avec HF1, HR1 |
| HA1 | `library-assets` | `cores.toml` : API par core et par OS, options swanstation | avec les précédents |
| HB1 | `platform-build` | paquets Mesa de la CI et de l'image de développement (tests seulement), lavapipe pour plus tard | avec les précédents |
| HU2 | `ui` | intégration, messages | après HF1 + HR1 |
| HQ1 / HV1 | `qa-systems` / `reviewer` | vérification complète, vrais cores ; revue de tout l'`unsafe` | à la fin |

## 9. Questions pour l'utilisateur

Défaut recommandé en gras ; « ok pour les défauts » suffit.

1. **Premier système de la 9.0.** a) **PlayStation en haute résolution (swanstation), sans nouveau système** ;
   b) Nintendo 64 ; c) Dreamcast.
2. **Premier système nouveau (9.1).** a) **Dreamcast (flycast : OpenGL et DirectX 11 dans le même core)** ;
   b) Nintendo 64 (OpenGL seulement) ; c) les deux dans la même version.
3. **PlayStation par défaut.** a) **pcsx_rearmed en rendu logiciel reste le défaut, la haute résolution est un réglage
   à activer** ; b) swanstation en haute résolution devient le défaut dès la 9.0.
4. **Résolution interne proposée à l'activation.** a) **×2**, réglable de ×1 à ×8 ; b) ×4 ; c) automatique selon la
   taille de la fenêtre.
5. **Place de DirectX 11.** a) **Après la mesure OpenGL sur ton Iris Xe : en 9.2 si OpenGL est bon, avancé avant la
   9.1 s'il est mauvais** ; b) toujours avant tout nouveau système ; c) seulement si OpenGL pose problème.
6. **Méthode DirectX 11.** a) **Commencer par le pont (interface en OpenGL, image du core copiée), en mesurant le coût
   de la copie ; revoir ensuite** ; b) viser d'emblée une interface entièrement en DirectX (plus long, plus risqué).
7. **Mode mesure.** a) **D'accord pour lancer « Mesurer 60 s » sur un jeu donné et coller le rapport** ; b) un simple
   compteur à l'écran suffit.
8. **macOS Intel.** a) **Accepter que certains cores ou réglages n'y existent pas (affichés grisés, « non disponible
   sur macOS »)** ; b) ne livrer un système que s'il marche sur toutes les cibles.
9. **Saturn et Nintendo DS**, qui ont des cores sans rendu matériel. a) **Les étudier à part, sans attendre la 9.x,
   quand tu le souhaites** ; b) les garder pour la 9.3 ; c) pas intéressé.
10. **GameCube / Wii, 3DS, PlayStation 2.** a) **Hors de la 9.x, à redécider plus tard** ; b) à prévoir dès maintenant
    (lequel ?).
11. **Vulkan.** a) **En 9.4, validé par les tests automatiques, essai par ton camarade quand il sera disponible** ;
    b) plus tôt ; c) abandonné.
12. **Ordre des versions.** a) **7.1 → 7.2 → 8.0 → 9.0, comme prévu** ; b) 9.0 avant la 8.0 (la 9.0 PlayStation ne
    dépend pas de la 8.0).
13. **À relever sur ton portable Windows** (pas de défaut) : modèle exact du processeur, mémoire, version du pilote
    graphique Intel (`dxdiag`, onglet Affichage). Sert à estimer et à interpréter les mesures.
