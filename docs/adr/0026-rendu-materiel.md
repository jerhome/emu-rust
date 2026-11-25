# ADR 0026 — Rendu matériel (9.x) : contexte OpenGL du core, interface toujours en OpenGL

Statut : **accepté, amendé pour la fusion le 2026-11-25** (« Amendement de fusion » ci-dessous : il **prime**). Historique :
accepté le 2026-11-25 par l'utilisateur, sur la ligne PC (`wip-7.2`), après la revue indépendante et trois relectures courtes ;
réponses Q-A à Q-F et 7 questions du §11 intégrées). Questions restantes Q-G à Q-J (`docs/design/v9.0-rendu-materiel.md`
§12) : à poser avant GX1, elles ne remettent pas en cause l'ADR. Mineurs de la dernière relecture : fin de la conception. Étude : `docs/etude-9.0-rendu-materiel.md` (les marques [V], [C], [M] y
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

## Amendement de fusion (2026-11-25) — base Mac, ordre des versions

Après la fusion des lignes PC et Mac (`.claude/fusion-plan.md`, D1-D15) :

- **Ordre** : la 9.0 part de `main` après la livraison de la **8.0** (la « 7.2 » PC est livrée en 7.5 ; la 7.6
  rangement, la 7.7 archives `.7z` et la 7.8 interface en anglais la précèdent). Les paliers de la 8.x sont
  renumérotés (ADR 0025, amendement de fusion : CD 8.2, ordinateurs 8.3, arcade 8.4 ; 8.1 = core jgenesis) : le
  « point unique de refus » (§1, §12) renvoie à ces numéros.
- **Recalage du code (lot L10)** : les relevés « dans le code de `wip-7.2` » [C] sont à revérifier contre `main`
  avant GX1 (fenêtres de la 7.3, `theme.rs`, `Page`, `emu-app` bibliothèque + deux binaires de la 7.5). Le fond de
  l'ADR (contexte GL du core partagé, interface toujours en OpenGL, D3D11 et Vulkan au second plan) ne change pas.
- **Cibles** : le Mac de test (Iris 6100, OpenGL 4.1, macOS 12) reste la borne haute du profil demandé ; le PC de test
  (Iris Xe, OpenGL 3.3 sous Windows) la borne basse ; Vulkan reste hors essai réel.
- **Style** : le compteur et « Mesurer 60 s » (§9) utilisent les jetons `theme::` de la 7.3.

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

### 1. Activation : capacité de la machine, capacité du lancement, un seul point de refus

- `emu-libretro` reçoit au chargement du core une **capacité** `HwSupport` (types et versions servis).
  `HwSupport::NONE` = comportement 8.0 exact : demande mémorisée, refusée (`false`), 56 décliné. C'est le seul
  réglage qui distingue « refus » et « acceptation » ; le refus n'est jamais décidé ailleurs.
- **Capacité du lancement (amendement B1)** : non nulle **seulement** pour un lancement qui la demande
  explicitement :
  - jeu lancé en haute résolution (réglage du système ou exception du jeu) avec un core qui a un profil
    `[cores.<id>.hw]` dans `cores.toml` (9.0 : swanstation seul) ;
  - (8.0 livrée, lot GU3) système personnalisé dont la définition déclare `render = "hardware"` ;
  - drapeau caché `--hw-render=force` (tests, captures Xvfb et essai Mac de l'utilisateur GE3 tant que la HD y est
    grisée ; seulement en ligne de commande, journalisé en avertissement).

  **Tout autre lancement reçoit `HwSupport::NONE`** : 14 refusé et 56 décliné exactement comme en 7.2. Raison :
  des cores livrés demandent déjà `SET_HW_RENDER` et se rabattent sur le logiciel **parce qu'il est refusé** :
  dosbox_pure pour la Voodoo, avec un thread interne (ADR 0018), swanstation en `Auto` (ADR 0021), tout core qui
  sonde 56. Leur servir un contexte sans l'avoir voulu casserait des systèmes livrés (écran noir, plantage).
