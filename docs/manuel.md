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

### Les cores

Un core est nécessaire pour chaque système. Ouvrez l'écran **Cores** : il propose d'installer les cores
recommandés. Si un core manque au lancement d'un jeu, l'application propose de l'installer.

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
  Vous changerez de disquette en cours de partie (voir plus bas).
- **Navigation** : flèches pour choisir, Entrée pour jouer, Tab pour atteindre la barre du haut, Échap pour
  effacer la recherche.

## 3. En jeu

### Le menu

Appuyez sur **Échap** : le menu s'ouvre et le jeu s'arrête. Vous pouvez y reprendre la partie, sauvegarder,
charger, changer de disquette, régler la manette ou quitter.

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

- **Dossiers de ROMs** et scan à l'ouverture ;
- **Bases d'identification** (DAT) : elles aident à reconnaître et nommer les jeux ; bouton de mise à jour ;
- **Jaquettes** : activation et réessai des jaquettes absentes ;
- **Identifiants** pour les services de jaquettes facultatifs (les mots de passe ne s'affichent jamais) ;
- **Manettes** : liste et apprentissage (à partir de la 7.2) ;
- **Afficher le terminal** (après redémarrage) : sous Windows, la fenêtre noire est cachée par défaut ;
  cochez la case pour la voir. Utile surtout pour dépanner ;
- **Journal** : les derniers messages de l'application. Les avertissements et erreurs sont mis en évidence.
  Le bouton **Copier** copie le texte : joignez-le quand vous signalez un problème.
  Le journal est aussi écrit dans le fichier `logs/emu-rust.log`, dans le dossier de données.

N'oubliez pas **Enregistrer** après une modification.

## 7. Problèmes fréquents

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
