# Guide d'essai — version 3.0 (Amiga, import de BIOS, dossier des ROMs)

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v3.0.0-rc.1`).

Durée : 45 minutes à 1 heure. But : vérifier l'Amiga au clavier 60 % (Windows), sur macOS Intel et dans Docker
(WSLg), l'import d'un Kickstart par le bouton, et le bouton « Ouvrir le dossier des ROMs ». Notez pour chaque étape
« OK » ou ce qui s'est passé (capture d'écran bienvenue).

Rappel : emu-rust ne télécharge **jamais** de Kickstart ni de jeu commercial. AROS (système libre compatible) est
intégré au core puae ; le Kickstart d'origine, si vous en possédez un (Amiga Forever, ou ROM lue sur votre Amiga),
s'importe par le bouton « Importer un BIOS… ».

## 0. Préparer

1. Lancer emu-rust : la bibliothèque s'ouvre.
2. **Cores** → installer `puae` (recommandé pour « Commodore - Amiga »). Sur Mac, installer aussi `puae2021`
   (voir §6).
3. Barre du haut de la bibliothèque : **Ouvrir le dossier des ROMs** (§5). Copier vos disquettes Amiga (`.adf`,
   `.adz`, `.dms`, `.fdi`, `.ipf`, `.m3u`, ou des `.zip` de disquettes) dans `roms/amiga/`, les jeux WHDLoad
   (`.lha`) et disques durs (`.hdf`) aussi dans `roms/amiga/` (hors de ce dossier, ces extensions sont ignorées).
4. **Scanner**. Les jeux en plusieurs disquettes (`(Disk 1 of 2)`…) apparaissent comme un seul jeu. Si le scan
   signale « N fichiers non classés » (par exemple un `.ipf` hors de `roms/amiga/` ou `roms/atarist/`), le bouton
   **Voir les fichiers non classés** (barre du bas) donne la liste et le dossier où ranger chacun.
5. Facultatif : **Réglages** → **Mettre à jour la base (DAT)** : l'Amiga reçoit le DAT TOSEC (disquettes) et le
   DAT WHDLoad ; l'Atari ST reçoit désormais le DAT TOSEC (et non plus No-Intro).

## 1. Sans Kickstart : AROS, avertissement une seule fois

1. Dossier des BIOS vide (pas de `kick*.A500`). Lancer un jeu Amiga.
2. Attendu : un dialogue « Pas de Kickstart (kick34005.A500) » explique que le jeu démarre avec AROS (compatibilité
   limitée), avec **Importer un Kickstart…**, **Continuer avec la ROM intégrée** et **Annuler** (clavier : Tab
   pour passer d'un bouton à l'autre, Entrée pour valider, Échap pour annuler).
3. **Continuer** : le jeu démarre sous AROS. Quitter, relancer le même jeu : **plus de dialogue** (avertissement
   unique par jeu). Un autre jeu : le dialogue revient une fois.
4. Bibliothèque → **Workbench / AROS** : le bureau AROS (ou l'écran « insérez une disquette ») s'affiche, mode
   Clavier.
5. Écran **Cores** : la ligne de puae indique « AROS intégré — Kickstart recommandé » en bleu (jamais en rouge).

## 2. Importer un Kickstart (bouton)

1. **Cores** → **Importer un BIOS…** (en haut), ou **Importer un Kickstart…** sur la ligne de puae, ou depuis le
   dialogue du §1.
2. Windows / macOS / Linux de bureau : la fenêtre de fichiers du système s'ouvre. Docker : le navigateur intégré
   (liste de dossiers et fichiers ; flèches / Tab, Entrée ouvre un dossier ou choisit le fichier, Retour arrière
   remonte, Échap annule ; on peut aussi taper un chemin puis **Aller**).
3. Choisir votre Kickstart 1.3 (A500). Attendu : « Fichier reconnu », la liste des noms sous lesquels il sera
   copié (`kick34005.A500` : nouveau) ; **Importer** → « installé ». Relancer le jeu du §1 : il démarre avec le
   Kickstart (écran de démarrage Commodore), sans dialogue.
4. Fichier non reconnu (par exemple une ROM modifiée) : « Fichier inconnu (md5 …) » et rien n'est copié ; le
   bouton **Importer quand même (non vérifié)…** propose uniquement les noms attendus par les cores (filtre
   « kick », « A500 »…). À essayer avec un fichier quelconque, puis supprimer le fichier importé du dossier
   `system/`.
5. Si un autre fichier existe déjà sous le même nom : il est signalé et **conservé** ; **Remplacer les fichiers
   existants** le renomme d'abord `<nom>.user-<date>` (jamais effacé).
6. ROM chiffrées d'**Amiga Forever** (`amiga-os-130.rom`…) : importées « non vérifiées » sous leur nom ; importez
   aussi `rom.key` (même bouton : il est copié sous le nom `rom.key`).

En ligne de commande : `emu-rust --import-bios <fichier>` ; fichier inconnu : `--bios-name kick34005.A500
--unverified` ; remplacer un fichier existant : `--replace-user-bios`. `emu-rust --list-bios` liste les Kickstarts
attendus par modèle, `emu-rust --check-bios puae` l'état (« absent : AROS utilisé… » n'est pas une erreur).

## 3. Clavier Amiga (clavier 60 % AZERTY)

Touche hôte : **Ctrl droit**, comme pour l'Atari ST (menu, Ctrl droit + K clavier virtuel, Ctrl droit + J
clavier / joystick, Ctrl droit + D disquette suivante, Ctrl droit + F plein écran).

| Touche du PC | Touche de l'Amiga |
|---|---|
| **Alt gauche** | **Amiga gauche** |
| **Alt droit (AltGr ; ⌥ droit sur Mac)** | **Amiga droite** |
| Alt de l'Amiga (gauche / droite) | clavier virtuel seulement (Ctrl droit + K) |
| Help, Del | clavier virtuel, ou leurs touches si votre clavier les a |

À vérifier :

1. Workbench (Kickstart importé) ou un jeu : Amiga droite + une lettre (raccourcis de menu du Workbench), Amiga
   gauche + Amiga droite + une lettre si votre jeu en a.
2. **Alt + F4, Alt + Tab et Alt + Espace restent à Windows** (et Cmd + Tab à macOS) : ils ne sont pas transmis à
   l'Amiga. Pour ces rares combinaisons, utiliser le clavier virtuel.
3. **Alt + Entrée ne bascule pas le plein écran** en mode ordinateur (la combinaison va à l'Amiga) : plein écran =
   **Ctrl droit + F**.
4. Clavier virtuel (Ctrl droit + K) : disposition A500 (F1–F10, Help, Del, pavé, Alt et Amiga).
5. Surcharges possibles dans `config.toml` (ex. touche Menu = Alt droit de l'Amiga) :

   ```toml
   [computer.remap.amiga]
   ContextMenu = "ralt"
   ```

## 4. Souris, joystick, disquettes

1. Jeu point-and-click (Monkey Island, Indiana Jones…) : cliquer dans l'image capture la souris ; Ctrl droit la
   libère (menu).
2. Ctrl droit + J : le clavier pilote le joystick (port 2 de l'Amiga) ; à nouveau : retour au clavier. Mémorisé
   pour le jeu.
3. Jeu à deux disquettes : DF0 et DF1 sont toutes deux insérées au démarrage (MultiDrive) ; Ctrl droit + D ou le
   menu **Disquettes…** changent la disquette de DF0. Vos fichiers d'origine ne sont jamais modifiés (copies de
   travail).
4. Menu **Disquettes…** → **Réinitialiser les disquettes…** : efface les copies de travail **et** les disquettes de
   sauvegarde créées par le jeu (`#SAVEDISK:`), puis redémarre.
