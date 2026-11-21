# Guide d'essai — version 4.0 (PC-DOS avec dosbox_pure)

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v4.0.0-rc.1`).

Durée : 45 minutes à 1 heure. But : vérifier les jeux DOS au clavier 60 % AZERTY (Windows), sur macOS Intel et dans
Docker (WSLg) : un jeu en zip, un jeu en dossier, le choix du programme de démarrage, la souris, la musique MIDI
facultative. Notez pour chaque étape « OK » ou ce qui s'est passé (capture d'écran bienvenue).

Rappel : emu-rust ne télécharge **aucun** jeu ni système commercial. Seul le SoundFont libre GeneralUser GS
(32 Mo, licence GeneralUser GS v2.0) se télécharge, et seulement si vous le demandez. Les ROM Roland MT-32 /
CM-32L, si vous en possédez, s'importent par « Importer un BIOS… ».

## À savoir avant de commencer

- **Vos fichiers ne sont jamais modifiés.** Un zip est lu tel quel : les sauvegardes, les réglages faits dans
  `SETUP` et le programme de démarrage choisi vont dans `saves/dosbox_pure/<jeu>.pure.zip`. Un jeu en dossier est
  d'abord copié (« Préparation du jeu… ») dans `cache/dosbox_pure/` ; la copie est refaite seulement si le dossier
  change, et peut être supprimée sans rien perdre (**Réglages → Vider le cache DOS**).
- **Changement pour les CD (Mega CD compris).** Les fichiers `.chd`, `.cue` et `.iso` sont désormais rangés par
  dossier : un `.chd` placé **hors** de `roms/megacd/` ou `roms/dos/` devient « non classé » (il était listé en
  Mega CD en 2.x). Rangez vos `.chd` Mega CD dans `roms/megacd/`. Un `.cue` / `.iso` Mega CD hors dossier reste
  reconnu par son en-tête.
- **Clavier.** La disposition du clavier DOS suit la langue du système (AZERTY ⇒ `fr`) : un `é` gravé tape `é`.
  **Ctrl droit** est la touche hôte et n'est jamais transmise au jeu ; Ctrl gauche, Alt gauche et AltGr le sont.
  Sous **Windows, Alt + F4, Alt + Tab et Alt + Espace restent à Windows** (un jeu DOS qui les utilise ne les recevra
  pas). **Alt + Entrée va à DOS** en mode ordinateur : le plein écran se fait par **Ctrl droit + F**.
- Programmes 32 bits DJGPP qui affichent « no DPMI » : copiez `CWSDPMI.EXE` dans le dossier ou le zip du jeu.

## 0. Préparer

1. Lancer emu-rust : la bibliothèque s'ouvre.
2. **Cores** → installer `dosbox_pure` (recommandé pour « DOS »).
3. **Ouvrir le dossier des ROMs**, créer `roms/dos/` et y copier :
   - un jeu en **zip** (par exemple un shareware : `Doom shareware.zip`) ;
   - un jeu **en dossier** (`roms/dos/Mon jeu/` contenant ses `.exe`) ;
   - facultatif : un jeu sur CD (`.iso` ou `.cue` + `.bin`, ou un zip contenant l'image) ;
   - facultatif : une disquette (`.img` / `.ima`).
   Les `.exe` posés directement dans `roms/dos/` sont ignorés (« rangez chaque jeu dans un dossier ou un zip »).
4. **Scanner**. Attendu : chaque zip et chaque dossier de jeu apparaît une fois ; les fichiers d'un dossier de jeu
   ne sont pas listés à part ; la piste `.bin` d'un CD n'apparaît pas comme un jeu Mega Drive.
5. Facultatif : **Réglages → Mettre à jour la base (DAT)** : les jeux reconnus par `DOS.dat` prennent leur titre.

## 1. Jeu en zip, menu DOS, programme mémorisé

1. Lancer le zip. Attendu : si le zip a un seul programme, il démarre ; sinon le **menu DOS** de dosbox_pure
   s'affiche, avec le rappel en bas de l'écran « Menu DOS : ↑ ↓ puis Entrée… · → : le lancer automatiquement la
   prochaine fois ».
2. Au clavier : ↓ / ↑ pour choisir (flèches par Fn sur le Razer), **→** sur le programme du jeu pour le mémoriser,
   Entrée pour le lancer.
3. Jouer un peu, sauvegarder une partie dans le jeu, revenir à la bibliothèque (Ctrl droit → Retour à la
   bibliothèque), relancer : le jeu démarre directement et la sauvegarde est là.
4. Ctrl droit → **Changer de programme de démarrage** : le menu DOS réapparaît (même avec un programme mémorisé) ;
   choisir un autre programme avec →.
5. Ctrl droit → **Réinitialiser les modifications du jeu…** : la confirmation (« Non » sélectionné par défaut)
   explique ce qui sera effacé ; **Oui** : le jeu redémarre comme au premier lancement (menu DOS). Le zip d'origine
   n'a pas changé (date et taille identiques).

## 2. Jeu en dossier

1. Lancer le jeu en dossier. Attendu : la fenêtre **Préparation du jeu…** avec une barre de progression (rapide pour
   un petit jeu), puis le jeu. **Annuler** (ou Échap) pendant la préparation d'un gros jeu : retour à la
   bibliothèque, rien de lancé.
2. Relancer : plus de préparation (copie réutilisée). Modifier un fichier du dossier puis relancer : la préparation
   est refaite.
3. Dans la bibliothèque, sur ce jeu, touche **P** (« lancer un autre programme ») : la liste des programmes à la
   racine du dossier ; Entrée sur `SETUP.EXE` puis **Pour cette fois** : le jeu démarre sur `SETUP`. Reprendre avec
   **Par défaut** : le programme est lancé à chaque fois (si aucun programme n'a été mémorisé avec → dans le menu
   DOS, qui reste prioritaire). **Revenir au menu DOS** retire ce choix.
4. Vérifier que le dossier d'origine n'a pas été modifié (aucun fichier nouveau, dates inchangées).
5. **Réglages → Jeux DOS → Vider le cache DOS (N Mo)** : le cache est vidé ; relancer le jeu : nouvelle
   préparation, la sauvegarde est toujours là.

## 3. Clavier, souris, mode Joystick

1. Dans un jeu qui demande du texte (nom du joueur, ligne de commande) : les lettres et accents AZERTY tapent ce qui
   est gravé. La touche `<` (à gauche de W) tape `<`.
2. **Ctrl droit + K** : le clavier virtuel PC (102 touches, pavé numérique, F11 / F12, Pause), légendes AZERTY.
   Flèches / Tab pour choisir, Entrée pour taper, Échap pour fermer.
3. Un point-and-click à la souris : cliquer dans la fenêtre capture la souris ; Ctrl droit + M la libère.
4. **Ctrl droit + J** : mode Joystick (ZQSD / flèches = directions, J / K = boutons) puis retour en mode Clavier.
5. Facultatif, manette PS4 : un jeu au joystick analogique DOS ; les sticks sont transmis tels quels (pas de
   conversion en croix pour ce core). Le choix du type de joystick DOS se fait dans le menu du core (L3).

## 4. Réglages DOS et musique MIDI (facultatif)

1. Ctrl droit → **Options du core** : la section **Réglages DOS** est en tête (vitesse, mémoire, CPU, carte
   graphique, carte son, MIDI, disposition du clavier, vitesse de la souris). « Portée : ce jeu » pour un réglage
   propre au jeu ; certains réglages demandent **Redémarrer le jeu**.
2. Ligne **MIDI** : sans SoundFont, la ligne d'aide rappelle où l'installer.
3. Bibliothèque → **Cores** → section « BIOS et SoundFont libres » → **GeneralUser GS** → **Installer** (32 Mo,
   progression, Annuler possible). Puis **Utiliser pour tous les jeux DOS**.
4. Relancer un jeu qui a une musique General MIDI (choisir « General MIDI » / « MPU-401 » dans son `SETUP`) :
   la musique MIDI est audible. Dans « Réglages DOS ▸ MIDI », la valeur `GeneralUser-GS.sf2` est proposée.
5. MT-32 (si vous possédez les ROM) : **Cores → Importer un BIOS…** → `MT32_CONTROL.ROM` puis `MT32_PCM.ROM` ;
   elles apparaissent ensuite dans « Réglages DOS ▸ MIDI ».

## 5. Ligne de commande (facultatif)

- `emu-rust --core dosbox_pure --rom "roms/dos/Mon jeu"` : préparation affichée en %, puis le jeu.
- `emu-rust --core dosbox_pure --rom "roms/dos/Mon jeu/SETUP.EXE"` : même jeu, démarré sur `SETUP`.
- `emu-rust --core dosbox_pure --rom jeu.conf` : refusé avec un message.
- `emu-rust --list-bios` (GeneralUser GS listé comme « SoundFont »), `emu-rust --install-bios generaluser-gs`,
  `emu-rust --clear-dos-cache`.

## 6. Docker (WSLg) et macOS

1. Docker : refaire les §1 et §2 avec un zip et un dossier dans le dossier des ROMs monté ; la préparation écrit
   dans le volume de données (`cache/`).
2. macOS Intel : refaire le §1 ; noter la fluidité (si le jeu saccade : « Réglages DOS ▸ Vitesse » plus bas, ou
   l'option du core « Force 60 FPS » pour ce jeu).
