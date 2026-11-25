# ADR 0026 — Rendu matériel (9.x) : contexte OpenGL du core, interface toujours en OpenGL

Statut : **proposé** (2026-11-25, architecte). À faire accepter par l'utilisateur avec les questions de
`docs/design/v9.0-rendu-materiel.md` (§11). Étude : `docs/etude-9.0-rendu-materiel.md` (les marques [V], [C], [M] y
sont définies et reprises ici : vérifié dans `libretro.h`, constaté dans notre code, de mémoire **à vérifier**).
Réponses de l'utilisateur du 2026-11-22 et du 2026-11-23 (`docs/questions-ouvertes.md`, section 9.0) : elles
**font foi**. Conception et découpage du palier 9.0 : `docs/design/v9.0-rendu-materiel.md`.

Amende : ADR 0002 (Vulkan « en phase 7 » → dernier palier 9.x ; contexte GL du core), 0008 (deux contextes GL dans
`emu-runtime` ; un hôte de rendu installé pour **toute** la session matérielle, pas seulement pendant `run`), 0010
(« compatible sans contexte partagé » → contexte partagé), 0021 §1 (swanstation : rendu matériel possible, sur
réglage). Précise 0001 (un core par processus), 0003 (cadence inchangée), 0007 (tests sur llvmpipe), 0022 (contexte du
core détruit à chaque déchargement), et s'aligne sur 0025 (8.0, proposé : point unique de refus, demande
`SET_HW_RENDER` mémorisée), qu'elle transforme (§12).

**Graphe des crates inchangé. Aucune dépendance de crate nouvelle en 9.0** (`glutin` 0.32 sait déjà partager des
contextes, `glow` a les requêtes de temps et les objets de synchronisation). `unsafe` nouveau : `emu-libretro`
(structure `retro_hw_render_callback`, trampolines, appels `context_reset` / `context_destroy`) et `emu-runtime`
(contexte du core, FBO, synchronisation, relecture, mesures GPU). Aucun hôte réseau nouveau.

## Contexte

Relevé dans le code de `wip-7.2` (2026-11-25) [C] :

- `emu-libretro` refuse `SET_HW_RENDER` (14) et décline `GET_PREFERRED_HW_RENDER` (56)
  (`crates/emu-libretro/src/callbacks.rs`, vers les lignes 907 et 956) ; la sentinelle `RETRO_HW_FRAME_BUFFER_VALID`
  est ignorée par `frame_from_raw`. Le gestionnaire `FrontendHandler` n'est installé que pendant `Core::run`.
- `emu-runtime::GlWindow` possède **un** contexte `glutin` (OpenGL 3.3 core demandé, ≤ 4.1 accepté), la surface,
  la fenêtre `winit` et le `FrameRenderer` ; `make_current` prévoit déjà « a core's HW context later ».
  `Emulator` est **sans GL** : il produit un `FrameRgba` que l'interface téléverse (`upload_frame`).
- Lancement direct : `Emulator` est chargé **avant** la fenêtre (`App::resumed` crée la fenêtre d'après l'`av_info`).
  Mode bibliothèque : la fenêtre existe avant le jeu, qui l'emprunte (`App::attach` / `detach`).
- Ordre de destruction de `App` : peintre egui, **fenêtre**, puis émulateur (commentaire de champ, ADR 0008).
- Un seul thread pour l'émulation, la fenêtre et egui (ADR 0010) ; la plein-écran est `Fullscreen::Borderless`.
- Aucune détection de WSL ou de Docker dans le code ; le conteneur de Claude tourne lui-même sur un noyau WSL2
  (`/proc/version` contient « microsoft ») : les tests ne peuvent pas dépendre de cette détection.
- CI : Linux (Xvfb, Mesa llvmpipe), macOS 15 Intel, Windows 2025 (OpenGL 1.1 GDI seulement [M]).

