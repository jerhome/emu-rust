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
| Windows | Another World (Amiga, ADF), Kickstart 1.3 | ✅ **OK** : mode joystick et son |
| Windows | Maniac Mansion (C64), vice_x64 | ❌→✅ installation refusée (CRC : index buildbot en retard, voir questions-ouvertes) → DLL posée à la main ; chargement long ; pointeur immobile sur le port 2 par défaut → **OK au port 1** (menu « Port joystick »), son OK, changement de face par le raccourci OK. Piste : le jeu/la DAT pourrait mémoriser le port connu par jeu, ou le guide le signaler |
| Windows | B.A.T. (CPC), cap32 | ❌ zip de `.dsk` dans `roms/cpc/` : Ready, `cat` → « Drive A: disc missing » (aucune disquette insérée) ; ✅ les 4 `.dsk` (« Disk N of 4 ») extraits dans `roms/cpc/` : regroupés, fonctionne (« trop beau », jouable à la manette PS4). **Défaut en diagnostic** (zip + cap32 `media = direct`) |

Suites : restent CPC (zip), 32X, GBA, GBC, non-régression des consoles 1.0 et ST ; Docker WSLg ; PS1 et 3DO avec la 7.0 (en cours).
