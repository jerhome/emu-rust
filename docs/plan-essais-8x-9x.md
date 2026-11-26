# Plan d'essais 8.x et 9.x (validé par l'utilisateur le 2026-11-22)

> **Numérotation (décision de l'utilisateur du 2026-11-26)** : 8.0 cartouches (part après la 7.6) ; 8.1 archives
> `.7z` ; 8.2 interface en anglais ; 8.3 core jgenesis ; CD **8.4**, ordinateurs **8.5**, arcade **8.6** (ADR 0025,
> amendement du 2026-11-26 ; remplace la numérotation de la fusion du 2026-11-25 : CD 8.2, ordinateurs 8.3, arcade
> 8.4). PC Engine (HuCard) : système de référence de la conception 8.0. **Atari Lynx (California Games) : premier
> système essayé en 8.0** (demande de l'utilisateur du 2026-11-25). La **9.0 vient après toute la 8.x** (après la
> 8.6 ; décision du 2026-11-26) : les essais 9.x suivent ceux de l'arcade.

Les jeux sont fournis par l'utilisateur ; emu-rust ne télécharge aucune ROM ni aucun BIOS commercial.

## 8.x — systèmes personnalisés (cores à rendu logiciel)

| Palier | Plate-forme | Jeu | Ce qu'il teste |
|---|---|---|---|
| 8.0 cartouches | Atari Lynx — **premier système essayé en 8.0** (demande de l'utilisateur du 2026-11-25) | California Games | portable, écran tournable ; cores `handy` et `mednafen_lynx` (les deux à essayer, `handy` d'abord) ; BIOS Lynx `lynxboot.img` **fourni par l'utilisateur** (jamais téléchargé) : essai sans le fichier (bandeau, import proposé), puis avec |
| 8.0 cartouches | PC Engine | R-Type | défilement, sprites, son |
| 8.0 cartouches | MSX | Metal Gear | cartouche Konami MSX1 |
| 8.0 cartouches | Atari 2600 | Pitfall! | console fondatrice, core `stella` |
| 8.0 cartouches | Neo Geo Pocket Color | Metal Slug: 1st Mission | portable SNK ; comparaison avec Metal Slug arcade (8.6) |
| 8.0 cartouches | Nintendo DS | Zelda: Phantom Hourglass (tout au stylet) | **pas urgente du tout** (utilisateur) ; deux écrans + écran tactile (souris) = nouveauté d'interface, core `melonds` en rendu logiciel ; palier à part possible, en fin de 8.x |
| 8.4 CD | PC Engine CD | Castlevania: Rondo of Blood | musique CD, BIOS System Card |
| 8.4 CD | Amiga CD32 | Speedball 2 | prolonge l'Amiga (puae), manette CD32, Bitmap Brothers |
| 8.5 ordinateurs | Sharp X68000 | Final Fight | conversion arcade, disquettes, son FM / MIDI |
| 8.5 ordinateurs | Macintosh 68k (**Mac II**, core `minivmac`) — étude faite (`docs/etude-mac68k.md`), faisable avec limites | SimCity Classic | ROM **Mac II** (`MacII.ROM`) + disquette System 6/7 bootable fournies par l'utilisateur ; pas de souris libretro (pointeur au stick), pas de changement de disque libretro ni d'états ; 7 questions ouvertes |
| 8.5 ordinateurs | Thomson (TO7/70 ou MO5 selon la version du jeu) | Sapiens (Loriciels) | « Informatique pour tous », core `theodore` (à vérifier) ; machine exacte à vérifier |
| 8.5 ordinateurs | Oric Atmos | L'Aigle d'Or (Loriciels) | cassette `.tap`, core `oricutron` (état à vérifier) |
| 8.5 ordinateurs | ZX Spectrum | Saboteur | cassette / chargement ; comparaison avec la version CPC |
| 8.6 arcade | FinalBurn Neo (Neo Geo) | Metal Slug | BIOS Neo Geo, zip arcade |
| 8.6 arcade | FinalBurn Neo (CPS2) | Street Fighter Alpha 2 | combat, 6 boutons |
| 8.6 arcade | MAME 2003-Plus (core léger, recommandé) | OutRun | romset MAME 0.78 ; complète FinalBurn Neo |
| 8.6 arcade | MAME actuel (core complet, lourd) | OutRun (même jeu, romset à jour) | romset exactement à la version du core ; à comparer avec 2003-Plus |

## 9.x — rendu matériel (OpenGL d'abord ; DirectX 11 sans urgence ; Vulkan encore moins)

Après la 8.6 (décision de l'utilisateur du 2026-11-26).

| Plate-forme | Jeu |
|---|---|
| PlayStation haute résolution | GTA |
| Dreamcast | Crazy Taxi |
| Nintendo 64 | GoldenEye 007 |
| Saturn | Sega Rally (core logiciel : possible avant la 9.x) |
| PSP | Gran Turismo |

## Non-régression et « lot son »

| Plate-forme | Jeu | Note |
|---|---|---|
| Atari ST | Le Manoir de Mortevielle | remplace Mouse Trap comme jeu d'essai ST ; synthèse vocale = test du son |

## Ajouts de l'utilisateur du 2026-11-23 (plates-formes « non gérées » à essayer via les systèmes personnalisés)

| Palier | Plate-forme | Jeu | Note |
|---|---|---|---|
| 8.4 CD | Neo Geo CD | Metal Slug | core `neocd` ; BIOS Neo Geo CD fourni par l'utilisateur |
| 8.0 cartouches | ColecoVision | Zaxxon | core `gearcoleco` ou `bluemsx` (à choisir) ; BIOS ColecoVision fourni par l'utilisateur |
| 8.5 ordinateurs | Apple II | Prince of Persia | **core libretro à vérifier** (existence / maintenance) ; disquettes `.dsk`/`.woz` |
| 8.5 ordinateurs | Atari 800 / XL | Star Raiders | core `atari800` ; ROM système (OS/BASIC) à vérifier |
| à placer | ScummVM | à choisir (ex. Beneath a Steel Sky, gratuit et légal ; Monkey Island / Day of the Tentacle si possédés) | moteur, pas une plate-forme : détection des jeux par dossier, pas par ROM — à concevoir à part |
