# Guide d'essai — version 2.1 (Mega CD express, manette)

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v2.1.0-rc.1`).

Durée : 30 à 45 minutes. But : jouer à **Dune (Mega CD)** sur le Mac Intel avec vos BIOS, vérifier que la partie
sauvegardée survit à toutes les façons de quitter, puis essayer la manette DS4. Notez pour chaque point « OK » ou
ce qui s'est passé (capture d'écran bienvenue).

Conseil pour le premier essai : avant de quitter le jeu, faites aussi une **sauvegarde d'état** (F2, ou Ctrl+S) :
si la sauvegarde interne du Mega CD était perdue, l'état permet de reprendre au même endroit (F4, ou Ctrl+L).

Journal : pour voir les messages, lancer l'application depuis le Terminal :
`/Applications/emu-rust.app/Contents/MacOS/emu-rust` (adapter le chemin si l'app est ailleurs).

## 1. Mega CD — préparation

1. Installer `v2.1.0-rc.1` (`.app`), l'ouvrir une fois, vérifier que le core **genesis_plus_gx** est installé
   (écran **Cores** ; c'est le même que pour la Mega Drive).
2. Copier le BIOS dans `~/Library/Application Support/emu-rust/system/` (Finder : Aller → Aller au dossier…), sous
   le nom **exact** `bios_CD_E.bin` (disque européen), `bios_CD_U.bin` (USA) ou `bios_CD_J.bin` (Japon),
   majuscules et minuscules comprises. Dans le doute, copier les trois. emu-rust ne télécharge jamais de BIOS
   commercial.
3. Mettre le jeu dans `<dossier des ROMs>/megacd/Dune (Europe)/` : le `.cue` **et tous ses fichiers** de pistes
   (`.bin`, `.wav`…) ensemble, noms inchangés ; ou un `.chd`. Pas de zip.
4. Bibliothèque → **Scanner**.

La région du disque est lue dans son en-tête (comme le fait le core) ; pour un `.chd`, elle vient de l'étiquette du
nom (`(Europe)`, `(USA)`, `(Japan)`…) ; sans étiquette, un BIOS présent suffit.

## 2. Mega CD — à vérifier

| # | Point | Attendu |
|---|---|---|
| 1 | Bibliothèque | **un seul** « Dune » dans « Mega CD / Sega CD » ; aucune piste listée ; aucun faux jeu Mega Drive ; analyse rapide |
| 2 | Sans le BIOS (le retirer une fois) | dialogue « BIOS Mega CD manquant : bios_CD_E.bin (Mega CD, Europe) », avec le chemin du dossier `system/` ; « Copier le chemin » le met dans le presse-papiers ; remettre le BIOS puis « Réessayer » ⇒ le jeu démarre. Tout se fait au clavier (Tab, Entrée, Échap) |
| 3 | Démarrage | logo du BIOS, puis le jeu ; image stable, vitesse correcte |
| 4 | **Musique et voix du CD** | présentes dès l'introduction ; pas de craquement ; pas de silence après un changement de scène |
| 5 | Touches | exactement celles de la Mega Drive (même profil `genesis_plus_gx`, réglages conservés) ; manette DS4 si branchée (§3) |
| 6 | Sauvegarde du jeu | sauvegarder dans Dune, **quitter par le menu** (Échap, puis « Retour à la bibliothèque »), relancer ⇒ la partie est là ; fichier `~/Library/Application Support/emu-rust/saves/genesis_plus_gx/Dune (Europe).brm` présent |
| 7 | Même essai en quittant par **⌘Q**, puis par la **fermeture de la fenêtre**, puis par **Ctrl+C** dans le Terminal | partie toujours là ; le journal affiche « jeu fermé, sauvegardes écrites » |
| 8 | État sauvegardé / chargé en cours de jeu (F2 / F4) | reprise au même endroit, musique comprise |
| 9 | Un jeu Mega Drive ensuite | inchangé (sauvegarde `.srm`, touches) |
| 10 | Format `.chd` (si disponible) | même résultat ; si le nom n'a pas d'étiquette de région, le BIOS présent suffit |
| 11 | Fichiers du jeu | dates de modification du `.cue` et des pistes inchangées |
| 12 | BIOS d'une autre révision (si c'est votre cas) | avertissement « BIOS Mega CD différent de la référence » avec « Lancer quand même » ; le jeu démarre en général |

Si le jeu refuse de démarrer : relever le message affiché (il nomme le BIOS attendu et le dossier) et les lignes
`Unable to open CD BIOS` du journal.

Limites connues : pas de jeu à plusieurs CD ; pas d'import de BIOS par bouton (3.0) ; pas d'identification
automatique (jaquette trouvée seulement si le fichier porte le nom Redump, ex. `Dune (Europe).cue`). Deux `.cue` de
même nom dans deux dossiers ont maintenant des sauvegardes séparées (§4).

## 3. Manette PS4 (DualShock 4)

1. USB : brancher la DS4, lancer un jeu Mega Drive (puis Dune) : la croix directionnelle et Croix / Rond / Carré
   (B / C / A) répondent ; notification « Manette connectée ».
2. Même chose en **Bluetooth** (appairage : maintenir PS + Share jusqu'au clignotement).
3. SNES / NES / Game Boy / Master System : Start = Options, Select = Share.
4. Clavier **et** manette en même temps ; deuxième manette = joueur 2.
5. Raccourcis : PS + R1 (sauver l'état), PS + L1 (charger), PS + Options (menu), PS + R2 maintenu (avance rapide),
   PS seul = rien. Atari ST : PS + R3 ouvre le clavier virtuel, PS + L3 passe à la disquette suivante.
6. Débrancher en jeu : pas de plantage, le jeu continue, notification ; rebrancher : reprise.
7. Stick gauche : D-pad sans dérive ; ajuster la zone morte si la manette est usée.
8. macOS : noter si le bouton PS ouvre autre chose, ou si une autorisation système est demandée.
9. Si la DS4 n'est pas reconnue : noter USB ou Bluetooth, et si Steam ou un autre utilitaire tournait.

## 4. Jeux homonymes, disquettes ajoutées (identité de jeu)

Avant de commencer : **copier** le dossier `saves/` et `states/` (sauvegarde de sécurité ; rien ne doit être perdu,
c'est justement ce qu'on vérifie). Atari ST : core **hatari**.

| # | Point | Attendu |
|---|---|---|
| 1 | Jeux déjà joués en 2.0 (un seul de ce nom) | sauvegardes, états et options retrouvés comme avant ; aucun message ; aucun fichier renommé dans `saves/` ni `states/` |
| 2 | Deux jeux homonymes : `Jeu1/disk1.st` et `Jeu2/disk1.st` (ou deux `Game.sfc` de dossiers différents). Lancer le premier, sauvegarder dans le jeu et un état, quitter ; lancer le second | au premier lancement du second, bandeau « Deux jeux portent le même nom « disk1 » : leurs sauvegardes sont maintenant séparées. » ; dans `saves/<core>/`, fichiers `disk1 (2).*` et dossier `disk1 (2)/` ; le premier garde ses fichiers |
| 3 | Homonymes **déjà joués ensemble en 2.0** (ils partageaient `G.srm`) | le second jeu reçoit une **copie** de la sauvegarde et des options (bandeau « La sauvegarde du jeu a été copiée… », « Options du jeu reprises… ») ; les **états ne sont pas copiés** : le bandeau dit à quel jeu ils restent attachés |
| 4 | Relancer chacun des deux | chacun retrouve sa partie ; plus de bandeau |
| 5 | État d'un autre fichier du même nom (remplacer la ROM par une autre version au même endroit, puis F4 / Ctrl+L, PS + L1, ou « Charger l'état » du menu) | question « Cet état peut venir d'un autre fichier du même nom… » ; **Non** a le focus (Entrée = ne rien charger) ; « Oui, charger » charge. Un état ancien d'un jeu sans homonyme se charge sans question |
| 6 | Atari ST : menu Disquettes → ajouter une image du même dossier ; quitter ; relancer | la disquette ajoutée est de nouveau dans la liste (section « Disquettes ajoutées »), même ordre, sans être insérée ; la disquette réinsérée au démarrage est la bonne |
| 7 | « Disquettes ajoutées » → Entrée sur une disquette | « Annuler » (focus), « Retirer de la liste (la copie est gardée) », « Retirer et supprimer la copie… » (nouvelle confirmation, taille et date affichées). Sur la disquette dans le lecteur : message « éjectez-la d'abord » |
| 8 | Ajouter des disquettes jusqu'à la limite (32 au total) | message « Liste pleine : retirez d'abord une disquette ajoutée. » ; rien n'est oublié en silence |
| 9 | « Nettoyer les copies… » (jeu lancé avec une rc.1 ou après des retraits) | liste des fichiers non utilisés (taille, date) ; **rien n'est coché** ; « Supprimer la sélection » demande confirmation et ne supprime que les lignes cochées ; « Reprendre » remet une copie dans la liste |
| 10 | Tout au clavier, puis à la manette | Flèches / Tab, Entrée, Espace (cases), Échap revient d'un niveau |
| 11 | Retour à la 2.0 (facultatif) | les jeux non homonymes retrouvent tout ; les fichiers `G (2).*` sont simplement ignorés |