Réponses de l'utilisateur qui cadrent l'ADR : PlayStation haute résolution (swanstation) en premier ; 9.1 Nintendo 64,
9.2 Dreamcast ; DirectX 11 **après** la mesure OpenGL sur son Intel Iris Xe (avancé avant la 9.1 si elle est
mauvaise), par un **pont D3D11 → OpenGL** ; mesure « 60 s » avec rapport à coller ; macOS : grisé quand absent ;
Vulkan en dernier palier, essayé d'abord par l'utilisateur ; DS étudiée à part ; GameCube / Wii / 3DS / PS2 hors 9.x.
Périmètre : Windows natif, puis macOS Intel (OpenGL ≤ 4.1), puis Linux natif ; **WSL / WSLg, Docker et noVNC restent
en rendu logiciel**. Machine d'essai : HP ProBook 640 G8, i7-1165G7, Iris Xe, 16 Go, pilote 32.0.101.7088.

## Décision

### 1. Activation : une capacité calculée, un seul point de refus

- `emu-libretro` reçoit à `Core::load` une **capacité** `HwSupport` (types et versions servis). `HwSupport::NONE` =
  comportement 8.0 exact : demande mémorisée, refusée (`false`). C'est le seul réglage qui distingue « refus » et
  « acceptation » ; le refus n'est jamais décidé ailleurs.
- La capacité est calculée par `emu-runtime` (`hw_support(environment, renderer)`) :

  | Situation | Capacité | Raison affichée |
  |---|---|---|
  | Linux sous WSL (`WSL_DISTRO_NAME`, `/proc/sys/fs/binfmt_misc/WSLInterop` ou `/proc/version`) | aucune | « non disponible sous WSL : rendu logiciel seulement » |
  | Docker (`/.dockerenv` ou `EMU_RUST_DOCKER=1`, posé par l'image) | aucune | « non disponible dans l'image Docker » |
  | Pilote logiciel (`GL_RENDERER` contient `llvmpipe`, `softpipe`, `swrast`, `Software Rasterizer`) | aucune | « pas de pilote graphique matériel » |
  | Réglage « rendu matériel » coupé par l'utilisateur | aucune | « désactivé dans les réglages » |
  | Windows, Linux natif | OpenGL compatibilité et core jusqu'à la version du pilote | — |
  | macOS Intel | OpenGL **core** ≤ 4.1 seulement ; profil de compatibilité refusé | « non disponible sur macOS » |

- Lancement direct (fenêtre pas encore créée) : capacité **statique** (environnement + OS, sans `GL_RENDERER`),
  affinée à l'attachement (§4). Mode bibliothèque : capacité exacte, la fenêtre existe.
- Les tests passent la capacité **explicitement** (`EmulatorOptions::hw_support`) ; l'interface accepte un drapeau
  caché `--hw-render=force` (tests et captures Xvfb uniquement, journalisé en avertissement), seul moyen de servir
  le rendu matériel sur llvmpipe. Aucune variable d'environnement lue par les tests de bibliothèque.
- **Refus propre** : demande non servie ⇒ `false` au core, qui se rabat sur le logiciel s'il le sait (swanstation en
  `Auto`), sinon `RuntimeError::HardwareRenderRequired` (8.0) dont le message dit **la raison** (tableau ci-dessus)
  au lieu de « prévu en 9.0 ». Une demande acceptée puis impossible à servir à l'attachement ⇒
  `RuntimeError::HwContext` et la proposition « Relancer en rendu logiciel » (même jeu, capacité `NONE`).

### 2. Contexte OpenGL : un contexte par core, partagé avec celui de l'interface (option B), repli par relecture (C)

| Option | Verdict | Raison |
|---|---|---|
| A. Le core dessine dans le contexte de l'interface | rejetée | pas de profil de compatibilité, version figée à celle de l'interface, état GL laissé par le core à remettre partout (egui, `FrameRenderer`) : défauts discrets |
| **B. Contexte du core dans le groupe de partage de l'interface** | **retenue** | sert exactement le type et la version demandés ; états GL isolés ; `SET_HW_SHARED_CONTEXT` satisfait d'office ; détruire le contexte du core ne touche jamais l'interface |
| C. Contexte isolé, relecture `glReadPixels` à chaque image | **repli automatique** | si la création partagée échoue ; plusieurs ms par image en haute résolution, signalé dans Stats et dans le rapport de mesure |

