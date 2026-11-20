# Essais DOSBox (PC-DOS) — étude de faisabilité pour la 4.0

Date : 2026-11-20. Essais faits sous Xvfb avec l'appli `emu-rust` 1.0.0 (`target/debug/emu-rust`, binaire local ; il ne
contient pas encore `keyboard.rs` / `disk.rs` de la branche `wip-2.0`, voir §3), sans modification du code.
Sources du core lues pour confirmer les comportements : https://github.com/schellingb/dosbox-pure (HEAD du 2026-11-07,
`9cccfa21`, dépôt redirigé « moved » depuis 2026-10-06), `README.md` et `dosbox_pure_libretro.cpp`.

## 1. Cores (buildbot libretro)

Les trois cores existent pour Linux, Windows et macOS x86_64 (HEAD 200, tous modifiés le 2026-11-20).

| Core | Linux x86_64 (zip) | `.so` | Windows x86_64 (zip) | macOS x86_64 (zip) |
|---|---|---|---|---|
| **dosbox_pure** | 1 443 942 o | 3 102 096 o | 1 789 506 o | 1 316 781 o |
| dosbox_core | 3 438 564 o | 8 813 304 o | 4 118 266 o | 4 356 584 o |
| dosbox_svn | 1 110 596 o | 3 152 184 o | 1 618 033 o | 1 121 784 o |

URL type : `https://buildbot.libretro.com/nightly/{linux/x86_64,windows/x86_64,apple/osx/x86_64}/latest/<core>_libretro.{so,dll,dylib}.zip`.

SHA256 (nightly du 2026-11-20, Linux ; le fichier est identique au téléchargement à deux reprises) :

| Core | zip | `.so` |
|---|---|---|
| dosbox_pure | `3c8f56a4686fe26dcd49d21df1218101b3457b2aa2a93bee96ad933a9af2a126` | `37e7f282e3d99f3a7536fed1ede483dd3538ed90476c9cbe127f01bab3930955` |
| dosbox_core | `afc9309129a7ea145a8d52cbd910705db82cbb48ac6b9aa709c17c47e7b82be7` | `2a5749c69d9e1e1bfdd7051808cb1c3d40cb3281c2f64384095f32a282b6c932` |
| dosbox_svn | `06b6fb94b7d196a4d623b340a2919c5a2ce94be1d4dea0adc9fd69872ff2e48c` | `9e76c3d60a1cd2a8abb3695fd530ec4deaed06174c194a88dd091ae545852187` |

`retro_get_system_info` (petit programme C avec `dlopen`), API libretro 1 pour les trois :

| Core | version | extensions | need_fullpath | block_extract |
|---|---|---|---|---|
| dosbox_pure | `1.0-preview8` | `zip\|dosz\|exe\|com\|bat\|iso\|chd\|cue\|ins\|img\|ima\|vhd\|jrc\|m3u\|m3u8\|conf\|/` | 1 | **1** |
| dosbox_core | `r4494 d38fcb5` | `exe\|com\|bat\|conf\|cue\|iso\|img` | 1 | 0 |
| dosbox_svn | `0.74-SVN r4393 46a3164` | `exe\|com\|bat\|conf\|cue\|iso\|img` | 1 | 0 |

- **Licence** : GPL-2.0 pour les trois (`.info` : `license = "GPLv2"` ; `LICENSE` du dépôt dosbox-pure = GPL v2 ; l'API GitHub
  donne `gpl-2.0`). Auteurs : DOSBox Team + Psyraven (Pure), + radius / Nikos Chantziaras (core, svn).
- **Recommandation : dosbox_pure.** Seul à gérer ZIP/DOSZ, CHD, VHD, disquettes, `.m3u8`, démarrage sans contenu, menu de
  démarrage intégré, sauvegarde des modifications, états (`savestate = true`), disque via `SET_DISK_CONTROL_EXT_INTERFACE`.
  dosbox_core : `savestate = false`, 130 options (cycles, MT-32/FluidSynth/BASSMIDI, Voodoo…), mais voir les écarts §2.
  dosbox_svn : DOSBox 0.74 « pur », options v1, pas de disk control, pas de savestate.
