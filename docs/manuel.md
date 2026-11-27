# Manuel d'emu-rust

emu-rust est un lecteur de jeux rétro. Il range vos jeux dans une bibliothèque, télécharge ce qui est libre et
lance chaque jeu avec le bon « core » (le programme qui imite la console). Ce manuel décrit l'usage courant.

## 1. Premiers pas

### Lancer l'application

Ouvrez emu-rust : la bibliothèque s'affiche. Au début, elle est vide.

### Le dossier des ROMs

1. Copiez vos jeux dans le dossier `roms/`, un sous-dossier par système (par exemple `roms/snes/`).
2. Vous pouvez aussi ajouter un autre dossier : **Réglages**, puis **Dossiers de ROMs**, puis **Ajouter**.
3. Cliquez sur **Scanner**. Les jeux apparaissent.

### Ranger les ROMs

Si vos jeux sont en vrac dans `roms/`, emu-rust peut les ranger pour vous, un sous-dossier par système.

- **Réglages**, section des dossiers de ROMs : **Créer les dossiers des systèmes** crée les sous-dossiers vides
  (rien n'est déplacé) ; **Ranger les ROMs…** analyse le dossier `roms/` d'emu-rust (pas les dossiers ajoutés).
- **Aperçu** : rien n'est encore déplacé. Les jeux sûrs sont cochés ; les suggestions ne le sont pas, avec leur
  raison. Décochez ce que vous voulez garder en place, puis cliquez sur **Ranger**. Les fichiers sont déplacés,
  jamais renommés, écrasés ni supprimés ; favoris, sauvegardes, historique et jaquettes suivent les jeux.
- **Choix du système** : quand une extension sert à plusieurs systèmes (`.bin`, `.iso`…), choisissez le système
  sur la ligne du jeu, ou pour toute l'extension d'un coup. **Autre système…** corrige un jeu mal reconnu. Un BIOS
  trouvé dans `roms/` est signalé : utilisez **Importer ce BIOS…** plutôt que le rangement.
- **Annulation** : **Annuler le dernier rangement…** remet les jeux à leur place d'origine, après confirmation.
  Une annulation n'est jamais arrêtée en route.
- **Reprise** : si l'ordinateur s'éteint pendant un rangement, emu-rust le termine au démarrage suivant, grâce au
  journal du rangement (dossier `rangement/` des données). Les problèmes sont notés dans le **Journal**.
- **Conseil** : faites une copie de votre dossier `roms/` avant le premier rangement, et fermez toute autre fenêtre
  d'emu-rust. Le rangement est refusé pendant un scan, un téléchargement ou une partie.

### Les cores

Un core est nécessaire pour chaque système. Ouvrez l'écran **Cores** : il propose d'installer les cores
recommandés. Si un core manque au lancement d'un jeu, l'application propose de l'installer.

Sur l'écran **Cores** :

- le champ de recherche trouve un core par son nom **ou par le système** qu'il fait tourner (tapez « 3DO » ou
  « PlayStation ») ;
- **Core utilisé par système** liste tous les systèmes ; ceux qui n'ont qu'un seul core l'affichent sans choix ;
- sous un core installé, **Peut être amélioré par un fichier d'origine** liste les fichiers facultatifs (par
  exemple `gba_bios.bin`, ou les ROM MT-32 pour le PC DOS) : absent, présent et vérifié, md5 inattendu, ou non
  vérifiable ; le bouton **Importer** range le vôtre ;
- **BIOS PlayStation** : un BIOS importé est reconnu par son empreinte, avec sa région. Si les cores le
  cherchent sous un autre nom (par exemple `scph5501.bin` pour un BIOS SCPH-7001 américain), l'import en fait une
  copie sous ce nom et l'écran l'indique (« copie de SCPH-7001 »). Un BIOS d'une autre région est signalé.

### Systèmes non testés

Libretro propose des cores pour beaucoup de machines qu'emu-rust n'a pas testées. Vous pouvez les ajouter vous-même ;
rien n'est garanti pour eux.

- **Ajouter** : sur l'écran **Cores**, après **Vérifier les mises à jour**, cliquez **Ajouter un système non
  testé…**. La liste montre les émulateurs non expérimentaux (**Tout afficher** ajoute les moteurs de jeu et les cores
  expérimentaux) et se filtre par machine. Les cores grisés disent pourquoi : rendu matériel (prévu en 9.0), disques
  seulement (prévu en 8.4), arcade (prévu en 8.6). Une machine déjà prise en charge par emu-rust n'est pas proposée.