- Le contexte du core est créé avec la **même configuration** (`glutin::config::Config`) que la fenêtre et rendu
  courant **sur la surface de la fenêtre** ; il ne dessine que dans son FBO. Les FBO et VAO ne sont pas partagés,
  les textures le sont : FBO créé **dans le contexte du core**, texture de couleur lue par l'interface.
- Synchronisation : à la fin de chaque entrée dans le core qui a produit une image, `glFenceSync` dans le contexte du
  core, puis `glWaitSync` (attente côté GPU, pas de blocage du processeur) dans celui de l'interface avant de lire la
  texture (règle des objets partagés, annexe D de la spécification OpenGL).
- Validé par un **prototype jetable** en tête de palier (llvmpipe dans le conteneur, puis Iris Xe chez l'utilisateur).
  Si B échoue sur l'Iris Xe, C devient le chemin normal de la 9.0 et l'ADR est amendée ; l'API publique ne change pas.

### 3. Versions servies

- `OPENGL_CORE` (3) : la version demandée, profil core ; macOS ≤ 4.1 (sinon refus « non disponible sur macOS »).
- `OPENGL` (1) : profil de compatibilité, version demandée (2.x accepté) ; **refusé sur macOS** (pas de partage
  possible entre un contexte 2.1 hérité et notre contexte 4.1 core [M]).
- `OPENGLES*` (2, 4, 5), `D3D9/10/12` (10, 8, 9) : refusés dans toute la 9.x. `D3D11` (7) : palier DirectX.
  `VULKAN` (6) : dernier palier.
- `GET_PREFERRED_HW_RENDER` (56) : répond `OPENGL_CORE` quand la capacité sert OpenGL, décline sinon (valeur de retour
  exacte à vérifier dans `libretro.h` au lot FFI [M]). Avec D3D11 (palier DirectX) : `D3D11` seulement si l'utilisateur
  l'a choisi pour ce core.
- Les demandes successives d'un core qui essaie plusieurs types (swanstation : jusqu'à 15 [C, ADR 0021]) sont toutes
  **journalisées** (16 au plus) ; la première servie est acceptée. Une demande **après** `retro_load_game` est refusée
  et journalisée (« redémarrez le jeu »).
- `SET_HW_SHARED_CONTEXT` (44) : `true`. `GET_HW_RENDER_INTERFACE` (41) : `false` pour OpenGL.
  `SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE` (43) et sa version (73) : refusés jusqu'au palier Vulkan.
- `cache_context` et `debug_context` : ignorés et journalisés (le contexte est toujours recréé ; pas de contexte de
  débogage en 9.x).

### 4. Cycle de vie et propriété

- **Ordre** : `Core::load` (le core appelle `SET_HW_RENDER` dans `retro_load_game`, acceptée) → la fenêtre existe →
  `Emulator::attach_gl(&GlWindow)` : contexte du core, FBO, puis `context_reset` (contexte du core courant) →
  `run_frame`… → destruction de l'`Emulator` : contexte du core courant, `context_destroy`, `retro_unload_game`,
  `retro_deinit`, retour au contexte de l'interface, destruction du contexte du core, puis déchargement de la
  bibliothèque (ADR 0022 inchangé). L'ordre « `context_destroy` avant `retro_unload_game` » est celui de RetroArch [M,
  à vérifier au lot FFI] ; le contexte reste vivant et courant jusqu'à `retro_deinit` dans tous les cas.
- **Toute entrée dans le core d'une session matérielle** (`retro_run`, `serialize`, `unserialize`, `reset`, réglage
  d'options suivi de `run`, `context_reset/destroy`, `unload_game`, `deinit`) se fait **contexte du core courant**, par
  une garde RAII qui rend le contexte de l'interface courant à la sortie, panique comprise. Tout appel `glow` existant
  continue de supposer le contexte de l'interface.
