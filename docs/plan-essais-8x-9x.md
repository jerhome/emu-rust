# Plan d'essais 8.x et 9.x (validé par l'utilisateur le 2026-11-22)

Les jeux sont fournis par l'utilisateur ; emu-rust ne télécharge aucune ROM ni aucun BIOS commercial.

## 8.x — systèmes personnalisés (cores à rendu logiciel)

| Palier | Plate-forme | Jeu | Ce qu'il teste |
|---|---|---|---|
| 8.0 cartouches | PC Engine | R-Type | défilement, sprites, son |
| 8.0 cartouches | MSX | Metal Gear | cartouche Konami MSX1 |
| 8.0 cartouches | Atari Lynx | California Games | portable, écran tournable, BIOS Lynx (`lynxboot.img`, fourni par l'utilisateur) |
| 8.1 CD | PC Engine CD | Castlevania: Rondo of Blood | musique CD, BIOS System Card |
| 8.1 CD | Amiga CD32 | Speedball 2 | prolonge l'Amiga (puae), manette CD32, Bitmap Brothers |
| 8.2 ordinateurs | Sharp X68000 | Final Fight | conversion arcade, disquettes, son FM / MIDI |
| 8.2 ordinateurs | Macintosh 68k (Mac Plus) — **à étudier** | SimCity Classic | souhait de l'utilisateur : gérer le vieux Mac ; core `minivmac` (état à vérifier), ROM Mac Plus + disquette System 6/7 fournies par l'utilisateur |
| 8.2 ordinateurs | Oric Atmos | L'Aigle d'Or (Loriciels) | cassette `.tap`, core `oricutron` (état à vérifier) |
| 8.2 ordinateurs | ZX Spectrum | Saboteur | cassette / chargement ; comparaison avec la version CPC |
| 8.3 arcade | FinalBurn Neo (Neo Geo) | Metal Slug | BIOS Neo Geo, zip arcade |
| 8.3 arcade | FinalBurn Neo (CPS2) | Street Fighter Alpha 2 | combat, 6 boutons |

## 9.x — rendu matériel (OpenGL d'abord ; DirectX 11 sans urgence ; Vulkan encore moins)

| Plate-forme | Jeu |
|---|---|
| PlayStation haute résolution | GTA |
| Dreamcast | Crazy Taxi |
| Nintendo 64 | GoldenEye 007 |
| Saturn | Sega Rally (core logiciel : possible avant la 9.x) |
| PSP | Gran Turismo |