- **Capacité de la machine**, calculée par `emu-runtime` (`hw_support(environment, renderer, enabled)`) une fois
  la fenêtre créée (le pilote est connu) ; la capacité du lancement est l'une ou `NONE` :

  | Situation | Capacité | Raison affichée |
  |---|---|---|
  | Linux sous WSL (`WSL_DISTRO_NAME`, `/proc/sys/fs/binfmt_misc/WSLInterop` ou `/proc/version`) | aucune | « non disponible sous WSL : rendu logiciel seulement » |
  | Docker (`/.dockerenv` ou `EMU_RUST_DOCKER=1`, posé par l'image) | aucune | « non disponible dans l'image Docker » |
  | Pilote logiciel (`GL_RENDERER` contient `llvmpipe`, `softpipe`, `swrast`, `Software Rasterizer`, `Apple Software Renderer`, `GDI Generic`, `Microsoft Basic Render`, `SwiftShader`) | aucune | « pas de pilote graphique matériel » |
  | Réglage « rendu matériel » coupé par l'utilisateur | aucune | « désactivé dans les réglages » |
  | Windows, Linux natif | OpenGL compatibilité et core jusqu'à la version du pilote | — |
  | macOS Intel | OpenGL **core** ≤ 4.1 seulement ; profil de compatibilité refusé | « non disponible sur macOS » |

  Le refus sous WSL est fondé sur l'**environnement**, jamais sur `GL_RENDERER` : sous WSLg, Mesa d3d12
  (`D3D12 (<carte>)`) rapporte un vrai GPU (m10). Linux natif est livré **« non essayé »** : aucune machine Linux
  avec carte graphique, tests llvmpipe seulement (réponse Q-C).
- **Lancement direct en haute résolution (amendement M5)** : la fenêtre est créée **avant** `Core::load` quand le
  lancement prévoit la haute résolution ; la capacité est donc toujours exacte et la version demandée par le core
  toujours comparée au pilote. La capacité « statique » (sans `GL_RENDERER`) est **supprimée**, sauf pour
  `addability` de la 8.0 (aucune fenêtre : environnement et plate-forme seulement, sans promesse).
- Les tests passent la capacité **explicitement** par de nouveaux points d'entrée (`Core::load_with_hw`,
  `Emulator::load_content_hw`) ; les points d'entrée existants restent et servent `NONE` : aucun littéral
  `EmulatorOptions` / `CoreConfig` existant ne change (m11). Aucune variable d'environnement lue par les tests
  de bibliothèque.
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

- Le contexte du core est créé avec la **même configuration** (`glutin::config::Config`) que la fenêtre. Il est
  rendu courant **sans surface** quand la plate-forme le permet (`make_current_surfaceless` : EGL avec
  `KHR_surfaceless_context`, CGL sur macOS) et **sur la surface de la fenêtre** sinon (WGL, GLX) [M, amendement
  M3]. Il ne dessine que dans son FBO. Les FBO et VAO ne sont pas partagés, les textures le sont : FBO créé **dans le
  contexte du core**, texture de couleur lue par l'interface.
- Conséquence de la surface de la fenêtre (WGL, GLX) : le FBO 0 du core est le tampon arrière de la fenêtre ; un core
  qui lie 0 dessine dans notre fenêtre. Sans effet visible tant que l'interface redessine toute l'image avant
  l'échange (règle à garder dans `draw_frame`), journalisé si le prototype le constate. Sans surface, lier 0 ne
  dessine rien. Deux contextes courants tour à tour sur la même `NSView` étant fragiles [M], macOS passe par le
  contexte sans surface ou n'est pas servi (HD grisée jusqu'à l'essai Mac, réponse Q-B).
