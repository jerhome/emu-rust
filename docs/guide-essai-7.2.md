# Guide d'essai — version 7.2 (Sega CD 32X)

Version : **v7.2.0-rc.1** (pré-release sur la base de tests factices ; votre essai décide de la suite).

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v7.2.0-rc.1`).

Durée : environ 45 min. But : lancer votre **Night Trap 32X** (Europe, deux `.chd`) avec **picodrive** et le BIOS
Mega CD Europe, puis vérifier la pastille « 32X », le marquage à la main et le changement de disque. Notez pour
chaque étape « OK » ou ce qui s'est passé (capture d'écran bienvenue).

Rappel : emu-rust ne télécharge **aucun** jeu ni **aucun** BIOS commercial. Le BIOS Mega CD vient de **vos**
fichiers ; la 32X n'a besoin d'aucun BIOS (picodrive démarre avec son propre code, aucune ROM Sega). Aucun fichier de votre dossier n'est modifié ni copié.

## À savoir avant de commencer

- **Où ranger** : sous `roms/megacd/`. Un `.chd` rangé ailleurs n'est ni classé « Mega CD » ni marquable : il faut
  le déplacer sous `megacd/`.
- **Deux `.chd` d'un même jeu** : nommés `… (Disc 1)` / `… (Disc 2)` (noms identiques au marqueur près), ils
  forment **un seul jeu** et une seule marque suffit. Vos fichiers
  `Night Trap (Europe) (Disc 1) (Mega-CD 32X).chd` et `… (Disc 2) (Mega-CD 32X).chd` sont dans ce cas, et
  portent déjà l'étiquette : la pastille doit apparaître d'elle-même. Sinon, **marquer chaque disque**.
- **Reconnaissance automatique** : une étiquette entre parenthèses dans le nom du jeu ou du fichier : `(Sega CD
  32X)`, `(Mega-CD 32X)`, `(Mega CD 32X)`, `(32X CD)`, `(CD 32X)` ou `(32X)` (casse et tiret indifférents). Les
  crochets `[32X]` (TOSEC) et les noms sans étiquette ne sont **pas** reconnus : marquage à la main.
- **Rescan complet** : un `.cue` déjà scanné avant la 7.2 n'est pas ré-analysé ; pour qu'un `.cue` profite de la
  nouvelle borne d'identification (800 Mio), faire un **rescan complet** (ou le marquer à la main).
- **Core** : un jeu marqué est **toujours** lancé avec `picodrive` (la préférence « Core utilisé » du Mega CD est
  sans effet sur lui). `genesis_plus_gx` n'émule pas la 32X : il n'est jamais utilisé en repli. Si `picodrive`
  manque, l'application propose de l'installer (avec la raison) ; sur refus, le jeu ne se lance pas.
- **BIOS** : celui de la **région du disque**. La région d'un `.chd` ne vient que de son nom : un `.chd` Europe
  avec le seul BIOS USA donne « Missing BIOS », comme en 6.0. Fichier attendu pour l'Europe : `bios_CD_E.bin`
  (USA : `bios_CD_U.bin`, Japon : `bios_CD_J.bin`), dans le dossier `system/`.
- **Sauvegardes** : picodrive écrit la RAM interne dans `saves/picodrive/` ; les `.brm` de `genesis_plus_gx` ne
  sont pas repris.

## Partie 1 — Reconnaissance et pastille

1. Écran **Cores** : installer `picodrive` (ou laisser l'application le proposer au premier lancement).
2. Ranger les deux `.chd` de Night Trap sous `roms/megacd/`, **Scanner** : **un seul** jeu « Night Trap » sous
   « Mega CD », avec une pastille rouge « 32X » en haut à gauche de la vignette. Il reste compté et filtré sous
   « Mega CD / Sega CD ».
3. Noter si le jeu apparaît en deux entrées (alors les noms ne se regroupent pas : voir partie 3).

## Partie 2 — Lancer avec picodrive

1. Importer votre BIOS Mega CD Europe (Cores → **Importer un BIOS…**) si ce n'est pas fait.
2. Sélectionner le jeu (flèches), **Entrée** : sans BIOS, le dialogue « BIOS manquant : bios_CD_E.bin (Mega CD /
   Sega CD, Europe) » avec **Importer un BIOS…** ; avec lui, le jeu démarre sous `picodrive`
   (écran Cores : `picodrive` « en cours d'utilisation »).
3. À essayer : **vidéo** (la 32X s'allume-t-elle ? la vidéo du jeu est-elle correcte ?), **son**, **changement de
   disque** (menu → **CD…** → disque 2), **sauvegarde** dans le jeu puis relance, **état** (sauvegarde et
   chargement d'état du menu).
4. Noter ce qui ne va pas : c'est le risque principal de la version (compatibilité réelle de picodrive).

## Partie 3 — Marquer à la main

1. Sélectionner un jeu Mega CD **sans** étiquette (un autre CD, ou renommer une copie sans l'étiquette puis
   rescanner) : la ligne d'aide du bas affiche **« Jeu Sega CD 32X ? Clic droit > Ce jeu est un CD 32X (ou touche
   X) »**. Elle n'apparaît que pour un jeu non marqué, dont l'identification ne repose pas sur le contenu (tout
   `.chd`).
2. **Clic droit** sur la vignette → **« Ce jeu est un CD 32X »**, ou la touche **X** (jeu sélectionné, grille
   active). Une fenêtre dit : le jeu sera lancé avec picodrive, ses sauvegardes `.brm` ne sont pas reprises, le
   nombre de jeux de même nom de fichier s'il y en a plusieurs, et de marquer aussi les autres disques listés à
   part. La pastille « 32X » apparaît ; l'invitation disparaît.
3. Relancer l'application : la pastille est toujours là (liste `cd32x` de la section `[library]` de `config.toml`,
   clé = nom du fichier en minuscules). **Renommer** le fichier perd la marque ; le **déplacer** la garde.
4. Clic droit → **« Ce n'est pas un CD 32X »** (ou X) retire la marque. Un jeu marqué par son **nom** n'a pas
   d'action : la pastille suffit.
5. Tout se fait au clavier : flèches, X, Entrée (**Échap** ferme la fenêtre).

## Titres non regroupés (une entrée par disque)

Pour ces titres, les noms du disque diffèrent trop pour former un seul jeu :

- **Slam City** (4 disques) : chaque entrée porte son étiquette (nom Redump) ; sinon marquer chaque disque.
- **Supreme Warrior (USA)** : idem. (L'édition Europe se regroupe.)
- **Fahrenheit** : disque 1 `(Key Disc) (Sega CD)` sans étiquette, disque 2 `(32X CD)` étiqueté. Pour partir du
  disque clé sous picodrive : **marquer à la main le disque 1**, ou écrire un `.m3u`. (L'ordre réel des disques est
  à valider par l'essai.)
- Fichiers `…CD1.chd` / `…CD2.chd` : une entrée par disque ; marquer chacun, ou renommer en `(Disc 1)` / `(Disc 2)`.

Les disques se suivent par le menu **CD…** ou par un **`.m3u`** que vous écrivez (un seul jeu, marqué si un disque
l'est). Les autres jeux concernés : Corpse Killer, Surgical Strike.

## À relever

- Le **nom exact** des fichiers et si la pastille apparaît d'elle-même.
- Le **rapport de scan** (nombre de jeux, doublons éventuels).
- Pour chaque étape ci-dessus : OK, ou le comportement observé (écran noir, image fausse, son, plantage), avec le
  journal de l'application si possible.

## Retour en arrière

La 7.2 ne migre rien : base et `config.toml` restent lisibles par la 7.1, la 7.0 et la 6.0. La clé `cd32x` est
simplement ignorée par les versions antérieures.
