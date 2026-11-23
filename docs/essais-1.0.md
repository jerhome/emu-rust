# Retours d'essai de la 1.0.0-rc.1

| # | Date | Plateforme | Jeu / core | Constat | Statut |
|---|---|---|---|---|---|
| 1 | 2026-11-20 | Windows (Intel Iris Xe, OpenGL 3.3) | Final Fight (USA) (Virtual Console), zip, snes9x | ✅ Bibliothèque → lancement OK ; jouable au clavier ; son impeccable (32040 Hz → 48000 Hz) | OK |
| 2 | 2026-11-20 | Windows | idem | Retour à l'écran d'accueil de Windows en jouant (reproductible) ; à 16:09:25 `audio stream failed`. Mini clavier Razer **60 %** (sans flèches, rangée F ni pavé ; accès par Fn) branché sur le PC d'entreprise : directions jouées par des combinaisons de touches → probablement un raccourci Windows (verrouillage, Win+…). Pas un plantage de l'émulateur. | **Confirmé par l'utilisateur** : combinaison de touches (raccourci Windows), aucun plantage sans fausse manœuvre | clos |
| 3 | 2026-11-20 | Windows | — | Après une erreur du flux audio, le son reste coupé jusqu'au redémarrage de l'appli : pas de reprise automatique quand le périphérique revient. | corrigé (rc.2) |
| 4 | 2026-11-20 | Windows | — | Profil clavier par défaut et raccourcis (F2…F11) inutilisables sur un clavier 60 % : disposition par défaut « directions main gauche » (ZQSD/WASD + JKL/UIO) et raccourcis sans touches F. | corrigé (rc.2) |
| 5 | 2026-11-20 | macOS Intel 12.7.6 | California Games, Mega Drive (genesis_plus_gx) | ✅ Lancement, image, son et clavier OK, comme sous Windows (1.0.0-rc.1) | OK |
| 6 | 2026-11-20 | Windows (clavier 60 %) | Dune, Mega Drive (genesis_plus_gx) | ✅ 1.0.0-rc.2 : tout fonctionne (lancement, image, son, clavier) | OK |
| 7 | 2026-11-20 | Windows | — | ✅ 1.0.0-rc.2 : raccourcis Ctrl OK ; son stable | OK |
| 8 | 2026-11-20 | Windows | Skate or Die (NES, mesen) · Alex Kidd (Master System) · Super Mario Land (Game Boy, gambatte) · Castle of Illusion (fichier `.sms`, Master System) | ✅ tous jouables (1.0.0-rc.2) | OK |
| 9 | 2026-11-20 | Windows | Castle of Illusion (Game Gear) | Affiché comme jeu Master System : le fichier est un `.sms` → identification correcte (pas un bug) | clos |
| 10 | 2026-11-20 | Docker | — | Essai Docker abandonné par l'utilisateur ; non bloquant pour la 1.0.0 (image toujours publiée) | sans objet |
| 11 | 2026-11-20 | Windows | Sonic 2, Game Gear (genesis_plus_gx) | ✅ 1.0.0 : fonctionne bien (vrai `.gg`) | OK |

## Essais 2.0.0-rc.1

| # | Date | Plateforme | Jeu / core | Constat | Statut |
|---|---|---|---|---|---|
| 1 | 2026-11-21 | Windows (clavier 60 %) | — / hatari | ❌ « core hatari : CRC32 x au lieu de y » : catalogue en cache plus vieux que le nightly reconstruit ; contournement « Vérifier les mises à jour » | corrigé (bf6801c) |
| 2 | 2026-11-21 | Windows | Mouse Trap (ST), hatari + EmuTOS 1.4 fr | ✅ Installation EmuTOS, bureau GEM, `MOUSE.TOS` lancé depuis la fenêtre A, mode joystick (Hôte+J), démarrage par **Espace** : le jeu tourne bien | OK |
| 3 | 2026-11-21 | Windows | idem | Ergonomie : l'utilisateur a pris `MOUSE.TOS` pour un pilote et ne savait pas lancer un programme depuis le bureau → ajouter au guide d'essai et à l'aide un mot sur « lancer un .PRG/.TOS depuis le bureau GEM » | à améliorer |

## Essais 6.0.0-rc.1 (2026-11-21)