- **La fiche** d'un core liste ses machines (une case chacune), sa licence, les fichiers système qu'il annonce
  (jamais téléchargés : **Importer un BIOS…** range les vôtres) et ses avertissements (expérimental, clavier complet
  prévu en 8.5, pas d'états sauvegardés). Un core qu'emu-rust n'a pas examiné demande d'abord une acceptation, une
  seule fois par core : il peut contenir des fichiers système d'origine dont le statut n'est pas vérifié. Usage
  personnel seulement.
- **Ensuite** emu-rust installe le core, lit ses réglages dans un processus à part (un core qui plante n'emporte pas
  l'application), crée le dossier `roms/<identifiant>/`, scanne et vous dit où déposer vos jeux.
- **Premier lancement de chaque jeu** : un avertissement « système non testé » ; **Essayer puis lancer** fait un essai
  de quelques secondes dans un processus à part, sur une copie du jeu. Un jeu en plusieurs fichiers, dans une archive
  ou de plus de 256 Mio n'est pas essayé : seul l'avertissement s'affiche, à chaque lancement. **Ça marche chez moi**
  supprime l'avertissement et l'essai pour ce système et ce core (pas l'acceptation du core). L'étiquette « non
  testé » reste.
- **Réglages > Systèmes non testés** : nom, extensions, cores (l'ordre compte), numéros ScreenScraper et IGDB, fichiers
  système à ne plus signaler, **Recalculer depuis le core** (relit les extensions du core), **Chercher la base
  d'identification** (au plus trois requêtes, par le nom libretro de la machine ; sans nom libretro, aucune base ni
  jaquette n'est cherchée) et **Supprimer**.
- **Supprimer** retire la définition (jamais effacée : dossier `systems/.retired/`) ; les fichiers, les sauvegardes et
  le dossier `roms/<identifiant>/` ne sont pas touchés, et les jeux restent dans la bibliothèque, masqués.
  **Systèmes retirés** : **Restaurer** les fait revenir ; **Oublier ses jeux** les retire pour de bon de la
  bibliothèque (jamais les fichiers). Un système retiré par une bascule vers un système officiel est listé « remplacé
  par… » sans bouton **Restaurer** ni **Oublier ses jeux**. Si l'identifiant du système est celui d'un système
  officiel, le retrait compte comme un conflit : les jeux propres à la définition restent masqués jusqu'à **Oublier
  ses jeux** (pas proposé tant qu'un système actif a le même identifiant) ou **Restaurer**, et la
  suppression avec oubli est refusée tant qu'un de ces jeux reste dans `roms/<identifiant>/` : déplacez-les.
- **Systèmes en erreur** : une définition illisible est listée avec la raison et ses jeux restent intacts. Au-delà de
  256 définitions, les suivantes sont ignorées : une seule ligne le dit, la liste est repliée.
- **Limite** : un core qui demande le rendu matériel est refusé (« prévu en 9.0 »), même s'il ne l'exigeait qu'en
  option et que le jeu s'est mal chargé pour une autre raison.

### Ce que l'application télécharge, et ce que vous fournissez

L'application télécharge seulement :

- les **cores** (depuis le site officiel de libretro) ;
- les **BIOS libres** (par exemple EmuTOS pour l'Atari ST) ;
- les **jaquettes** et les informations sur les jeux (si vous les activez) ;
- la SoundFont libre pour le PC DOS, **seulement si vous le demandez**.

Elle ne télécharge **jamais** de jeu ni de BIOS commercial. Les jeux et les BIOS d'origine (PlayStation,
Mega CD, Kickstart de l'Amiga, 3DO...) sont à vous : procurez-vous-les vous-même. L'application reconnaît un
fichier par son empreinte et le range au bon endroit grâce au bouton **Importer**.

## 2. La bibliothèque

- **Scan** : le bouton **Scanner** parcourt vos dossiers. Vous pouvez aussi scanner à chaque ouverture
  (**Réglages**). Les jeux sont reconnus par leur contenu, pas seulement par leur nom.
- **Jaquettes** : elles sont désactivées au départ. Activez-les dans **Réglages**, rubrique **Jaquettes**.
  Pour réessayer celles qui manquent : **Rechercher à nouveau les jaquettes absentes**.
- **Chercher** : tapez `/` (ou Ctrl+F) puis le nom du jeu.
- **Favoris** : sélectionnez un jeu et appuyez sur `F` (ou cliquez l'étoile). L'onglet **Favoris** les regroupe.
  L'onglet **Récemment joués** garde les derniers jeux lancés.
- **Jeux à plusieurs disquettes** : un jeu en plusieurs disquettes (fichier `.m3u`) n'apparaît qu'une fois.
  Des disquettes numérotées par leur dernier chiffre (`MANIACD0.D64`, `MANIACD1.D64`) sont aussi regroupées.
  Vous changerez de disquette en cours de partie (voir plus bas).
- **Clic droit sur une vignette** : un seul menu, **Réinitialiser la jaquette** (touche `R`) et, pour un jeu
  Mega CD, **Ce jeu est un CD 32X** ou **Ce n'est pas un CD 32X** (touche `X`).
- **Navigation** : flèches pour choisir, Entrée pour jouer, Tab pour atteindre la barre du haut, Échap pour
  effacer la recherche.

### Jeux Sega CD 32X

Quelques jeux Mega CD demandent aussi la 32X (par exemple Night Trap en version 32X). Ils ne tournent qu'avec
le core **picodrive** : l'application ne les lance jamais avec genesis_plus_gx, et propose d'installer picodrive
s'il manque. Un tel jeu porte la pastille **32X**. Il est reconnu par son disque ou par son nom (« (Sega CD
32X) », « (Mega CD 32X) », « (CD 32X) »…). Sinon, marquez-le vous-même : sélectionnez-le, puis clic droit
**Ce jeu est un CD 32X**, ou la touche `X`. La marque est gardée dans `config.toml` (`[library] cd32x`) ; les
sauvegardes faites avec l'autre core ne sont pas reprises. La même touche retire la marque.

### Fichiers d'origine du jeu sélectionné

Quand le core qui lancera le jeu sélectionné exige ou gagne à avoir un fichier que vous devez fournir (BIOS,
Kickstart, ROM MT-32…), un bandeau s'affiche en bas de la bibliothèque. Il dit, pour chaque fichier, s'il est
**absent**, présent avec un **md5 inattendu** (une autre version) ou **non vérifié**, et ce qu'il apporte.
Le bouton **Importer…** ouvre l'import ; un fichier inconnu peut être importé quand même, comme « non vérifié ».

### Pied de page et journal

En bas de chaque écran (bibliothèque, Réglages, Cores, Aide, assistant, et le menu en jeu) : la version
d'emu-rust, puis le bouton **Journal**. Le bouton affiche le nombre d'erreurs et d'avertissements non lus.
Le **Journal** liste les messages techniques de l'application et des cores, avec un filtre (**Erreurs**,
**Avertissements**, **Tout**), **Copier le journal** et **Ouvrir le dossier des journaux**. PgUp / PgDn font
défiler, Échap revient à l'écran précédent. Le journal est aussi écrit dans `logs/emu-rust.log` (dans le
dossier de données) ; le fichier de la séance précédente est gardé sous `emu-rust.log.1`.

## 3. En jeu

### Le menu

Appuyez sur **Échap** : le menu s'ouvre et le jeu s'arrête. Vous pouvez y reprendre la partie, sauvegarder,
charger, changer de disquette, régler la manette ou quitter.

En bas du menu, **Derniers messages** reprend les messages affichés pendant la partie (avec l'heure) ; **Copier**
les copie tous. Le bouton **Journal** du pied de page ouvre le journal sans quitter le jeu ; Échap revient au menu.

### La touche hôte (systèmes « ordinateur »)

Sur les ordinateurs (Atari ST, Amiga, PC DOS, Amstrad CPC, Commodore 64), le clavier sert au jeu. Un
raccourci utilise donc une touche à part, la **touche hôte** : **Ctrl droit** par défaut.

- Touche hôte seule : ouvre le menu.
- Touche hôte + une lettre : un raccourci (voir le tableau du §4).

Si votre clavier n'a pas de Ctrl droit, une autre touche peut être choisie dans la configuration.
Sur un Mac, la touche ⌘ de droite est possible.

### Sauvegardes

Il y a deux sortes de sauvegarde.

- **Sauvegarde d'état** : une photo complète du jeu à un instant donné. Il y a plusieurs emplacements.
  **F2** sauvegarde, **F4** charge, **F6** / **F7** changent d'emplacement.
  Attention : sur les jeux à disquettes, l'état ne contient pas les disquettes.
- **Sauvegarde en jeu** : celle du jeu lui-même (cartouche avec pile, carte mémoire, fichier de sauvegarde).
  Elle se fait par les menus du jeu et est conservée automatiquement.

### Changer de disquette

Pour un jeu à plusieurs disquettes : menu **Disquettes**, ou touche hôte + **D** (disquette suivante).
Vos jeux d'origine ne sont jamais modifiés : le jeu écrit sur une copie de travail.

## 4. Clavier

Raccourcis par défaut en jeu (sans touche hôte) :

| Touche | Action |
|---|---|
| Échap | Menu |
| P | Pause |
| F5 | Redémarrer le jeu |
| F2 / F4 | Sauvegarder / charger l'état |
| F6 / F7 | Emplacement précédent / suivant |
| Espace (maintenu) | Avance rapide |
| F11 | Plein écran |
| F3 | Afficher les statistiques |

Sur un portable où les touches F demandent la touche Fn, utilisez **Ctrl + lettre** : S sauvegarde,
L charge, `,` et `.` changent d'emplacement, R redémarre, F plein écran, I statistiques.

Sur les ordinateurs, avec la touche hôte maintenue :

| Touche hôte + | Action |
|---|---|
| S / L | Sauvegarder / charger l'état |
| `,` / `.` | Emplacement précédent / suivant |
| R | Redémarrer |
| P | Pause |
| Espace (maintenu) | Avance rapide |
| F / I | Plein écran / statistiques |
| K | Clavier virtuel à l'écran |
| M | Capturer ou libérer la souris |
| J | Passer de clavier à joystick |
| D | Disquette suivante |

La souris est capturée au premier clic dans la fenêtre ; la touche hôte la libère.

Pour les consoles, le clavier imite la manette. Par défaut : flèches (ou ZQSD) pour la croix, Entrée pour
Start, Maj droite pour Select. Pour changer les touches, ouvrez le menu puis la configuration des touches.

## 5. Manettes

### DualShock 4 (manette PlayStation 4)

Branchez-la en USB, ou en Bluetooth, avant ou pendant l'usage. Une notification indique le joueur attribué.
Le premier branché est le joueur 1. Le clavier reste utilisable en même temps.

Le bouton **PS** (ou **Share** si votre système garde PS pour lui) sert de touche de raccourci. Maintenez-le,
puis :

| PS + | Action |
|---|---|
| Options | Menu |
| Share | Pause |
| R1 / L1 | Sauvegarder / charger l'état |
| Droite / Gauche | Emplacement suivant / précédent |
| R2 (maintenu) | Avance rapide |
| Triangle | Plein écran |
| Carré | Redémarrer |
| Croix | Statistiques |

Un appui bref sur PS ne fait rien : pas de menu par accident.

### Manette non reconnue (à partir de la version 7.2)

Les manettes USB bon marché (type SNES) ne sont parfois pas reconnues, ou ont des boutons mal placés.
Vous pouvez les apprendre une fois ; l'application s'en souvient pour ce modèle.

1. Branchez la manette. Si elle est inconnue, une notification dit : « Manette non reconnue ».
2. Ouvrez **Réglages**, puis **Manettes**. La liste indique pour chaque manette : reconnue, apprise ou non reconnue.
3. Cliquez sur **Configurer** (aussi possible pour une manette reconnue dont les boutons sont faux).
4. Choisissez la forme de votre manette : SNES, NES, Mega Drive 3 boutons ou 6 boutons.
5. Suivez les invites, une touche à la fois (« Appuyez sur A... »). Commencez par la croix.
   **Passer** saute une touche, **Retour** revient en arrière, **Annuler** (ou Échap) abandonne.
   Un bouton déjà utilisé est refusé avec un message.
6. À la fin, cliquez sur **Enregistrer**.

La manette apprise se rattache seule à chaque branchement. **Oublier** efface l'apprentissage.
Pendant l'apprentissage, la manette ne commande pas les menus : utilisez la souris ou le clavier.

Sur une manette apprise, **Select** sert de touche de raccourci (comme PS sur la DualShock) :
Select + Start ouvre le menu, Select + R1 sauvegarde l'état, Select + L1 le charge.
Appuyé **seul**, Select reste transmis au jeu (beaucoup de jeux en ont besoin) ; pendant une combinaison, le jeu
peut le voir une fraction de seconde avant que le second bouton soit pressé.

## 6. Réglages

Ouvrez **Réglages** depuis la bibliothèque. On y trouve :

- **Dossiers de ROMs** et scan à l'ouverture ; **Créer les dossiers des systèmes** et **Ranger les ROMs…**
  (voir « Ranger les ROMs ») ;
- **Bases d'identification** (DAT) : elles aident à reconnaître et nommer les jeux ; bouton de mise à jour ;
- **Jaquettes** : activation et réessai des jaquettes absentes ;
- **Identifiants** pour les services de jaquettes facultatifs (les mots de passe ne s'affichent jamais) ;
- **Manettes** : liste et apprentissage (à partir de la 7.2) ;
- **Afficher le terminal** (après redémarrage) : sous Windows, la fenêtre noire est cachée par défaut ;
  cochez la case pour la voir (emu-rust se relance alors avec `emu-rust-cli.exe`, placé à côté). Sur Mac,
  l'application se rouvre dans le Terminal. Utile surtout pour dépanner ;
- **Systèmes non testés** : modifier, supprimer et restaurer les systèmes ajoutés (voir « Systèmes non testés ») ;
- **Ouvrir le journal** : la page **Journal** (voir « Pied de page et journal »).

Sous Windows, les commandes en ligne (`--help`, `--install-core`…) passent par `emu-rust-cli.exe` :
`emu-rust.exe` n'a pas de console.

N'oubliez pas **Enregistrer** après une modification.

## 7. Problèmes fréquents

**Copier un message d'erreur.**
Les messages d'erreur se sélectionnent à la souris, et un bouton **Copier** les accompagne. Joignez le texte, ou
le **Journal** entier (**Copier le journal**), quand vous signalez un problème.

**Un bandeau jaune parle d'un BIOS ou d'un fichier d'origine.**
Ce core marche mieux (ou exige) un fichier que vous devez fournir. Cliquez sur **Importer**, choisissez votre
fichier : il est reconnu et rangé. Si le bandeau dit que le fichier est facultatif, le jeu se lance quand même.
L'application ne télécharge jamais ces fichiers.

**« BIOS manquant » au lancement.**
Le système exige un BIOS (par exemple le Mega CD). Importez le vôtre depuis l'écran **Cores**, puis relancez.

**Un jeu ne se lance pas : « Core manquant ».**
Cliquez sur **Installer**, ou ouvrez l'écran **Cores**.

**Pas de son.**
Vérifiez le volume du système et la sortie audio choisie. Le son est coupé en pause et pendant l'avance
rapide. Sous noVNC (navigateur), il n'y a pas de son. Si le problème reste, regardez le **Journal**.

**Une jaquette manque.**
Activez les jaquettes dans les **Réglages** puis lancez **Rechercher à nouveau les jaquettes absentes**.
Hors ligne, seul le cache est utilisé.

**Le jeu est lent sur un Mac modeste.**

- Gardez les réglages par défaut : ils sont légers.
- Pour l'Amiga, préférez le core plus léger (puae2021) dans l'écran **Cores**.
- Pour le Commodore 64, gardez le core par défaut (l'autre est environ cinq fois plus lent).
- Pour la PlayStation, gardez le core par défaut (rapide).
- Fermez les autres applications, et évitez l'avance rapide longtemps.
- F3 affiche les statistiques pour voir si l'image tient son rythme.

**La touche hôte ne répond pas (Mac).**
Certains petits claviers n'ont pas de Ctrl droit : choisissez une autre touche hôte dans la configuration.

**Un réglage ne s'enregistre pas.**
Si le fichier de configuration est invalide, l'application le dit : les réglages valent pour la séance seulement.
Corrigez ou supprimez `config.toml` dans le dossier de données.