- **Synchronisation (amendement M4)** : à la sortie de chaque entrée dans le core qui a produit une image, dans le
  contexte du core : `glFenceSync` **puis `glFlush`** (sans ce `glFlush`, `glWaitSync` de l'autre contexte peut
  ne jamais se débloquer sur un seul thread). Dans le contexte de l'interface : `glWaitSync` (attente côté GPU),
  **puis** liaison de la texture (une texture partagée se relie après l'attente : règle des objets partagés, annexe D
  de la spécification OpenGL), puis `glDeleteSync`. **Un seul emplacement de barrière par session**
  (`Option<GLsync>`, partagé entre l'hôte et la fenêtre) ; **chaque consommateur le vide (`take`) avant de
  supprimer** : la nouvelle barrière de `leave` remplace et supprime la précédente non consommée (avance rapide :
  plusieurs `retro_run` par affichage, ADR 0010 ; fenêtre réduite : aucun affichage), `show_hw_frame` attend puis
  supprime celle qu'il a prise, `release_gl` supprime la dernière. Une image réaffichée sans nouvelle barrière
  (pause et menu egui : même `HwFrame`, ou image répétée `NULL`) **n'attend pas** : jamais de double
  `glDeleteSync`, jamais de croissance sans limite (3e relecture, majeur 2). Contexte
  du core sans objets de synchronisation
  (compatibilité 2.1 ou 3.0 sans `ARB_sync`) : `glFinish` dans le contexte du core à la place de la barrière
  (coût mesuré par le compteur, test headless en compatibilité 3.0).
- **Groupe de partage et fuites (amendement M2)** : les objets GL qu'un core crée sans les libérer dans
  `context_destroy` appartiennent au groupe de partage de l'interface et **survivent** à la session (mode
  bibliothèque : une session après l'autre dans le même contexte d'interface). L'hôte supprime lui-même tout ce qu'il
  a créé (FBO, texture, renderbuffer, barrières, requêtes), **contexte du core courant** (`enter`, suppression,
  `leave` ; `enter` en échec ⇒ rien n'est supprimé, des noms lus dans le mauvais contexte désigneraient les objets
  d'egui), avant de détruire le contexte du core ; il ne peut rien pour
  les objets du core. Parade par core : `isolated = true` dans `[cores.<id>.hw]` ⇒ chemin C pour ce core (contexte
  isolé, libéré en entier à sa destruction). Constat : core factice `MOCK_HW_LEAK`, dix relancements avec relevé
  mémoire au prototype (GP0, GE1) et à l'essai réel (GE2).
- Validé par un **prototype jetable** en tête de palier (llvmpipe dans le conteneur, puis Iris Xe chez l'utilisateur) :
  partage avec et sans surface, barrière en core 3.3 et repli `glFinish` en compatibilité 3.0, dix créations et
  destructions. Si B échoue sur l'Iris Xe, C devient le chemin normal de la 9.0 et l'ADR est amendée ; l'API publique
  ne change pas.

### 3. Versions servies

- `OPENGL_CORE` (3) : la version demandée, profil core ; macOS ≤ 4.1 (sinon refus « non disponible sur macOS »).
  C'est le seul type dont la version est comparée à la capacité.
- `OPENGL` (1) : profil de compatibilité ; `libretro.h` ne définit pas de version pour ce type : les champs de
  version sont **ignorés** et le contexte est le contexte de compatibilité le plus haut que donne le pilote (m1).
  **Refusé sur macOS** (pas de partage possible entre un contexte 2.1 hérité et notre contexte 4.1 core [M]).
- `OPENGLES*` (2, 4, 5), `D3D10` (8), `D3D12` (9), `D3D9` (10) et toute valeur plus haute : refusés dans toute
  la 9.x. `D3D11` (7) : palier DirectX. `VULKAN` (6) : dernier palier. `NONE` (0) : refusé sans journal
  d'erreur. Type Rust : `HwContextKind` a `Direct3D11` (7) et `Unknown(u32)` pour le reste ; le type 8.0
  (`Direct3D`) est aligné sur ce nom (m4, note pour l'architecte 8.0).
- `GET_PREFERRED_HW_RENDER` (56), seulement quand la capacité du lancement n'est pas `NONE` : la valeur est
  **écrite même quand la réponse est `false`** ; compatibilité et core servis ⇒ `OPENGL_CORE`, `true` ; core
  seul (macOS) ⇒ `OPENGL_CORE`, `false` ; `NONE` ⇒ rien d'écrit, `false` (m2). Avec D3D11 (palier DirectX) :
  `D3D11` seulement si l'utilisateur l'a choisi pour ce core.
- Lecture de `retro_hw_render_callback` : les `bool` C sont lus en `u8` et convertis par `!= 0` (toute valeur
  d'octet est valide, pas celle d'un `bool` Rust), comme `SET_FASTFORWARDING_OVERRIDE` (m5).
- Les demandes successives d'un core qui essaie plusieurs types (swanstation : jusqu'à 15 [C, ADR 0021]) sont toutes
  **journalisées** (16 au plus) ; la première servie est acceptée. Une demande **après** `retro_load_game` est refusée
  et journalisée (« redémarrez le jeu »).
- `SET_HW_SHARED_CONTEXT` (44) : `true`. `GET_HW_RENDER_INTERFACE` (41) : `false` pour OpenGL.
  `SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE` (43) et sa version (73) : refusés jusqu'au palier Vulkan.
- `cache_context` et `debug_context` : ignorés et journalisés (le contexte est toujours recréé ; pas de contexte de
  débogage en 9.x).

### 4. Cycle de vie et propriété

- **Ordre** : fenêtre créée (§1) → `Core::load_with_hw` (le core appelle `SET_HW_RENDER` dans `retro_load_game`,
  acceptée) → `Emulator::attach_gl(&GlWindow)` : contexte du core, FBO, puis `context_reset` (contexte du core
  courant) → `run_frame`… → fin de session.
- **Fin de session (amendement M1)**, séquences de `emu-libretro` (`Emulator::drop` appelle `unload_game` avant le
  `Drop` de `Core`, l'ordre doit donc tenir dans chacune) :
  - `Core::unload_game` : `enter` → `context_destroy` (si non nul et après un `context_reset` réussi) →
    `retro_unload_game` → `leave` ;
  - `Core::shutdown` (appelé par `Drop`, idempotent) : `unload_game` si besoin, `enter` → `retro_deinit` →
    `leave`, hôte détruit, puis bibliothèque fermée (ADR 0022 inchangé) ;
  - **règle : jamais d'appel au core avec le contexte de l'interface courant** (un core qui appelle `glDelete*` ou
    `glBind*` casserait les objets d'egui ; `catch_unwind` ne rattrape pas une faute mémoire). Seule exception
    nommée : les accesseurs de mémoire (`retro_get_memory_data` / `retro_get_memory_size`, écriture de la SRAM,
    `emulator.rs` vers la ligne 831), qui ne touchent pas au GL et restent permis hors `enter` ;
  - `enter` en échec à la fin (relecture, majeur 1) ⇒ le core **n'est plus appelé** : ni `context_destroy`, ni
    `retro_unload_game`, ni `retro_deinit` ; la bibliothèque reste chargée (fuite assumée) et est inscrite comme
    **résidente** (ADR 0022) : le chargement suivant du même core passe par la copie privée. SRAM déjà écrite avant
    la fin (lecture de la mémoire du core, sans GL). Journalisé, testé avec l'hôte factice ;
  - `leave() -> bool` : `false` ⇒ la session est arrêtée (`HwContext`) et **finit comme l'abandon ci-dessus** :
    plus aucun appel au core (ni `context_destroy`, ni `retro_unload_game`, ni `retro_deinit`), bibliothèque
    laissée chargée et inscrite résidente. L'interface réessaie **tout de suite** de rendre son contexte courant ;
    en cas d'échec elle ne dessine pas cette image, réessaie à l'image suivante, puis affiche « contexte graphique
    perdu » et ferme proprement la fenêtre (SRAM déjà écrite). Dans `Drop`, `enter` et `leave` sont
    enveloppés dans `catch_unwind` (jamais de panique dans un `Drop`).

  L'ordre « `context_destroy` avant `retro_unload_game` » est celui de RetroArch (vérifié par la revue) ; le
  contexte du core reste vivant jusqu'à `retro_deinit`, ou jusqu'à `release_gl` en cas d'abandon.
- Threads d'un core abandonné (bibliothèque laissée chargée, sans `retro_deinit`) : ils peuvent continuer à appeler
  nos callbacks ; règle d'ADR 0008 inchangée, tout appel depuis un autre thread reçoit une valeur neutre et est
  journalisé une fois.
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
  surface vit tant qu'un contexte peut y être rendu courant. L'interface termine malgré tout la session matérielle
  **avant** la fenêtre par `Emulator::release_gl()` (m13) : idempotente, elle écrit la SRAM, appelle
  `unload_game` puis `shutdown` sous le contexte du core, supprime nos objets GL sous ce même contexte (§2) et
  détruit ce contexte ; l'`Emulator` devient inerte
  (`run_frame` ⇒ `HwContext`) et son `Drop` ne refait rien. `App::teardown` et `App::detach` l'appellent ;
  `App.emulator` n'a pas à devenir un `Option`.
- Pas de réattachement d'une session détruite : « Redémarrer », changer de jeu ou de core recréent tout (ADR 0022).

### 5. Image : FBO, sentinelle, affichage, relecture

- FBO du core : texture couleur RGBA8 de `max_width × max_height` (`AvInfo`) ; `depth` et `stencil` ⇒
  `DEPTH24_STENCIL8` ; `depth` seul ⇒ profondeur seule (`DEPTH_COMPONENT24`) ; `stencil` seul est invalide
  selon `libretro.h` et ignoré (journalisé) (m3). L'alpha du FBO est quelconque (bit de masque de la PS1) :
  l'affichage l'ignore et `read_frame` le force à 255 (m8). `get_current_framebuffer` renvoie **toujours le même nom** de FBO (des cores le gardent en
  cache [M]). Taille au-delà de `GL_MAX_TEXTURE_SIZE` ou allocation refusée ⇒ `HwContext` « résolution interne trop
  élevée ».
- `SET_SYSTEM_AV_INFO` / `SET_GEOMETRY` avec un maximum plus grand : pièces jointes réallouées **après** `retro_run`,
  même nom de FBO. Conséquence assumée : l'image de ce `retro_run`-là peut être rognée (une image) (m9). Échec ⇒
  message « redémarrez le jeu ».
- `video_refresh(RETRO_HW_FRAME_BUFFER_VALID, w, h, _)` : nouvelle méthode `FrontendHandler::hardware_frame(w, h)`
  (par défaut sans effet) ; `w × h` est la partie utile, `bottom_left_origin` décide du retournement. `NULL` = image
  répétée. Sentinelle reçue **sans hôte installé** : chemin 8.0 (`FrameKind::HwSentinel`, refus).
- L'image d'une session matérielle **n'est pas en mémoire** : `Emulator::video()` rend `Software(&FrameRgba)` ou
  `Hardware(HwFrame)` (texture partagée, partie utile, retournement, identifiant du groupe de partage et
  **identifiant de session** : un `HwFrame`, `Copy`, peut survivre à sa session ; `show_hw_frame` refuse celui
  d'une session terminée, m7).
  `GlWindow::draw_frame` affiche l'un ou l'autre par le même `FrameRenderer` (mise à l'échelle, rapport d'aspect).
- `Emulator::read_frame()` relit le FBO **à la demande** (`glReadPixels` dans le contexte du core) : captures du jeu,
  `--screenshot`, empreintes de test. Jamais à chaque image, sauf repli C. La capture de fenêtre
  (`GlWindow::capture`) est inchangée et contient l'image du core.

### 6. Plein écran, fenêtre, perte de contexte

- Bascule plein écran / fenêtre : `Fullscreen::Borderless` sur la **même** fenêtre, aucune recréation de contexte,
  aucun `context_reset`. Pas de plein écran exclusif en 9.x. Redimensionnement : surface seulement, le FBO suit
  l'`AvInfo`, pas la fenêtre.
- Perte de contexte (réinitialisation du pilote sous Windows) : **ni détectée ni récupérée en 9.x** (m6) :
  `GL_CONTEXT_LOST` n'est rapporté qu'avec un contexte « robuste », exclu en 9.x. Effet possible : écran noir ou
  arrêt du core ; l'utilisateur relance le jeu. Pas de variante d'erreur dédiée.

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
  vsync, synchro, taille de fenêtre, **cadence du jeu** (`av_info.timing.fps`) et **fréquence de l'écran**, nombre
  d'images, FPS moyen, 1 % bas (FPS de la pire fenêtre d'une seconde : sur 60 s, la pire des 60 ; jamais calculé
  sur les intervalles, faussés par la vsync), images lentes, temps core / GPU / présentation (moyenne, p99, max),
  sous-alimentations audio.
- **Seuils relatifs à la cadence du jeu (amendement M7, réponses Q-D et 6)** : GTA est en PAL (50 i/s), un seuil fixe
  à 60 i/s serait toujours franchi. « OpenGL mauvais » à ×2 = FPS moyen **< 97 % de la cadence** (48,5 i/s en PAL),
  **ou plus de 2 % d'images lentes**, ou saccades visibles. Une image est **lente** quand son **temps de travail**
  (`retro_run` + GPU du core + présentation **hors attente de vsync**) dépasse **une période** (20 ms en PAL) :
  l'intervalle entre deux images ne sert pas, car un jeu à 50 i/s sur un écran à 60 Hz avec vsync alterne 16,7 et
  33,3 ms sans aucune lenteur. **En attente de la question Q-J** de la conception (ci-dessus : son défaut). La
  présentation hors attente de vsync n'est pas mesurable directement [M] : on prend l'instant juste avant
  `swap_buffers` (affichage et egui soumis), méthode fixée et vérifiée au lot GR2 ; les intervalles entre images
  restent dans le rapport pour information. Les seuils chiffrés sont le critère principal ; le ressenti de
  l'utilisateur compte aussi un peu.
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
- **Chemin fenêtre réel** (`GlWindow` + contexte du core partagé, sous Xvfb) : test d'intégration dans
  `emu-runtime/tests/` dès le lot du socle, pas seulement des captures (m16).
- **Non-régression des cores livrés qui demandent déjà le rendu matériel** (B1) : lancement sans haute résolution ⇒
  capacité `NONE` (test unitaire du plan de lancement) ; dosbox_pure avec la Voodoo, réel `#[ignore]` : refus et
  repli logiciel inchangés.
- CI : Linux exécute tout (Xvfb + llvmpipe) ; macOS exécute si le contexte 4.1 logiciel d'Apple est disponible,
  sinon ignore avec raison [M] ; Windows ignore les tests GL matériels (pas de pilote) mais **compile et passe
  `clippy` sur `x86_64-pc-windows-msvc`** (seule vérification du code `#[cfg(windows)]` : aucune cible Windows
  dans `scripts/`, m12) ; D3D11 par WARP au palier DirectX [M]. Vulkan : lavapipe au dernier palier
  (`mesa-vulkan-drivers`).
- Conséquence assumée : sous Windows, cible principale, la vérification automatique est faible ; l'essai de
  l'utilisateur et le rapport de mesure comptent plus qu'aux versions précédentes.

### 11. `unsafe`

- Confiné à `emu-libretro` (lecture et écriture de `retro_hw_render_callback` pendant l'appel 14 ; trampolines
  `extern "C"` sans donnée utilisateur, enveloppés dans `catch_unwind` ; appels de `context_reset` /
  `context_destroy` gardés) et à `emu-runtime` (contextes `glutin`, `glow`, synchronisation, relecture, requêtes).
  Chaque bloc a son `// SAFETY:`. L'API vue d'`emu-ui` reste entièrement sûre (`HwFrame` opaque, vérifié contre le
  groupe de partage de la fenêtre).
- Invariants écrits dans le code et testés : `context_destroy` toujours appelé une fois après un `context_reset`
  réussi, avant le déchargement, sauf abandon (`enter` en échec ou `leave` faux, §4 : le core n'est plus appelé et la
  bibliothèque n'est pas déchargée) ; aucun pointeur du core conservé hors de l'appel sauf les deux fonctions du core
  (`context_reset`, `context_destroy`), valides tant que la bibliothèque est chargée ; contexte de l'interface
  courant à la sortie de toute entrée dans le core ; `get_proc_address` rend un pointeur de fonction, converti du
  `*const c_void` de l'hôte dans `emu-libretro` seulement, sous `// SAFETY:` (m5).
- Revue `reviewer` obligatoire de tout ce code.

### 12. Alignement sur la 8.0 (ADR 0025)

- La 8.0 mémorise la demande et la refuse en un point unique (`HardwareRenderRequired`, `LaunchCheck::HardwareRender`,
  `addability`). La 9.0 garde ces noms et **change le critère** : « refusé » devient « refusé **si la capacité de
  cette machine ne sert pas l'API** », avec la raison (§1). `RenderNeed::Hardware(apis)` est comparé, pour
  `addability` seulement, à la capacité de la plate-forme sans fenêtre (environnement et OS) ; `render = "hardware"` est accepté dans les définitions (format 1 inchangé : une 8.0 la
  refuse proprement).
- Si la 9.0 passe **avant** la 8.0 (ordre ajustable), la 9.0 fait elle-même la mémorisation (`hw_render_request`,
  `FrameKind`) et la 8.0 en hérite ; la partie « systèmes personnalisés à rendu matériel » attend alors la 8.0.
- `HwRenderRequest` (8.0 : type et version) gagne `depth`, `stencil`, `bottom_left_origin` ; à déclarer
  `#[non_exhaustive]` dès la 8.0 (note pour son architecte), ainsi que le nom `HwContextKind::Direct3D11` (§3).
- Ordre des versions (réponse Q-E) : la 9.0 part de `main` **après** la fusion de la 7.2 et la livraison de la 8.0 ;
  seuls le prototype (branche `proto-9.0`) et l'image Docker / CI (`EMU_RUST_DOCKER`, paquets EGL) partent avant.
  Le cas « 9.0 avant 8.0 » ci-dessus n'est gardé que pour mémoire.

## Conséquences

- La 9.0 livre un socle réutilisé tel quel par la Nintendo 64 (9.1) et le Dreamcast (9.2) : un système nouveau en
  OpenGL ne demande plus que catalogue, cores, BIOS et tests.
- Deux contextes GL par session matérielle ; un `glFenceSync` / `glWaitSync` et deux changements de contexte par
  image (coût attendu négligeable [M], mesuré par le compteur).
- WSL, Docker, noVNC : rien ne change pour l'utilisateur sinon le message de refus, plus précis.
- Systèmes livrés : rien ne change hors haute résolution choisie (§1, tableau ci-dessous) ; les cores qui demandent
  déjà le rendu matériel (dosbox_pure, swanstation en `Auto`) gardent leur repli logiciel.
- macOS Intel : servi en OpenGL core ≤ 4.1 ; ce qui n'y tourne pas est grisé (réponse 8 de l'utilisateur). Haute
  résolution PlayStation **grisée sur Mac** (`macos = false`) jusqu'à un essai réussi de l'utilisateur sur son Mac
  Intel (réponse Q-B). Aucun build ni essai macOS possible ici.
- Linux natif livré « non essayé » (réponse Q-C).
- Abandon d'une session (contexte du core inutilisable en fin de session) : la bibliothèque du core reste chargée
  jusqu'à la fin du processus, ses threads éventuels aussi (appels neutralisés, ADR 0008) ; le core suivant de même
  nom passe par la copie privée (ADR 0022). Rare, journalisé.
- Haute résolution = autre core (swanstation au lieu de pcsx_rearmed) ⇒ **carte mémoire et états séparés** (rangés
  par core, `emu-runtime/src/saves.rs` ; la carte de la famille n'est reprise qu'une fois, au premier lancement
  du nouveau core). Choix de l'utilisateur (réponse Q-A) : on change de core, avec un message.
- Mémoire graphique : un core qui ne libère pas ses objets GL fait monter la mémoire d'une session à l'autre en mode
  bibliothèque (§2, partagée avec la mémoire centrale sur l'Iris Xe) ; constaté au prototype et à l'essai, parade
  `isolated = true` par core.
- Un core lourd peut ne pas tenir la cadence sur le thread de l'interface : mesuré ; un thread d'émulation reste
  hors 9.x (ADR 0010).
- Lignes « amendée par ADR 0026 » à ajouter aux ADR 0002, 0008, 0010 et 0021 à l'acceptation (lot architecte).

## Effet sur chaque système livré

Commun à tous : panneau Stats étendu (temps calculés **seulement** quand le panneau est ouvert ou qu'une mesure
tourne) et « Mesurer 60 s » (sur action de l'utilisateur) ; capacité du lancement `NONE` hors haute résolution
choisie, donc commandes 14 et 56 inchangées.

| Système (`systems.toml`) | Effet de la 9.0 |
|---|---|
| `megadrive` (Mega Drive / Genesis) | rien |
| `snes` (Super Nintendo) | rien |
| `nes` (NES / Famicom) | rien |
| `gb` (Game Boy) | rien |
| `gbc` (Game Boy Color) | rien |
| `gamegear` (Game Gear) | rien |
| `mastersystem` (Master System) | rien |
| `atarist` (Atari ST) | rien |
| `megacd` (Mega CD / Sega CD, dont CD 32X) | rien |
| `amiga` (Amiga) | rien |
| `dos` (PC MS-DOS) | **à vérifier** : dosbox_pure demande `SET_HW_RENDER` pour la Voodoo ; capacité `NONE` ⇒ refus et repli logiciel inchangés (test du plan de lancement, test réel `#[ignore]`, GQ2) |
| `cpc` (Amstrad CPC) | rien |
| `c64` (Commodore 64) | rien |
| `gba` (Game Boy Advance) | rien |
| `32x` (32X) | rien |
| `psx` (PlayStation) | **change, désactivé par défaut** : haute résolution (swanstation en OpenGL) sur réglage du système ou exception du jeu ; pcsx_rearmed reste le défaut ; swanstation choisi sans haute résolution : `NONE`, logiciel comme en 7.0 ; haute résolution ⇒ carte mémoire et états séparés (message) ; grisée sur Mac, sous WSL, Docker et pilote logiciel |
| `3do` (3DO) | rien |