5. Menu → **Options de ce jeu** : Modèle (A500, A500+, A600, A1200…), Kickstart, Vidéo, MultiDrive. Un jeu AGA
   (A1200) demande le Kickstart 3.1 (`kick40068.A1200`) ; sans lui, AROS.

## 5. Bouton « Ouvrir le dossier des ROMs »

1. Windows : l'Explorateur s'ouvre sur le dossier des ROMs. macOS : le Finder. Linux : le gestionnaire de fichiers.
2. Si des dossiers supplémentaires sont configurés (**Réglages**), un choix s'affiche d'abord.
3. Si aucun gestionnaire ne peut s'ouvrir, le chemin s'affiche avec **Copier le chemin**.
4. **Docker** : le dossier ne s'ouvre pas (il est dans le conteneur) ; le chemin `/data/roms` s'affiche avec
   l'aide : copier les jeux avec `docker cp`, ou décommenter dans `compose.yaml` le montage
   `${EMU_HOST_ROMS_DIR}:/data/roms` et définir `EMU_HOST_ROMS_DIR` (dossier Windows ou WSL) : le dialogue indique
   alors aussi ce dossier de l'hôte.

## 6. Mac Intel : Kickstart et puae2021

1. Kickstart sur Mac : même bouton **Importer un BIOS…** (fenêtre de fichiers macOS). Le fichier est copié dans le
   dossier des BIOS d'emu-rust (`~/Library/Application Support/emu-rust/system`).
2. **puae2021 est recommandé sur Mac** (plus léger que puae) : écran **Cores**, section **Core utilisé par
   système**, ligne « Amiga » → choisir « puae2021 (recommandé sur Mac) » (« non installé » : l'installer depuis
   sa ligne). Le choix est enregistré (`[cores.preferred]` de `config.toml`) ; revenir à puae de la même façon.
3. À noter : fluidité (son haché ? images perdues ?) avec puae puis avec puae2021, sur le même jeu.

## 7. Docker (WSLg)

1. Lancer l'image comme en 2.0. Bibliothèque : **Ouvrir le dossier des ROMs** affiche le chemin et l'aide (§5).
2. **Importer un BIOS…** utilise toujours le navigateur intégré (§2.2) : naviguer jusqu'au dossier monté, choisir
   le Kickstart.
3. Lancer un jeu Amiga : souris capturée au clic ; si la capture se comporte mal, `[computer] grab_pointer = false`.

## 8. À rapporter

- Pour chaque section : OK, ou ce qui s'est passé (et le système : Windows, macOS, Docker).
- Jeux essayés, modèle utilisé, Kickstart ou AROS, puae ou puae2021.
- Toute touche qui ne fait pas ce qui est annoncé au §3.
