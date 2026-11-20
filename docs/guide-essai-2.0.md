# Guide d'essai — version 2.0 (Atari ST)

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v2.0.0-rc.1`).

Durée : 30 à 45 minutes. But : vérifier l'Atari ST au clavier 60 % (Windows), sur macOS Intel et dans Docker (WSLg).
Notez pour chaque étape « OK » ou ce qui s'est passé (capture d'écran bienvenue).

## 0. Préparer

1. Lancer emu-rust (double-clic, ou `emu-rust` dans un terminal) : la bibliothèque s'ouvre.
2. **Cores** → installer `hatari` (recommandé pour « Atari - ST »).
3. Toujours dans **Cores**, section **BIOS libres** : EmuTOS 1.4, variante proposée selon la langue du système
   (repli : « EmuTOS 192 Ko — français »). Changer la variante avec Gauche / Droite, puis **Installer**.
   Attendu : « installé : EmuTOS 192 Ko — français (ST/STE), version 1.4 ». Si vous aviez déjà votre propre
   `tos.img`, il est signalé « fourni par vous (conservé) » et n'est remplacé qu'après confirmation (il est alors
   renommé `tos.img.user-<date>`, jamais effacé).
4. Copier vos disquettes ST (`.st`, `.msa`, `.stx`, `.ipf`, `.dim`, `.m3u`, ou des `.zip`) dans le dossier des ROMs
   puis **Scanner**. Les jeux en plusieurs disquettes (`(Disk 1 of 2)`…) apparaissent comme un seul jeu.

Variante : supprimer `tos.img` puis lancer un jeu ST : la bibliothèque propose « Installer et lancer » EmuTOS.

## 1. Touche hôte (Ctrl droit, partout, macOS compris)

Sur un ordinateur émulé, Échap, Ctrl et les lettres vont à l'Atari. Les raccourcis passent par **Ctrl droit** :

| Touche | Action |
|---|---|
| Ctrl droit seul (appui + relâchement) | menu (pause, libère la souris) ; à nouveau : fermer le menu |
| Ctrl droit + S / L | sauver / charger l'état |
| Ctrl droit + , / . | emplacement précédent / suivant |
| Ctrl droit + R / P / F / I | reset / pause / plein écran / statistiques |
| Ctrl droit + Espace (maintenu) | avance rapide |
| Ctrl droit + K | clavier virtuel |
| Ctrl droit + M | capturer / libérer la souris |
| Ctrl droit + J | bascule clavier / joystick (mémorisée pour le jeu) |
| Ctrl droit + D | disquette suivante |

Clavier Mac sans Ctrl droit : dans `config.toml`, `[hotkeys]` `host_key = "SuperRight"` (⌘ droit).
En console (Mega Drive…), rien ne change : Échap = menu, Ctrl + lettre comme en 1.0.

## 2. Bureau Atari ST (souris)

1. Bibliothèque → **Bureau Atari ST** (barre du haut). Le bureau GEM d'EmuTOS apparaît après quelques secondes.
2. Cliquer dans l'image : la souris est **capturée** (OSD « Souris capturée — Ctrl droite pour libérer »), le
   pointeur système disparaît, celui de l'Atari bouge. Ouvrir le menu « Desk » ou « Bureau », double-cliquer
   l'icône du lecteur A.
3. Ctrl droit : le menu s'ouvre et la souris est libérée. Ctrl droit + M : capture / libération sans menu.
4. Basculer vers une autre fenêtre (Alt+Tab) : la souris doit être libérée, aucune touche ne reste enfoncée.
5. Menu → **Clavier et souris…** : sensibilité (Gauche / Droite), capture au clic oui / non.

Docker (noVNC / WSLg) : si la capture se comporte mal, mettre `[computer] grab_pointer = false` : mode « sans
capture » (gardez le pointeur au-dessus de l'image). À signaler : ce qui marche ou pas sans ce réglage.

## 3. Clavier (clavier 60 % AZERTY)

1. Sur le bureau, taper du texte (ex. renommer un fichier, ou Alt + A pour ouvrir le lecteur A) : avec EmuTOS
   français, les touches tapent ce qui est gravé sur votre clavier AZERTY (sauf quelques signes).
2. **Clavier virtuel** : Ctrl droit + K. Il s'affiche en bas, la partie continue. Flèches / Tab pour choisir,
   Entrée pour taper ; ou à la souris (souris libérée). Essayer F1–F10, Help, Undo, le pavé numérique.
   Shift / Control / Alternate restent enfoncées jusqu'à la touche suivante. Échap ou Ctrl droit + K : fermer.
   À vérifier en priorité : **Help** et **Undo** (correspondance avec hatari non confirmée).

## 4. Un jeu (joystick au clavier)

1. Lancer un jeu ST depuis la bibliothèque. Mode par défaut : **Clavier** (toutes les touches vont à l'Atari).
2. Ctrl droit + J : mode **Joystick** (OSD). ZQSD (positions WASD) et les flèches dirigent, J = tir, K = tir 2 ;
   les autres touches vont toujours au clavier ST (Espace, F1…). Ctrl droit + J : retour au mode Clavier.
3. Quitter (menu → Retour à la bibliothèque) puis relancer : le mode choisi est conservé pour ce jeu.

## 5. Disquettes

1. Jeu à deux disquettes : quand le jeu demande la disquette 2, Ctrl droit + D (OSD « Disquette 2/2 : … »), ou
   menu → **Disquettes…** → choisir la disquette. « Éjecter » vide le lecteur ; « Ajouter une image du même
   dossier » ajoute une autre disquette pour la session.
2. Vos fichiers ne sont **jamais modifiés** : hatari travaille sur des copies dans
   `<données>/saves/hatari/<jeu>.disks/` (la progression écrite sur disquette y reste). Vérifier que la date de
   modification de vos fichiers d'origine ne change pas après avoir sauvegardé dans un jeu.
3. Menu → Disquettes → **Réinitialiser les disquettes…** (confirmation) : le jeu redémarre depuis vos fichiers
   d'origine (lancement depuis la bibliothèque seulement).
4. Sauvegarder / charger un état : un avertissement unique rappelle que l'état ne contient pas les disquettes.

## 6. Options par jeu

Menu → **Options du core** : « Portée des changements : ce jeu / tous les jeux ». En « ce jeu », changer par
exemple la machine (ST / STE) ; l'option est marquée « (ce jeu) » et enregistrée dans
`<données>/game-options/hatari/<jeu>.toml`. « Redémarrer le jeu » applique les options qui l'exigent. Vérifier
qu'un autre jeu garde la valeur globale.

## À remonter

- Ce qui ne marche pas, avec le système (Windows / macOS / Docker WSLg / noVNC).
- Capture de la souris : fluide ? pointeur qui sort de la fenêtre ? (macOS : autorisations d'accessibilité.)
- Touches du clavier ST mal placées (avec EmuTOS français), Help / Undo.
- Tout message d'erreur affiché.

## Limites connues (2.0.0-rc.1)

- Une disquette **ajoutée en cours de jeu** (menu Disquettes → Ajouter) n'est pas mémorisée : la rajouter à la
  session suivante crée une nouvelle copie de travail (`02 X.st`, `03 X.st`…) qui repart de l'original ; ces copies
  ne sont pas nettoyées automatiquement (dossier `<données>/saves/hatari/<jeu>.disks/`). Corrigé en 2.1.
- La manette PS4 n'est pas encore branchée (prévue après la 2.0).