- L'**hôte de rendu** (trait sûr `HwRenderHost`, implémenté par `emu-runtime`) est installé dans l'état global
  d'`emu-libretro` de `attach_gl` à la destruction (et non pendant `run` seulement, amendement d'ADR 0008) :
  `get_current_framebuffer` et `get_proc_address` peuvent être appelés pendant `context_reset`, `serialize`…
  Appelés depuis un autre thread : valeur neutre (0, nul), journalisés une fois (règle d'ADR 0008).
- **Sûreté par construction de l'ordre de destruction** : la fenêtre `winit` et la surface passent dans un
  `Rc<GlTarget>` partagé par `GlWindow` et le contexte du core ; quel que soit l'ordre des champs de `App`, la
  surface vit tant qu'un contexte peut y être rendu courant. L'interface détruit malgré tout la session matérielle
  **avant** la fenêtre (`App::teardown`, `App::detach` du mode bibliothèque).
- Pas de réattachement d'une session détruite : « Redémarrer », changer de jeu ou de core recréent tout (ADR 0022).

### 5. Image : FBO, sentinelle, affichage, relecture

- FBO du core : texture couleur RGBA8 de `max_width × max_height` (`AvInfo`), plus un `DEPTH24_STENCIL8` si
  `depth` ou `stencil`. `get_current_framebuffer` renvoie **toujours le même nom** de FBO (des cores le gardent en
  cache [M]). Taille au-delà de `GL_MAX_TEXTURE_SIZE` ou allocation refusée ⇒ `HwContext` « résolution interne trop
  élevée ».
- `SET_SYSTEM_AV_INFO` / `SET_GEOMETRY` avec un maximum plus grand : pièces jointes réallouées **après** `retro_run`,
  même nom de FBO. Échec ⇒ message « redémarrez le jeu ».
- `video_refresh(RETRO_HW_FRAME_BUFFER_VALID, w, h, _)` : nouvelle méthode `FrontendHandler::hardware_frame(w, h)`
  (par défaut sans effet) ; `w × h` est la partie utile, `bottom_left_origin` décide du retournement. `NULL` = image
  répétée. Sentinelle reçue **sans hôte installé** : chemin 8.0 (`FrameKind::HwSentinel`, refus).
- L'image d'une session matérielle **n'est pas en mémoire** : `Emulator::video()` rend `Software(&FrameRgba)` ou
  `Hardware(HwFrame)` (texture partagée, partie utile, retournement, identifiant du groupe de partage).
  `GlWindow::draw_frame` affiche l'un ou l'autre par le même `FrameRenderer` (mise à l'échelle, rapport d'aspect).
- `Emulator::read_frame()` relit le FBO **à la demande** (`glReadPixels` dans le contexte du core) : captures du jeu,
  `--screenshot`, empreintes de test. Jamais à chaque image, sauf repli C. La capture de fenêtre
  (`GlWindow::capture`) est inchangée et contient l'image du core.

### 6. Plein écran, fenêtre, perte de contexte

- Bascule plein écran / fenêtre : `Fullscreen::Borderless` sur la **même** fenêtre, aucune recréation de contexte,
  aucun `context_reset`. Pas de plein écran exclusif en 9.x. Redimensionnement : surface seulement, le FBO suit
  l'`AvInfo`, pas la fenêtre.
- Perte de contexte (réinitialisation du pilote sous Windows) : **non récupérée en 9.x**. Détection au mieux
  (`glGetError` = `GL_CONTEXT_LOST` après l'échange, si le pilote le rapporte) ⇒ partie arrêtée, SRAM écrite si
  possible, message « le pilote graphique a été réinitialisé : relancez le jeu ». Pas de contexte « robuste ».

### 7. Sauvegardes d'état, réinitialisation, options

- `serialize` / `unserialize` / `reset` d'une session matérielle : contexte du core courant (§4). Les états restent
  propres au core (comme aujourd'hui) ; un état pris à ×2 se recharge à ×4 si le core le permet (états en résolution
  native [M]).
- Vignette d'état et captures : `read_frame()`.
- Options qui changent le moteur de rendu du core (`swanstation_GPU_Renderer`…) : appliquées **au prochain
  lancement** (« redémarrez le jeu ») ; la résolution interne est appliquée en cours de jeu si le core la prend sans
  nouvelle demande `SET_HW_RENDER`, sinon au prochain lancement. Le menu de l'option l'indique.

### 8. Interface toujours en OpenGL ; place de Direct3D 11 et de Vulkan

- L'interface (egui, `FrameRenderer`, captures) **reste en OpenGL sur toutes les cibles pendant toute la 9.x**. Tout
  moteur de core finit en **texture OpenGL du groupe de partage de l'interface**.
- Dans `emu-runtime`, un trait interne `HwBackend` porte un moteur : `GlShared` (9.0), `GlReadback` (repli, 9.0),
  `D3d11Bridge` (`#[cfg(windows)]`, palier DirectX : périphérique D3D11 créé par nous, copie vers une texture GL,
  puis `WGL_NV_DX_interop2` si le pilote Intel l'expose [M] ; dépendance `windows` décidée à ce palier),
  `VulkanReadback` (dernier palier ; puis `GL_EXT_memory_object` si disponible [M]).
- Côté FFI, `HwRenderHost` s'étendra par **méthodes à implémentation par défaut** (`render_interface()` pour
  `GET_HW_RENDER_INTERFACE`, négociation Vulkan) : ajout non cassant.
- Écartés : interface entièrement en D3D11 (deuxième moteur d'interface), ANGLE (DLL tierces, contextes ES), `wgpu`
  (pas de périphérique D3D11 à donner au core [M]). À rouvrir seulement si la mesure du pont l'exige (nouvelle ADR).
- La mesure sépare toujours le temps du core de celui de la copie du pont, pour ne pas pénaliser DirectX à tort.

### 9. Mesure : compteur et « Mesurer 60 s »

- Mesures par image, dès le rendu logiciel : temps de `retro_run`, temps GPU du core (deux `glQueryCounter`
  `GL_TIMESTAMP` dans le contexte du core, lus deux images plus tard : jamais d'attente, pas d'imbrication avec les
  requêtes du core), temps de présentation (affichage, egui, échange). Actives seulement quand le panneau Stats est
  ouvert ou qu'une mesure tourne.
- « Mesurer 60 s » (menu en jeu, et `--measure <s>` en ligne de commande après 10 s de chauffe) écrit un rapport
  texte dans `<data>/measures/` et propose « Copier le rapport ». Contenu : versions (emu-rust, core), OS,
  `GL_RENDERER`, `GL_VERSION`, chemin de rendu (logiciel, GL partagé, GL relu, plus tard D3D11), résolution interne,
  vsync, synchro, taille de fenêtre, nombre d'images, FPS moyen, 1 % bas, images > 16,7 ms, temps core / GPU /
  présentation (moyenne, p99, max), sous-alimentations audio.
- C'est ce rapport, même jeu, même scène, qui décide de la place de DirectX 11 (réponse 5 de l'utilisateur).

### 10. Tests automatiques

- **Core factice à rendu matériel** (ADR 0009, `tests/mock-core/mock_core.c`) : demande un contexte, charge ses
  fonctions par `get_proc_address`, efface et dessine des rectangles `glScissor` + `glClear` (résultat exact quel
  que soit le pilote), sentinelle, origine en bas à gauche, profondeur, agrandissement de l'`AvInfo`, état
  sérialisable, journal du cycle de vie. Empreintes **exactes**.
- **GL sans écran** : Mesa llvmpipe, contexte EGL sans surface (`glutin`, plate-forme « device » ou « surfaceless »
  [M]) ; repli : fenêtre cachée sous Xvfb (déjà utilisé). OSMesa écarté.
- **Vrais cores** (`#[ignore]`) : swanstation en OpenGL sur llvmpipe à ×1 et ×2, homebrew de `tests/` ; empreintes
  **à tolérance** (image réduite, seuil par pixel, version de Mesa notée).
- Tests sans GL d'`emu-libretro` : hôte factice (trait `HwRenderHost`), le core factice en mode « sans dessin ».
- CI : Linux exécute tout (Xvfb + llvmpipe) ; macOS exécute si le contexte 4.1 logiciel d'Apple est disponible,
  sinon ignore avec raison [M] ; Windows ignore les tests GL matériels (pas de pilote) ; D3D11 par WARP au palier
  DirectX [M]. Vulkan : lavapipe au dernier palier (`mesa-vulkan-drivers`).
- Conséquence assumée : sous Windows, cible principale, la vérification automatique est faible ; l'essai de
  l'utilisateur et le rapport de mesure comptent plus qu'aux versions précédentes.

### 11. `unsafe`

- Confiné à `emu-libretro` (lecture et écriture de `retro_hw_render_callback` pendant l'appel 14 ; trampolines
  `extern "C"` sans donnée utilisateur, enveloppés dans `catch_unwind` ; appels de `context_reset` /
  `context_destroy` gardés) et à `emu-runtime` (contextes `glutin`, `glow`, synchronisation, relecture, requêtes).
  Chaque bloc a son `// SAFETY:`. L'API vue d'`emu-ui` reste entièrement sûre (`HwFrame` opaque, vérifié contre le
  groupe de partage de la fenêtre).
- Invariants écrits dans le code et testés : `context_destroy` toujours appelé une fois après un `context_reset`
  réussi, avant le déchargement ; aucun pointeur du core conservé hors de l'appel sauf les deux fonctions du core
  (`context_reset`, `context_destroy`), valides tant que la bibliothèque est chargée ; contexte de l'interface
  courant à la sortie de toute entrée dans le core.
- Revue `reviewer` obligatoire de tout ce code.

### 12. Alignement sur la 8.0 (ADR 0025)

- La 8.0 mémorise la demande et la refuse en un point unique (`HardwareRenderRequired`, `LaunchCheck::HardwareRender`,
  `addability`). La 9.0 garde ces noms et **change le critère** : « refusé » devient « refusé **si la capacité de
  cette machine ne sert pas l'API** », avec la raison (§1). `RenderNeed::Hardware(apis)` est comparé à la capacité
  statique de la plate-forme ; `render = "hardware"` est accepté dans les définitions (format 1 inchangé : une 8.0 la
  refuse proprement).
- Si la 9.0 passe **avant** la 8.0 (ordre ajustable), la 9.0 fait elle-même la mémorisation (`hw_render_request`,
  `FrameKind`) et la 8.0 en hérite ; la partie « systèmes personnalisés à rendu matériel » attend alors la 8.0.
- `HwRenderRequest` (8.0 : type et version) gagne `depth`, `stencil`, `bottom_left_origin` ; à déclarer
  `#[non_exhaustive]` dès la 8.0 (note pour son architecte).

## Conséquences

- La 9.0 livre un socle réutilisé tel quel par la Nintendo 64 (9.1) et le Dreamcast (9.2) : un système nouveau en
  OpenGL ne demande plus que catalogue, cores, BIOS et tests.
- Deux contextes GL par session matérielle ; un `glFenceSync` / `glWaitSync` et deux changements de contexte par
  image (coût attendu négligeable [M], mesuré par le compteur).
- WSL, Docker, noVNC : rien ne change pour l'utilisateur sinon le message de refus, plus précis.
- macOS Intel : servi en OpenGL core ≤ 4.1 ; ce qui n'y tourne pas est grisé (réponse 8 de l'utilisateur). Aucun build
  ni essai macOS possible ici.
- Un core lourd peut ne pas tenir la cadence sur le thread de l'interface : mesuré ; un thread d'émulation reste
  hors 9.x (ADR 0010).
- Lignes « amendée par ADR 0026 » à ajouter aux ADR 0002, 0008, 0010 et 0021 à l'acceptation (lot architecte).