- **Écarts `.info` / binaire (Pure)** : `.info` annonce `display_version = "0.9.9"` et l'extension `tc` ; le binaire est
  `1.0-preview8` et n'a pas `tc`. Les releases GitHub de Pure ne contiennent que des `1.0-previewN` (dernière : `1.0-preview6`,
  2026-07-14, `dosbox_pure_libretro-linux_x64-1.0-preview6.zip` 1 358 500 o). Le nightly (preview8) est donc plus récent que
  la « dernière release » : TOFU comme les autres cores, ou épingler le SHA256 du nightly testé.
- `.info` (Pure) : `supports_no_game = true`, `disk_control = true`, `needs_fullpath = true`, `hw_render = false`,
  `core_options_version = 2.0`, `needs_kbd_mouse_focus = true`, `systemid = dos`, `database = "DOS"`.
  `.info` (dosbox_core) : `firmware0..9` = MT32_CONTROL.ROM, MT32_PCM.ROM, CM32L_*.ROM, libbass/libbassmidi (`firmware*_opt = true`).

### Options importantes de dosbox_pure (clés `dosbox_pure_*`, 54 options, v2)

| Clé | Défaut | Valeurs / rôle |
|---|---|---|
| `cycles` | `auto` | `auto,max,315,1320,2750,4720,7800,13400,26800,77000,200000,500000` (performance émulée) |
| `cycles_max`, `cycles_scale`, `cycle_limit` | `none`, 1.0, 1.0 | détail du réglage de vitesse |
| `memory_size` | `16` (Mo) | `none,4,8,16,24,32,48,64,96,128,224,256` ; redémarrage requis |
| `machine` | `svga` | `svga,vga,ega,cga,tandy,hercules,pcjr` ; `svga` (S3, ET3000/4000, Paradise, VESA), `svgamem` 2 Mo |
| `cpu_type` / `cpu_core` | `auto` / `auto` | `386,386_slow,386_prefetch,486_slow,pentium_slow` / `dynamic,normal,simple` |
| `sblaster_type` / `sblaster_conf` | `sb16` / `A220 I7 D1 H5` | SB16, Pro, 2, 1, GameBlaster, aucune ; adlib `auto,cms,opl2,dualopl2,opl3,…`, émulation OPL `default`/`nuked` |
| `gus`, `tandysound` | `false`, `auto` | Gravis Ultrasound, Tandy |
| `audiorate` | `48000` | `48000,44100,32000,22050,16000,11025,8000,49716` ; redémarrage requis |
| `midi` | `frontend` | `frontend,disabled` + **liste dynamique** des `.sf2` et jeux de ROM MT-32/SC-55 trouvés dans le dossier système (voir ci-dessous) |
| `volume_*` | 1.0 | sb, midi, adlib, speaker, cdrom, other |
| `voodoo` (+`_perf,_scale,_gamma`) | `8mb` | 3dfx Voodoo (rendu OpenGL côté core ; le core sonde `GET_PREFERRED_HW_RENDER`, réponse `false` sans conséquence ici) |
| `aspect_correction`, `overscan` | `false`, 0 | `false,true,doublescan,padded,padded-doublescan` |
| `force60fps` | `false` | force la cadence de sortie |
| `savestate` | `on` | `on,rewind,disabled` |
| `mouse_input` | `true` | `true,virtual,direct,pad,false` (voir §5) ; `mouse_speed_factor(_x)`, `mouse_wheel` (touche liée à la molette) |
| `keyboard_layout` | `us` | 26 dispositions (`fr`, `be`, `uk`, `de`…) : **à lier à la langue de l'utilisateur** |
| `auto_mapping`, `on_screen_keyboard`, `joystick_*`, `actionwheel_inputs` | | pads et clavier virtuel (L3) |
| `conf` | `false` | `false,inside,outside` : charge `dosbox.conf` du contenu ou `<nom>.conf` à côté |
| `menu_time`, `menu_transparency` | 99, 70 | durée du menu de démarrage (`-1` = jamais de retour au menu) |
| `strict_mode` | `false` | ignore saves/ROMs hors contenu |
| `bootos_ramdisk`, `bootos_dfreespace`, `bootos_forcenormal` | `false`, `1024`, `false` | installation d'un vrai OS (Windows 9x…) : `false,true,diff` |
| `modem` | `null` | `null,dial` (utilise `SET_NETPACKET_INTERFACE`, cmd 78) |

