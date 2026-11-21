# Guide d'essai — version 5.0 (Amstrad CPC avec cap32, Commodore 64 avec VICE)

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v5.0.0-rc.1` pour le CPC, `v5.0.0-rc.2` pour le
C64).

Durée : 45 minutes à 1 heure par machine. But : vérifier les jeux CPC puis C64 au clavier 60 % AZERTY (Windows), sur
macOS Intel et dans Docker (WSLg) : disquette, cassette (chargement accéléré), BASIC au clavier, joystick au clavier.
Notez pour chaque étape « OK » ou ce qui s'est passé (capture d'écran bienvenue).

Rappel : emu-rust ne télécharge **aucun** jeu. Les ROM système sont **intégrées aux cores** (cap32 : ROM Amstrad,
redistribution autorisée par Amstrad ; VICE : ROM Commodore, acceptées pour un usage personnel) ; l'écran Cores
affiche « ROM intégrées au core » et la notice de licence. JiffyDOS (facultatif) s'importe par « Importer un BIOS… ».

## À savoir avant de commencer

- **Mode Joystick par défaut** pour le CPC et le C64 : ZQSD (ou les flèches) + J / K pilotent le joystick, sans
  bascule préalable. **Ctrl droit + J** bascule en mode Clavier (toutes les touches vont à la machine) ; le choix
  est retenu pour ce jeu. Les entrées « Amstrad CPC (BASIC) » et « Commodore 64 (BASIC) » démarrent en Clavier.
  Réglages → Ordinateurs → « Mode par défaut » : Automatique (selon le système) / Clavier / Joystick.
- **Ctrl droit** est la touche hôte (menu, raccourcis) et n'est jamais transmise à la machine.
- Sous **Windows, Alt + F4, Alt + Tab et Alt + Espace restent à Windows**. **Alt + Entrée va à la machine émulée** :
  le plein écran se fait par **Ctrl droit + F**.
- **Chargement accéléré** : pendant un chargement (cassette surtout), le core demande l'avance rapide ; l'OSD
  affiche « Chargement accéléré », le son est coupé, puis tout revient à la normale. Désactivable dans Réglages →
  Ordinateurs (« Chargement accéléré automatique ») ou par `--no-auto-fast-load`.
- **Vos fichiers ne sont jamais modifiés** : CPC — les écritures disquette vont dans
  `saves/cap32/<jeu>/<image>.sav` ; C64 — VICE travaille sur des copies (`saves/vice_x64/<jeu>.disks/`).

## Partie 1 — Amstrad CPC

### Touches (Razer 60 % AZERTY, CPC français si le système est en français)

| CPC | Touche |
|---|---|
| f1 … f9, f0 (pavé) | F1 … F9, F10 (Fn + chiffres) |
| COPY | Alt gauche |
| Cassette : lecture / arrêt / rembobiner | Début / Fin / Page ↑, ou menu « Cassette » |
| Joystick | ZQSD / flèches + J (feu 1), K (feu 2) |

1. **Cores** → installer `cap32`. Attendu : ligne « ROM Amstrad intégrées au core » et la notice Amstrad.
2. Copier dans `roms/cpc/` un jeu disquette (`.dsk`), un jeu cassette (`.cdt`), éventuellement un jeu à deux faces
   (`(Side A)` / `(Side B)`). **Scanner**.
3. **Amstrad CPC (BASIC)** : l'écran « Amstrad 128K Microcomputer … BASIC 1.1 » apparaît, en mode Clavier. Taper
   `print "éàç"` en AZERTY : les caractères du CPC français s'affichent. Ctrl droit + K : clavier virtuel CPC
   (légendes françaises).
4. **Jeu disquette** : il démarre seul (autorun). Jouer au joystick clavier **sans bascule**. Si le jeu ne démarre
   pas : menu (Ctrl droit) → Options du core → « Commande de démarrage… », saisir par exemple `RUN"DISC`,
   Entrée, puis « Redémarrer le jeu ».
5. **Jeu cassette** : le core tape `RUN"` ; OSD « Chargement accéléré », son coupé, puis vitesse normale. Menu →
   **Cassette : Arrêt** puis **Lecture** : la bande s'arrête puis reprend.
6. **Deux faces** : Ctrl droit + D (ou menu → Disquettes) passe à la face B.
7. Sauvegarder une partie dans un jeu qui écrit sur la disquette, quitter, relancer : la progression est là, le
   fichier `.dsk` d'origine est inchangé.
8. f1 … f0 par F1 … F10, COPY par Alt gauche (dans le BASIC : curseur de copie).

## Partie 2 — Commodore 64

### Touches

| C64 | Touche |
|---|---|
| RUN/STOP | Échap |
| RESTORE | AltGr |
| £ | `<` (touche à gauche de W) |
| C= | Ctrl gauche |
| ← | `²` si présent, sinon clavier virtuel |
| Datasette lecture / arrêt / rembobiner | menu « Cassette » |

1. **Cores** → installer `vice_x64` (`vice_x64sc` : plus exact, environ 5 fois plus lent). Attendu : « ROM Commodore
   intégrées au core VICE » et la notice.
2. Copier dans `roms/c64/` un jeu disquette (`.d64`), un jeu cassette (`.tap` / `.t64`), un `.prg`, un `.crt`.
   **Scanner**.
3. **Commodore 64 (BASIC)** : « COMMODORE 64 BASIC V2 », mode Clavier. Taper `<` : `£` s'affiche. RUN/STOP + RESTORE
   (Échap + AltGr) revient au prompt.
4. **Jeu disquette** : chargement automatique (`LOAD"*",8,1`), chargement accéléré, puis jeu au joystick.
   Programme précis : Options du core → « Programme à lancer… » (par exemple `INTRO`).
5. **Port joystick** : si le jeu ne répond pas au joystick, menu → « Port joystick : 2 » → passe au port 1 (retenu
   pour ce jeu). Un fichier nommé `(j1)` / `(j2)` affiche « fixé par le nom du fichier ».
6. **Jeu cassette** : chargement accéléré ; menu → Cassette : Arrêt / Lecture.
7. Essayer un jeu au port 1 et un jeu au port 2.

## Ligne de commande

- `emu-rust --core cap32 --rom "Jeu.dsk" --launch 'RUN"DISC'`
- `emu-rust --core vice_x64 --rom "Jeu.d64" --launch INTRO --no-auto-fast-load`
- `emu-rust --check-bios cap32` : « ROM intégrée au core ».
