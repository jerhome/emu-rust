# Guide d'essai — version 7.1 (finition de l'interface, rangement des ROMs)

Version : **v7.1.0-rc.1** (2026-11-25).

Builds : <https://github.com/jerhome/emu-rust/releases> (pré-release `v7.1.0-rc.1`).

Durée : environ 1 h. But : vérifier la finition de l'interface (icône, console cachée, pied de page, Journal,
messages copiables), puis le **rangement des ROMs** et son **annulation**, l'écran **Cores**, et enfin que les jeux
déjà essayés marchent toujours. Essai principal sous **Windows** ; macOS Intel si vous avez le temps. Notez pour
chaque étape « OK » ou ce qui s'est passé (capture d'écran bienvenue).

**Avant de commencer : faites une copie de votre dossier `roms/`** (ou au moins d'un sous-ensemble sur lequel vous
essaierez le rangement). Le rangement déplace des fichiers ; il sait s'annuler, mais une copie évite tout regret.

## Partie 1 — Démarrage et finition (Windows)

1. Installez la rc par-dessus la 7.0.2 et lancez `emu-rust.exe`.
   - **Aucune fenêtre de console noire** ne doit s'ouvrir derrière l'application.
   - L'icône de l'application (lettre « B ») apparaît dans la barre des tâches, dans la fenêtre et sur le `.exe`
     dans l'Explorateur.
   - Vos jeux, favoris et jaquettes de la 7.0.2 sont retrouvés.
2. **Pied de page** : en bas de l'écran principal, « emu-rust 7.1.0-rc.1 — dev by claude@jerhome ». Il est aussi en bas de l'écran **Cores** et
   des Réglages.
3. **Console** : Réglages → cochez « Afficher la console (après redémarrage) », quittez, relancez : la console
   apparaît. Décochez, relancez : elle a disparu.
   - Pour la ligne de commande, utilisez `emu-rust-cli.exe` (livré à côté) : `emu-rust-cli --version` doit répondre
     dans le terminal.
4. **Journal** : ouvrez le panneau « Journal ». Le bouton indique le nombre d'erreurs et d'avertissements, par
   exemple « Journal (1 erreur) ». Le bouton « Copier le journal » met tout le texte dans le presse-papiers : collez-le
   dans le Bloc-notes pour vérifier.
5. **Messages copiables** : provoquez une erreur (par exemple, lancez un jeu dont le core n'est pas installé). Le
   texte du bandeau se sélectionne à la souris et le bouton « Copier » le copie.
6. **En jeu** : lancez un jeu, ouvrez le menu (Ctrl droit ou bouton PS) → « Derniers messages » : les messages
   récents s'affichent et se copient.

## Partie 2 — Rangement des ROMs

1. Déposez quelques jeux **en vrac** à la racine de `roms/` (ou dans un mauvais dossier), de plusieurs systèmes :
   par exemple une ROM Mega Drive, un jeu PlayStation (`.cue` + `.bin`), les deux faces de Maniac Mansion C64
   (`MANIACD0.D64`, `MANIACD1.D64`), un zip CPC.
2. Réglages → « Créer les dossiers des systèmes » : un dossier par système apparaît dans `roms/` (rien d'autre ne
   change). Puis Réglages → « Ranger les ROMs… ». Un **aperçu** liste ce qui sera déplacé et où (`roms/<système>/…`), et ce qui
   reste en place (fichier inconnu, BIOS : « utilisez Importer ce BIOS… »).
   - Vérifiez que `MANIACD0` et `MANIACD1` sont **regroupés** en un seul jeu « MANIACD ».
   - Vérifiez que le `.cue` et ses `.bin` partent ensemble.
3. Cliquez « Ranger N jeu(x) ». Le **rapport du rangement** s'affiche ; « Copier le rapport » le copie.
4. Dans la bibliothèque : les jeux rangés sont toujours là, **une seule tuile** pour Maniac Mansion, favoris et
   jaquettes conservés. Lancez un jeu rangé dont vous aviez une sauvegarde : elle est retrouvée.
5. **Annulation** : Réglages → « Annuler le dernier rangement… » → « Oui, annuler ce rangement ». Les fichiers
   reviennent à leur place d'origine ; la bibliothèque suit.
6. **Fichier ouvert** (facultatif) : ouvrez une ROM dans un autre programme qui la verrouille, puis rangez. Le
   rangement doit s'arrêter proprement avec un message clair (fichier utilisé), sans rien perdre ; relancez une fois
   le fichier libéré.
7. **Lecture seule** (facultatif) : marquez un dossier de `roms/` en lecture seule (Propriétés → Lecture seule). Le
   rangement doit le signaler sans planter et ranger le reste.
8. **État sauvegardé d'un jeu CD** (facultatif) : avec un jeu PlayStation lancé par `pcsx_rearmed`, faites une
   sauvegarde d'état, rangez, relancez et rechargez l'état. Mesuré en test (2026-11-25) : l'état se recharge avec
   `pcsx_rearmed`. Les autres cores PlayStation (`swanstation`, `mednafen_psx`) n'ont pas été mesurés : dites-nous
   ce qui se passe si vous les utilisez.

## Partie 3 — Écran Cores et BIOS

1. Écran **Cores** : pour `mgba`, `gambatte` et `dosbox_pure`, une ligne « Peut être amélioré par un fichier
   d'origine : … » indique le fichier facultatif et s'il est présent (« fichier d'origine absent » sinon).
   L'avertissement correspondant ne s'affiche qu'**une fois par core et par fichier**.
2. **BIOS PlayStation** : si vous avez une autre révision que `scph5502.bin` (ex. SCPH-1002, SCPH-7002), importez-la
   (« Importer un BIOS… ») : elle est reconnue, et l'écran Cores indique sous quel nom (alias de région) elle sert.
3. Les systèmes à un seul core s'affichent sans menu de choix ; la recherche par système filtre la liste.

## Partie 4 — Non-régression

Rejouez rapidement ce qui marchait en 7.0.x :

| Jeu | Système / core | Attendu |
|---|---|---|
| GTA | PlayStation, `pcsx_rearmed` | démarre, carte mémoire retrouvée |
| Road Rash | 3DO, `opera` | démarre |
| B.A.T. (zip) | Amstrad CPC, `cap32` | la disquette est lue |
| Night Trap (2 `.chd`) | Mega CD 32X, `picodrive` | le jeu démarre |

## Partie 5 — macOS Intel (facultatif)

Lancez le `.app` : icône dans le Dock, pied de page, Journal, un rangement sur une copie de dossier, puis son
annulation.

## Ce qu'il faut rapporter

Pour chaque partie : « OK » ou le symptôme, avec le texte copié du message ou du Journal (bouton « Copier ») et
une capture si l'affichage est en cause. Pour le rangement, joignez le rapport copié.