**MIDI, SoundFont, MT-32.** Pure n'embarque ni SoundFont ni ROM. Le dossier système est scanné : fichiers `*.sf2`
(« General MIDI SoundFont »), paires `*_control.rom` + `*_pcm.rom` (MT-32 / CM-32L, ROMs Roland **commerciales : ne rien
télécharger, l'utilisateur les fournit**), jeux `rom1.bin`/`waverom1.bin` (SC-55, idem). Le contenu peut aussi porter
`DOSBOX.SF2`, `MT32_CONTROL.ROM`/`MT32_PCM.ROM` à la racine de C: (surcharge par jeu). Message du core :
« Select the .SF2 SoundFont file, .ROM file or interface used for MIDI output ». Sans cela (ou sans interface MIDI du
frontend), la musique General MIDI est muette : `[DOSBOX LOG] No working midi device found/selected` puis `MIDI: Opened device:none`.
SoundFont libre candidate (non vérifiée ici) : GeneralUser GS (licence permissive, redistribuable) — à vérifier avant tout
ajout à `emu-assets`.
**Important** : ce scan passe par `GET_VFS_INTERFACE` version 3 (`DBP_ScanSystem` retourne vide sinon) : tant que le frontend
ne l'implémente pas, la liste MIDI reste réduite à `frontend`/`disabled`, et les shells `.dosz` / images d'OS du dossier
système ne sont pas listés.

### dosbox_core / dosbox_svn (écarts constatés)

- Les deux **montent le dossier du programme** comme C: (`MOUNT C "<dossier>" -freesize 1024` visible à l'écran) : les écritures
  vont directement dans le dossier du contenu (core : option `save_overlay` pour un overlay). Pas de zip.
- dosbox_core : ~130 options (`cpu_cycles_mode`, `memsize`, `machine`, `sbtype`, `mididevice` = `alsa,bassmidi,fluidsynth,mt32,libretro,none`,
  `mt32.*`, `fluid.*`, `bassmidi.*`, `vkbd_*`, `pinhack*`…). Au chargement, il émet des centaines de
  `RETRO_ENVIRONMENT_GET_VARIABLE failed for core option "<clé sans préfixe>"` (ses clés internes sont sans le préfixe
  `dosbox_core_`) et `Failed to open ALSA sequencer` ; l'image s'affiche malgré tout. Un seul avertissement du frontend :
  « libretro callback received on a thread that does not own the core; ignored » (le core appelle un callback depuis un autre
  thread : `thread_sync`, `mt32.thread`) — à surveiller.
- dosbox_svn : démarre sans erreur, 320x200, 60 fps, 44 100 Hz (vidéo à 60 fps contre 70,09 pour Pure).
- Conclusion : n'utiliser que **dosbox_pure** pour la 4.0 ; les deux autres restent des options de repli (core à installer à la main).

## 2. Callbacks d'environnement (dosbox_pure `1.0-preview8`)

Commandes observées (frontend 1.0.0 + harnais C + code source). Niveau DEBUG `unhandled libretro environment command` :

| cmd | Nom | Usage par Pure | État dans le binaire 1.0.0 |
|---|---|---|---|
| 12 | `SET_KEYBOARD_CALLBACK` | **essentiel** : la saisie clavier DOS passe par `keyboard_event(down, keycode, character, modifiers)` ; l'état « touche relâchée » est vérifié en plus par `input_state(RETRO_DEVICE_KEYBOARD)` | non géré (géré sur `wip-2.0` : `emu-libretro/src/keyboard.rs`, à retester) |
| 13 / 58 | disk control / EXT | échange disquettes, CD, images (Pure essaie EXT puis simple) | non gérés dans ce binaire (`disk.rs` sur `wip-2.0`) |
| 18 | `SET_SUPPORT_NO_GAME` | oui (`retro_load_game(NULL)` renvoie `true`) | géré |
| 28 | `GET_PERF_INTERFACE` | `get_time_usec` (repli interne sinon) | non géré, sans conséquence |
| 45\|EXP (0x1002D) | `GET_VFS_INTERFACE` | scan système (SF2, ROM MIDI, shells, OS) ; lectures/écritures de fichiers via VFS si dispo (repli `fopen`) | non géré → listes dynamiques vides |
| 48\|EXP | `GET_MIDI_INTERFACE` | sortie MIDI « frontend » (option `midi` par défaut) | non géré → pas de MIDI |
| 56 | `GET_PREFERRED_HW_RENDER` | sonde pour le 3dfx OpenGL | non géré, sans conséquence (hw_render = false) |
| 71\|EXP | `GET_THROTTLE_STATE` | informatif (`[DBP THROTTLE] NONE 0.0 -> NONE 70.086594`) | non géré, sans conséquence |
| 78 | `SET_NETPACKET_INTERFACE` | modem / IPX | non géré, sans conséquence |

Correction : dans `docs/essais-hatari.md`, « cmd 0x1002D (45\|EXPERIMENTAL) `GET_HW_RENDER_INTERFACE` » est en fait
**`GET_VFS_INTERFACE`** (`GET_HW_RENDER_INTERFACE` vaut 41).

Gérés sans erreur : 9, 10, 11, 15, 17, 18, 27 (log), 31 (dossier de sauvegarde), 35, 36 (`SET_MEMORY_MAPS`, cheats), 42, 52, 55,
60 (`SET_MESSAGE_EXT`, notifications ; repli `log_cb`), 67, 69. Messages du core visibles en INFO via le log :
`[DOSBOX STATUS] Program: PUREMENU|DOSBOX|<programme> - Cycles: -1 - Frameskip: -1 - Paused: 0`, `[DOSBOX LOG] …`, `[DBP THROTTLE]`.
Aucun `SET_MESSAGE_EXT` notable observé en 600 images.

AV : au chargement `640x400`, `70,0866 fps`, `48 000 Hz` (texte VGA 80x25). Un jeu VGA 13h affiche `320x200` (Floppy Bird,
vérifié) : la géométrie change selon le mode vidéo ; vérifier le **ratio d'aspect** (4:3 attendu pour du 320x200 / 640x400 ;
option `aspect_correction`). Rendu logiciel, aucun besoin OpenGL côté core. Audio : 0 underrun (`--no-audio`, stats de l'appli).
Le core n'expose pas de SRAM (`retro_get_memory_size` = 0) : pas de `.srm`, il écrit lui-même ses fichiers de sauvegarde.

## 3. Essais headless (dosbox_pure `1.0-preview8`)

Commande type :

```text
xvfb-run -a -s "-screen 0 1280x800x24" env RUST_LOG=debug target/debug/emu-rust --core dosbox_pure_libretro.so --rom X \
  --data-dir D --system-dir D/sys --save-dir D/sav --frames 600 --dump-frame f.png --no-audio
```

| Contenu | Résultat (image) |
|---|---|
| `hello.zip` (un seul `HELLO.COM`) | **lance directement** le programme : texte « HELLO FROM EMU-RUST_ » en 640x400, `Program: HELLO` |
| `menu.zip` (deux `.COM`) | **menu de démarrage de DOSBox Pure** (titre « DOSBOX PURE START MENU », liste des exécutables, « Go to Command Line », boutons « Set Auto Start » / « Run », onglets « START MENU » / « CONTROLLER MAPPER » avec L/R) ; il attend une entrée manette/clavier |
| `bat.zip` (`RUN.BAT` : `ver`, `echo … > OUT.TXT`, `dir`) | le .bat s'exécute : `DOSBox version pure-libretro. Reported DOS version 5.00.`, `dir` (RUN.BAT, OUT.TXT, 262 144 000 octets libres) ; bandeau « PRESS ANY KEY TO RETURN TO START MENU » (désactivable : `General > Advanced > Start Menu`, `menu_time`) |
| dossier vide (`--rom <dir>`) | menu « No executable file found » + « Go to Command Line » (le dossier est accepté, extension `/`) |
| fichier inexistant | `ERROR … Unable to open ZIP file`, puis menu « No executable file found » (pas de crash, code 0) |
| **sans contenu** | non essayable avec le CLI (`--rom` obligatoire) ; essayé avec un harnais C : `retro_load_game(NULL)` = `true` (comme annoncé : `supports_no_game`) |
| `.conf` (`[autoexec] imgmount a x86BOOT.img -t floppy` / `boot -l a`) | monte le dossier du `.conf` comme C:, exécute l'autoexec : **FreeDOS 1.4 démarre** (écran d'installation « Do you want to proceed [Y,N]? ») |
| `x86BOOT.img` directement | menu « [ Boot from Disk Image ] », liste des programmes de la disquette (FDAUTO.BAT…), option « EJECT x86BOOT.img » |
| `flpybird.zip` (archive FreeDOS complète) | menu : `BATLINKS\FLPYBIRD.BAT` et `GAMES\FLPYBIRD\FLPYBIRD.COM` |
| `fb1.zip` (seulement `FLPYBIRD.COM`) | Floppy Bird : ciel, oiseau, bâtiments, sol (VGA 320x200) |
| `kraptor.zip` (jeu DJGPP) | `Load error: no DPMI - Get csdpmi*b.zip` : les programmes 32 bits DJGPP exigent un hôte DPMI (**CWSDPMI.EXE**) dans le contenu |

- Dossier de sauvegarde : après `bat.zip`, le core écrit **`<save>/dosbox_pure/bat.pure.zip`** (128 o, contient seulement
  `OUT.TXT`) : message `[DOSBOX LOG] [DOSBOX] Saving filesystem modifications to …/bat.pure.zip`. Le zip d'origine n'est jamais
  modifié. (Le sous-dossier `dosbox_pure/` est celui du frontend, par core.)
- Les 3 cores, l'appli 1.0.0, aucun crash, `exit 0`. Aucun conteneur Docker utilisé.
- Menu de démarrage : une fois lancé, le core le redessine lui-même ; pour la 4.0, prévoir un moyen d'**atteindre la ligne
  de commande** sans manette (clavier complet).

## 4. Logiciels de test libres

| Élément | Licence | Source | SHA256 |
|---|---|---|---|
| `HELLO.COM` (16 octets de code + texte, généré) | aucune (code trivial, domaine public) | octets : `B4 09 BA 10 01 CD 21 31 C0 CD 16 B8 00 4C CD 21` + `HELLO FROM EMU-RUST$` (affiche, attend une touche, quitte) | `ccd70355ebcc1b2438568850ddd6755449905ef7a26ed7e6b9a0ef187491a68d` ; `hello.zip` (non reproductible byte à byte : dates du zip) `69d646c8…` |
| FreeDOS 1.4 « Floppy Edition » | GPL-2.0 (noyau, FreeCOM) ; chaque paquet a sa licence (champ `Copying-Policy` du LSM) | https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files/distributions/1.4/FD14-FloppyEdition.zip (23 624 258 o, 2025-04-05) | zip `45b1fa7c52dd996c3bfa5e352ffcd410781b952a6ad629f15a4c9ec4bbaefc5a` (fichier `verify.txt` officiel du même dossier) |
| ↳ `144m/x86BOOT.img` (disquette 1,44 Mo, démarrable) | idem | dans le zip précédent | `552f7cbb0625960c050a3e682a4d2121cc2ffdfa9dc9d592ccd49edf179333dc` (`x86DSK01.img` : `b9dd5315…3516`) |
| Floppy Bird (clone de Flappy Bird, `FLPYBIRD.COM` 8 704 o) | MIT | paquet FreeDOS : https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files/repositories/1.4/games/flpybird.zip (2 073 159 o) ; source https://github.com/icebreaker/floppybird (Mihail Szabolcs) | zip `a6f5c0c1bf5aa1746c9a335714c58835d4ce1ed90caa8de9072d9fb7f1d08ac9` ; `FLPYBIRD.COM` `e9c7f236f8ac2dbe6ba40fd7f69563d47946837895e68d65a928d5a055eb8690` |
| KRaptor (shoot'em up DJGPP) | MIT (LSM : « MIT license », auteur Kronoman) | …/repositories/1.4/games/kraptor.zip (10 377 798 o) | zip `d14e3a780a488be427dbaa577aef8e878f2d62e8b6ab516902c4e9fc99c929cd` ; nécessite CWSDPMI.EXE (GPL/redistribuable, non récupéré) |
| FreeDOS autres jeux (`fmines` GPL-3, `freedoom`…) | variables | …/repositories/1.4/games/ | non téléchargés |

- FreeDOS 1.4 LiveCD (293 950 337 o, sha256 `2020ff6bb681967fd6eff8f51ad2e5cd5ab4421165948cef4246e4f7fcaf6339`) et LegacyCD
  (274 386 388 o, `715b0175…73d4`) existent pour tester **iso/cue** ; non téléchargés (taille, disque). Les SHA256 de tous les
  fichiers du dossier sont dans le `verify.txt` officiel (même URL de base).
- Tests déterministes proposés (tous ≤ 2 Mo hors FreeDOS) : (1) `hello.zip`, 600 images → texte en 640x400 ; (2) `fb1.zip`
  (Floppy Bird, 300 images) → 320x200 ; (3) FreeDOS via `.conf` + `x86BOOT.img`, ~2 500 images → écran d'installation ;
  (4) `bat.zip` → `.pure.zip` créé dans le dossier de sauvegarde (test de l'écriture). Le comportement dépend du core nightly :
  épingler le SHA256 du `.so` pour les empreintes d'images (ou tolérance si TOFU).
- Aucun programme commercial n'a été téléchargé. Ne **jamais** télécharger MS-DOS, Windows 9x, jeux commerciaux, ni les ROM Roland.

## 5. Formats

| Format | Traitement | Notes |
|---|---|---|
| `.zip`, `.dosz` | **natif** (`block_extract = 1`, le core lit le zip lui-même) ; **le frontend ne doit pas extraire** (le core a `block_extract=1`) | si le frontend propose de décompresser, ne pas le faire pour ce core. Zip avec un seul exécutable : lancement direct ; sinon menu |
| dossier (extension `/`) | natif (accepté, essayé) | C: = ce dossier |
| `.exe` `.com` `.bat` | natif (le dossier parent est monté en C:) | `.conf` : charge la config + autoexec, monte son dossier |
| `.iso` `.cue` `.chd` `.ins` | natifs (CD montés en D: ou E:) | un `.cue` référence des `.bin` : ils doivent rester à côté ; `.chd` natif |
| `.img` `.ima` `.jrc` | natifs : disquettes/disques ; menu « Boot from Disk Image » ; `.jrc` = disquette compressée | essayé : `x86BOOT.img` ; démarrage direct via `.conf` + `imgmount`/`boot` |
| `.vhd` | natif (disque dur ; `[ Run Installed Operating System ]`) | une image d'OS **créée** par le core est écrite dans le **dossier système** (`<nom>.vhd`, `SFT_NEWOSIMAGE`), pas dans les saves |
| `.m3u`, `.m3u8` | natifs : liste d'images pour l'échange de disques | complète l'interface disk control (cmd 58) |
| `.conf` | natif | ou option `conf = inside/outside` |

**Modifications** : si le contenu est un zip/iso/dossier-de-conteneur, les écritures vont dans
`<saves>/<core>/<nom du contenu>.pure.zip` (différentiel, relu au lancement suivant ; écrit 2 s à 60 s après la
dernière modification selon la taille, et à la fermeture) ; un fichier `*.SAVENAME` à la racine de C: partage une sauvegarde
entre plusieurs contenus. Contenu non conteneur (`.exe`/`.com` en direct, dossier) : rien n'est écrit dans les saves.
OS installé : `OS Disk Modifications` = `false`/`true`/`diff` (`<nom>-CDRIVE.sav` dans les saves pour `diff`). Disquettes/CD
montés depuis un zip : sauvegarde `<nom>-<hash>.sav` ; la mappe de manette `C:\PADMAP.DBP` est stockée dans la même sauvegarde.
Conséquence : **le frontend doit fournir un dossier de sauvegarde accessible en écriture et persistant par jeu** (déjà le cas :
`<save>/dosbox_pure/`) et inclure ces `.pure.zip` / `.sav` dans la sauvegarde de l'utilisateur ; ne rien « nettoyer ».

## 6. Recommandations pour l'architecte

1. **Faisabilité : oui.** Le core tourne avec l'appli 1.0.0 en rendu logiciel, audio sans underrun, menu de démarrage
   atteint, programmes DOS lancés, FreeDOS démarre, écriture des modifications constatée. Cible : **dosbox_pure**.
2. **Réutilisation de la 2.0 / 3.0** :
   - **Clavier complet + `SET_KEYBOARD_CALLBACK`** (cmd 12) : indispensable ici (c'est le chemin principal des touches DOS).
     Transmettre aussi `input_state(RETRO_DEVICE_KEYBOARD)` (relâchement, Shift maintenu pendant « Restart » = menu).
     Voir le travail `keyboard.rs` de `wip-2.0`. Penser à la disposition (option `keyboard_layout`, `fr` pour l'utilisateur).
   - **Touche hôte (Ctrl droit, ADR 0015)** : Pure annonce `needs_kbd_mouse_focus = true`. En mode clavier d'ordinateur, les
     raccourcis de l'appli doivent être suspendus (équivalent du « Game Focus » de RetroArch) ; touche hôte pour sortir.
   - **Souris** : `RETRO_DEVICE_MOUSE` (relatif) et `RETRO_DEVICE_POINTER` (absolu, `PRESSED`) lus chaque trame ; réglage par
     `mouse_input` (`true` = auto, `virtual`, `direct` — absolu, exige un pilote type vmwmouse dans l'OS —, `pad`, `false`).
     Souris relative avec capture du pointeur (comme Amiga/ST) ; molette → touche (`mouse_wheel`).
   - **Options par jeu** : clés `dosbox_pure_*` par contenu (cycles, mémoire, machine, SB, layout, `conf`) ; plusieurs sont
     « restart required ». Prévoir un profil par défaut « DOS » (cycles `auto`, `aspect_correction` à décider).
   - **Import de fichiers** : zip/iso/chd/img/vhd/`.m3u8` sans traitement ; **ne pas extraire les zip** pour ce core
     (contrairement à hatari). Le scan bibliothèque doit accepter dossiers et `.conf`, et regrouper les images multi-disques.
   - **Disk control EXT (cmd 58)** déjà requis par la 2.0 : même code pour changer disquette/CD.
3. **À ajouter côté frontend (propre à la 4.0)** :
   - `GET_VFS_INTERFACE` v3 (cmd 45\|EXP) : sans lui, pas de liste de SoundFonts, de ROM MIDI, de shells `.dosz` ni d'images
     d'OS du dossier système. Alternative : écrire/lire ces fichiers par `fopen` (le core lit déjà le système par `fopen`
     s'il ne trouve pas dans le contenu) mais les listes dynamiques resteraient vides.
   - `GET_MIDI_INTERFACE` (cmd 48\|EXP) pour une sortie MIDI « frontend » (optionnel ; sinon SoundFont via le dossier système).
   - MIDI : documenter, dans le gestionnaire de BIOS/firmwares (ADR 0013), les ROM MT-32/CM-32L et SC-55 comme **optionnelles,
     fournies par l'utilisateur, jamais téléchargées** (ADR 0005). Seule piste de téléchargement libre : une SoundFont GM
     redistribuable (licence à vérifier, hors périmètre de cette étude).
   - Ligne de commande DOS accessible sans manette : le menu de démarrage du core est piloté par manette ; l'option
     `menu_time` / auto-start et `.conf` permettent de sauter le menu.
   - Bibliothèque : DOS n'a pas de DAT « no-intro » exploitable simplement (`database = "DOS"` côté libretro) : identification
     par nom de fichier/dossier ; pas de hash par jeu. Les jaquettes viendront par nom.
4. **Tests** : étendre `scripts/fetch-test-assets.sh` avec le core dosbox_pure (TOFU ou SHA256 du nightly), `FD14-FloppyEdition.zip`
   (SHA256 épinglé, extraire `144m/x86BOOT.img`), `flpybird.zip` (SHA256 épinglé), et générer `HELLO.COM`/`hello.zip`/`bat.zip`
   par un petit script de test ; tests `#[ignore]` d'empreinte d'image (voir §4) et de présence de
   `<save>/dosbox_pure/*.pure.zip`. Tests d'entrée (clavier : « N » à l'écran d'installation FreeDOS) après le travail clavier.
5. **Licences** : cores GPL-2.0 (cores externes, comme hatari) ; FreeDOS et ses paquets GPL/MIT : redistribution possible mais
   `THIRD-PARTY-DATA.txt` doit citer ce qui est livré. Ne rien livrer de commercial (Windows, jeux, ROM Roland).
6. **Risques** : (a) versions « preview » de Pure en nightly (peu de recul) ; (b) performances (CPU dynamique `dynamic`, à mesurer
   sur la cible Docker/WSLg ; non mesuré ici, llvmpipe) ; (c) les programmes DJGPP exigent CWSDPMI dans le contenu ;
   (d) l'hôte macOS/Windows : seuls les en-têtes HTTP ont été vérifiés (Windows/macOS HEAD 200), pas d'exécution ;
   (e) `dosbox_core` appelle des callbacks depuis un autre thread (ignorés par le frontend) : ne pas le proposer par défaut.

## Fichiers créés

- `docs/essais-dosbox.md` (ce rapport). Aucun code, aucun test, aucun script modifié.
- Scratchpad nettoyé (sauf un dossier `dbp/` : clone du dépôt dosbox-pure de 14 Mo, d'origine incertaine, laissé en place).
  Aucun conteneur ni image Docker créé.
