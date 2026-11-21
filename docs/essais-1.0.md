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

Suites : essais Windows prévus par l'utilisateur (C64 Maniac Mansion, Mega CD Dune, Amiga Hunter, DOS Storm Master, manette) ; essai Docker WSLg à faire. PS1 non disponible (7.0 non commencée).