| Hôte | Jeu | Résultat |
|---|---|---|
| Linux sous WSL2 (binaire Linux, WSLg) | Dune II (Mega Drive), clavier | **OK**, impeccable |
| idem | son | **OK** après `libasound2-plugins` + `~/.asoundrc` (configuration de l'hôte, documentée) |
| idem | manette | non vue : WSL2 ne voit pas l'USB sans `usbipd-win` (attendu) |
| Windows | Dune (Mega CD), BIOS européen | ✅ **parfait** : image, son, **manette PS4** OK (première validation réelle du Mega CD et de la manette) |
| Windows | Storm Master (PC-DOS, jeu-dossier, START.EXE) | ✅ **succès** : son et capture de la souris OK ; lancer → retour bibliothèque → relancer OK (copie privée du core, ADR 0022, validée en réel) |
| Windows | Hunter (Amiga, `.ipf`) + `capsimg.dll` dans `system/` | ❌ reste sur l'écran « disquette Workbench » (IPF non lu) — mis de côté, voir questions-ouvertes |
| Windows | Hunter (Amiga, `.ipf`), 7.0.1, `CAPSImg_5.1fs3_Windows_x86-64` (`capsimg.dll` seule + `version.txt`) dans `system/` | ❌ journal : `DLOPEN: Failed to open …\system\capsimg.dll` / `Unable to open capsimg.dll` → fichier trouvé, LoadLibrary refuse. Vérifié : PE 64 bits (8664), `Unblock-File` sans effet, `msvcp140.dll` à côté d'`emu-rust.exe` sans effet ; pas de droits admin (VC++ non installable). **Remis de côté (2026-11-23).** Piste : sonde emu-rust qui charge la DLL elle-même et affiche l'erreur Windows exacte + imports PE manquants (import guidé capsimg) |
| Windows | Another World (Amiga, ADF), Kickstart 1.3 | ✅ **OK** : mode joystick et son |
| Windows | Maniac Mansion (C64), vice_x64 | ❌→✅ installation refusée (CRC : index buildbot en retard, voir questions-ouvertes) → DLL posée à la main ; chargement long ; pointeur immobile sur le port 2 par défaut → **OK au port 1** (menu « Port joystick »), son OK, changement de face par le raccourci OK. Piste : le jeu/la DAT pourrait mémoriser le port connu par jeu, ou le guide le signaler |
| Windows | B.A.T. (CPC), cap32 | ❌ zip de `.dsk` dans `roms/cpc/` : Ready, `cat` → « Drive A: disc missing » (aucune disquette insérée) ; ✅ les 4 `.dsk` (« Disk N of 4 ») extraits dans `roms/cpc/` : regroupés, fonctionne (« trop beau », jouable à la manette PS4), changement de disquette OK. **Défaut en diagnostic** (zip + cap32 `media = direct`) |
| Windows | NBA Jam (32X), picodrive | ✅ **impeccable** : image, son, manette PS4 (fichier extrait) ; ❌ zip simple (un seul fichier) non utilisable, l'utilisateur a dû extraire — symptôme à préciser, défaut à diagnostiquer |
| Windows | Metroid Fusion (GBA), mgba | ✅ **impeccable** : image, son, manette PS4 |
| Windows | Zelda Link's Awakening DX (GBC), gambatte | ✅ **impeccable** : image, son, sauvegarde (reprise après relance) |
| Windows | Final Fight (SNES), snes9x — non-régression | ✅ **impeccable** : son et manette PS4 |
| Windows | Skate or Die (NES), mesen — non-régression, fichier à la racine de `roms/` | ✅ **impeccable** : son et manette PS4 |
| Windows | Alex Kidd (Master System), Sonic 2 (Game Gear), Super Mario Land (Game Boy), Mouse Trap (Atari ST) — non-régression | ✅ **tous OK** (« tous les tests ok sur tous les jeux sur la 6 ») |

Bilan 6.0.0-rc.1 (2026-11-21) : **toutes les plates-formes validées** sous Windows (jeux de la liste de l'utilisateur). Défauts trouvés : zip CPC (corrigé pour la 7.0), zip 32X (symptôme à préciser), IPF/capsimg (mis de côté), index buildbot VICE (externe). Restent : Docker WSLg ; PS1 et 3DO avec la 7.0.

## Essais 7.0.0-rc.1 (Windows, 2026-11-22)

| Plate-forme | Jeu, core | Résultat |
|---|---|---|
| Windows | GTA (PlayStation), pcsx_rearmed, **BIOS HLE intégré** (sans `scph*.bin`) | ✅ **impeccable** : image, son, manette PS4 |
| Windows | GTA (PlayStation), pcsx_rearmed, **BIOS Europe de l'utilisateur** | ✅ jeu OK après lancement forcé ; ❌ **empreinte du BIOS non reconnue** à l'import (seul md5 accepté pour `scph5502.bin` : 32736f17… = SCPH-5502 v3.0E) — md5, taille et nom du fichier demandés à l'utilisateur |
| Windows | GTA (PlayStation), pcsx_rearmed, **BIOS `scph5502.bin` reconnu** (bon fichier récupéré par l'utilisateur ; le premier n'était pas le SCPH-5502 v3.0E) | ✅ **OK** : import reconnu, jeu fonctionnel. Point ouvert : accepter les autres révisions européennes valides (SCPH-1002/7002/7502/9002…) au lieu de « empreinte inconnue » |
| Windows | GTA, pcsx_rearmed — mode manette DualShock | ✅ **OK** : bascule Numérique → DualShock, mode retenu après réinitialisation |
| Windows | GTA, **swanstation** (changement de core) | ✅ **OK** : le jeu tourne ; reprise de carte mémoire non vérifiée (pas de sauvegarde possible avant une fin de mission) |
| Windows | GTA, **mednafen_psx (Beetle PSX)** | ✅ **OK** : « ça marche quand même sacrément bien » (pas de lenteur signalée) |
| Windows | Road Rash (3DO), opera, **sans `panafz10.bin` (Opera Boot intégré)** | ✅ **impeccable** — question ouverte de la 7.0 tranchée : le jeu démarre avec le BIOS libre intégré |
| Windows | Road Rash (3DO), opera, **avec `panafz10.bin`** | ✅ **OK** : import reconnu, jeu fonctionnel. Remarque UX : la 3DO n'apparaît pas dans « Core utilisé par système » (liste limitée aux systèmes à plusieurs cores, cores_ui/mod.rs:770) ; on ne retrouve le core qu'en cherchant « opera » → 7.1 : afficher aussi les systèmes à core unique (sans choix) et chercher par nom de système |
| Windows | B.A.T. (CPC) en `.zip`, cap32 | ✅ **zip corrigé** : démarre, jouable (plus de « disc missing ») ; ❓ **pas de musique** — l'utilisateur n'entend que des bruitages (« sons de l'émulateur ») alors qu'il pense qu'il y a de la musique. À établir : la version CPC a-t-elle de la musique en jeu, modèle de CPC (cap32_model), bruit de lecteur (cap32 floppy sound). Lot son |
| Windows | Gryzor (CPC), cap32 | ❌ **aucun son** (B.A.T. : seulement le bruit du lecteur). Défaut audio CPC en diagnostic (qa-systems, 2026-11-22) |
| Windows | Gryzor (CPC), cap32 — **6.0 et 7.0** | ✅ bruitages (tirs) présents dans les deux versions ; ❓ pas de musique, **identique en 6.0 et 7.0 → pas une régression, son CPC fonctionnel**. Musique absente : propre au jeu (beaucoup de jeux CPC n'ont de musique qu'au titre) ou au core ; à revoir au « lot son » avec un jeu à musique certaine. Ne bloque pas la 7.0 |
| Windows | Maniac Mansion (C64, 2 `.d64`), vice_x64 — 7.0.0 | ⚠️ hors de `roms/c64/` : « FILE NOT FOUND ERROR » (démarrage probable sur la face 2) ; ✅ rangé dans `roms/c64/` + scan : fonctionne. À examiner : ordre/regroupement des faces `.d64` hors dossier système (non bloquant ; le rangement 7.1 les placera dans `c64/`) |
| Windows | Maniac Mansion (C64), 2 `.d64` dans `roms/c64/` | ⚠️ **deux tuiles** (une par face, celle de la face 2 ne démarre pas) : faces non regroupées en un jeu → noms des fichiers demandés ; contournement : le zip |
| Windows | Skweek (CPC), cap32 — 7.0.0 | ✅ **musique OK** : le son CPC (puce AY) est complet ; l'absence de musique en jeu dans Gryzor / B.A.T. vient des jeux eux-mêmes. Point clos |
| Windows | **v7.0.0 stable** (zip de la release) à la place de la rc | ✅ démarre, bibliothèque, sauvegardes et réglages retrouvés |
| Windows | Night Trap (Europe) (Disc 1/2) (Mega-CD 32X) `.chd`, **picodrive** (choisi pour Mega CD), 7.0.0 — essai exploratoire 7.2 | ❌ « Le core a refusé le jeu : invalid CD image ». **Important pour la 7.2** : picodrive refuse ce `.chd` (CHD non lu par ce build, CHD de type DVD au lieu de CD, ou autre). À vérifier : même `.chd` avec genesis_plus_gx (s'il démarre, le CHD est valide), `chdman info` (type CD/DVD), essai en `.cue/.bin` |
| Windows | Night Trap 32X (2 `.chd` regroupés), **genesis_plus_gx**, 7.0.0 | ❌ « Le core a refusé le jeu : unable to open file, could not load <chd> from m3u file list ». **Défaut probable chez nous** : le `.m3u` généré pour regrouper les disques pointe mal vers les `.chd` (chemin relatif/absolu, dossier du m3u, caractères) — expliquerait aussi « invalid CD image » de picodrive. Jeux Mega CD/PS1 multi-disques en `.chd` concernés. À diagnostiquer en priorité (correctif 7.0.1 ?) |
| Windows | Night Trap 32X (2 `.chd` regroupés), **genesis_plus_gx**, **7.0.1** | ✅ correctif validé : le disque est lu, le BIOS Mega CD démarre, écrans Sega affichés ; puis le jeu lui-même affiche « This game requires the Sega 32X Attachment » (attendu : version CD 32X, genesis_plus_gx n'émule pas le 32X → picodrive, 7.2) |
| Windows | Interface | 🙁 **demande utilisateur** : pouvoir **copier/coller les messages d'erreur** depuis l'application (« trop chiant de recopier ») → messages sélectionnables + bouton « Copier » (dialogues d'erreur, bandeaux, panneau Journal) |
