# Guide d'essai — version 7.5 (finition PC : terminal, Journal, fichiers d'origine, CD 32X)

Version : **v7.5.0** (brouillon de préparation du lot L6, 2026-11-25). Les libellés d'interface cités ici viennent
du contrat `docs/design/v7.5-finition-pc.md` et des guides 7.1 et 7.2 : chacun est marqué
**[À VÉRIFIER APRÈS L5]** et doit être confronté à l'interface réellement livrée par L5 (texte exact, ordre, présence).

Builds : <https://github.com/jerhome/emu-rust/releases> (version 7.5.0, quand elle sera publiée).

Durée : environ 1 h 15. But : vérifier la finition de l'interface reprise de la ligne PC (terminal, Journal, copie,
pied de page), les fichiers d'origine (écran Cores, bandeau, BIOS PlayStation), le **CD 32X** (Night Trap Europe en
`.chd` avec marque manuelle), la manette SNES et les disquettes `MANIACD`, puis macOS et la non-régression. Notez
pour chaque étape « OK » ou ce qui s'est passé (capture d'écran bienvenue).

Hors de cet essai : le **rangement de `roms/`** (version 7.6), le glisser-déposer, les archives `.7z` et
l'anglais.

Rappel : emu-rust ne télécharge **aucun** jeu ni **aucun** BIOS commercial. Aucun BIOS Sega n'est requis pour la
32X ; le BIOS Mega CD vient de **vos** fichiers.

## À savoir avant de commencer

- Faites une copie de votre `config.toml` (dossier de données de l'application) : l'essai de la partie 1 le
  modifie (réglage du terminal, marque 32X).
- Si votre `config.toml` vient de la 7.1.0-rc.1 PC (section `[interface]` avec `console = true`), il est lu :
  voir la question F3.
- Les captures Xvfb du pied de page, du Journal et du bandeau sont prises par l'agent qualité (L6) ; l'essai réel
  (fenêtre, presse-papiers, manette, son) est le vôtre.

## Partie 1 — Windows : terminal, Journal, copie, pied de page

1. Installez la 7.5.0 par-dessus votre version et lancez `emu-rust.exe`. Aucune fenêtre de console noire ne doit
   s'ouvrir. L'icône est celle de la 7.5 (voir la partie 4 pour la comparer à celle du Mac).
   Vos jeux, favoris et jaquettes sont retrouvés.
2. **Terminal** : Réglages → case « Afficher le terminal » (nom exact à relever) [À VÉRIFIER APRÈS L5]. Cochez, quittez, relancez :
   un terminal s'ouvre avec les messages. Décochez, relancez : il a disparu.
3. **`emu-rust-cli.exe`** (livré à côté) : lancé depuis un terminal Windows, `emu-rust-cli --version` répond **dans
   ce terminal** et rend la main. `emu-rust-cli --help` cite `emu-rust-cli.exe` (et non plus
   `emu-rust-console.exe`).
4. **Journal** : le **pied de page** de chaque page (bibliothèque, Réglages, Cores, Aide) affiche
   « emu-rust 7.5.0 — dev by claude@jerhome » et un bouton « Journal » [À VÉRIFIER APRÈS L5]. Provoquez une erreur (lancez un jeu
   dont le core n'est pas installé et refusez l'installation) : le bouton affiche un compteur d'erreurs, par
   exemple « Journal (1 erreur) » [À VÉRIFIER APRÈS L5].
5. Ouvrez le Journal (bouton du pied de page, ou lien « Ouvrir le journal » dans Réglages [À VÉRIFIER APRÈS L5]). Échap revient à
   la page précédente. Le volet Journal des Réglages de la 7.1 Mac n'existe plus. Relevez : lignes des cores
   marquées comme telles, filtres, bouton « Copier » [À VÉRIFIER APRÈS L5].
6. **Copier** : collez le texte copié dans le Bloc-notes. Le texte d'un message d'erreur (bandeau) est aussi
   sélectionnable à la souris et copiable avec « Copier » [À VÉRIFIER APRÈS L5].
7. **Fichier du journal** : `logs/emu-rust.log` existe dans le dossier de données (et `emu-rust.log.1` après un
   second lancement). Lancez une **seconde instance** : un avertissement indique qu'elle n'écrit pas de fichier.
8. **En jeu** : lancez un jeu, ouvrez le menu en jeu (Ctrl droit ou bouton PS) : le pied de page est en bas du menu,
   son bouton « Journal » ouvre le panneau (jeu en pause) avec « Derniers messages » copiables [À VÉRIFIER APRÈS L5].
9. **Erreur fatale** (facultatif) : le message de plantage s'affiche dans une boîte Windows (`MessageBoxW`) et non
   dans une console qui se ferme aussitôt.

## Partie 2 — Fichiers d'origine, écran Cores, BIOS PlayStation

1. Écran **Cores** : pour `mgba`, `gambatte` et `dosbox_pure`, une ligne « Peut être amélioré par un fichier
   d'origine » indique le fichier facultatif et s'il est présent [À VÉRIFIER APRÈS L5]. Les systèmes à core unique n'ont pas de
   menu de choix ; la recherche filtre la liste [À VÉRIFIER APRÈS L5].
2. **Bandeau du jeu sélectionné** (bibliothèque) : choisissez un jeu Game Boy, un jeu GBA sans `gba_bios.bin`, un
   jeu Amiga sans Kickstart, un jeu DOS (ROM MT-32 / CM-32L proposées) : le bandeau annonce le fichier absent, son
   bénéfice, et propose « Importer… » [À VÉRIFIER APRÈS L5]. Un fichier au md5 inconnu s'affiche « non vérifié » et se prend avec
   « Importer quand même (non vérifié) » [À VÉRIFIER APRÈS L5]. Pas d'avertissement « une fois par core et fichier ».
3. **PlayStation et 3DO** : sans BIOS, le bandeau dit que le fichier **améliore** (le core démarre avec son
   remplaçant), pas qu'il est « requis » ; pour le Mega CD il reste « requis » [À VÉRIFIER APRÈS L5]. (Question F2.)
4. **BIOS PlayStation** : importez une révision autre que `scph5502.bin` (SCPH-1001, 1002, 7002…) avec
   « Importer un BIOS… » [À VÉRIFIER APRÈS L5] : elle est reconnue, et l'écran Cores indique sous quel nom de région elle sert
   (alias). Un BIOS d'une autre région que celle du jeu est signalé.
5. **Buildbot en mise à jour** : si le téléchargement d'un core échoue parce que le buildbot se met à jour, le
   message le dit (« buildbot en cours de mise à jour » [À VÉRIFIER APRÈS L5]) au lieu d'une erreur de CRC. Non reproductible à
   volonté : notez-le si cela arrive.

## Partie 3 — CD 32X : Night Trap (Europe), `.chd`, marque manuelle

Fichiers : `Night Trap (Europe) (Disc 1) (Mega-CD 32X).chd` et `… (Disc 2) …`, sous `roms/megacd/`. BIOS Mega CD
Europe (`bios_CD_E.bin`) importé. Core : `picodrive`.

1. **Marque par le nom** : l'étiquette « Mega-CD 32X » entre parenthèses est reconnue. Après scan : un seul jeu
   « Night Trap » avec la pastille « 32X » [À VÉRIFIER APRÈS L5].
2. **Étiquettes** : renommez une copie en `(Sega CD 32X)`, `(Mega CD 32X)`, `(32X CD)`, `(CD 32X)` : chacune est
   reconnue (casse et tiret indifférents). **Changement 7.5** : un `(32X)` seul entre parenthèses n'est **plus**
   reconnu ; il faut alors la marque manuelle.
3. **Marque manuelle** : renommez une copie en retirant toute étiquette, rescannez. La ligne d'aide invite :
   « Jeu Sega CD 32X ? Clic droit ou X → Ce jeu est un CD 32X » [À VÉRIFIER APRÈS L5]. Clic droit sur la vignette : **un seul menu**
   avec « Réinitialiser la jaquette » (touche R) et « Ce jeu est un CD 32X » (touche X) [À VÉRIFIER APRÈS L5]. Après la marque :
   fenêtre d'information (picodrive, `.brm` non repris, nombre de jeux de même nom de fichier) [À VÉRIFIER APRÈS L5], pastille
   « 32X », invitation disparue.
4. Relancez l'application : la pastille est toujours là (`[library] cd32x` de `config.toml`, nom de fichier en
   minuscules). Le menu propose alors « Ce n'est pas un CD 32X » [À VÉRIFIER APRÈS L5] (touche X) et la marque se retire. Un jeu
   marqué par son nom ou par la sonde n'a pas cette entrée.
5. **Lancer** : Entrée sur Night Trap. Sans picodrive : le dialogue propose de l'installer avec la raison
   « picodrive requis » [À VÉRIFIER APRÈS L5] ; sur refus, le jeu ne se lance **pas** (jamais de repli sur `genesis_plus_gx`).
   Avec picodrive : le jeu démarre. Relevez vidéo, son, changement de disque (menu → CD… [À VÉRIFIER APRÈS L5] → disque 2), sauvegarde
   dans le jeu puis relance, état du menu.
6. **Écriture impossible** (facultatif) : mettez `config.toml` en lecture seule et marquez un jeu : la marque est
   annulée et l'échec affiché [À VÉRIFIER APRÈS L5].
7. **Notice 32X** (écran Cores ou aide du système 32X) : « aucun BIOS Sega » [À VÉRIFIER APRÈS L5].

## Partie 4 — Manette SNES, MANIACD, icône

1. **Manette SNES** (non touchée par la 7.5 : à vérifier qu'elle l'est bien) : avec votre manette, lancez un jeu SNES.
   Boutons, croix et gâchettes répondent comme en 7.2 ; la configuration des touches est inchangée.
2. **MANIACD** : placez `MANIACD0.D64` et `MANIACD1.D64` dans `roms/c64/` : **une seule tuile** « MANIACD »
   (disque 0 et 1 groupés). Même chose pour une paire à chiffre final sur Atari ST (`JEUD0`, `JEUD1`), Amiga ou CPC :
   regroupée (règle stricte : nom d'au moins 4 caractères, lettre `D` ou mot « disk / side », même dossier).
   (Question F4.)
3. **Icône** : celle de la 7.5 (dessin Mac, manette bleue) dans la barre des tâches, la fenêtre et le `.exe`.

## Partie 5 — macOS Intel

1. Lancez la `.app` : l'**icône** s'affiche dans le Dock et le Finder (dessin 7.5).
2. **Terminal** : Réglages → « Afficher le terminal » [À VÉRIFIER APRÈS L5], relancez : l'application se relance dans le Terminal ;
   décochez : retour à la normale.
3. **Copier** : copiez le Journal et un message d'erreur ; collez dans TextEdit [À VÉRIFIER APRÈS L5].
4. Pied de page et Journal comme en partie 1 (étapes 4 à 8).
5. Night Trap 32X (partie 3) si le temps le permet.

## Partie 6 — Non-régression

Rejouez ce qui marchait en 7.4 :

| Jeu | Système / core | Attendu |
|---|---|---|
| Night Trap (2 `.chd`) | Mega CD 32X, `picodrive` | pastille, démarrage, changement de disque |
| Un jeu SNES | `snes9x` | manette et son corrects |
| GTA | PlayStation, `pcsx_rearmed` | démarre, carte mémoire retrouvée |
| Road Rash | 3DO, `opera` | démarre |
| B.A.T. (zip) | Amstrad CPC, `cap32` | la disquette est lue |
| Un jeu Mega Drive | `genesis_plus_gx` | rien de changé |

Aller-retour de `config.toml` : après la partie 3, le fichier contient `[library] cd32x` ; si vous partiez d'une
7.1.0-rc.1 (`[interface] console = true`), il contient `[app] show_terminal = true` et plus la clé `console`.

## Questions à confirmer à l'essai (F1 à F4)

Réponses par défaut déjà appliquées (« ok pour les défauts » suffit) :

| # | Question | Défaut appliqué | Où le constater |
|---|---|---|---|
| F1 | PC Engine CD-ROM² en 8.0 ou plus tard | 8.0 = HuCard seulement, CD-ROM² en 8.2 | roadmap (pas d'essai) |
| F2 | Bandeau PlayStation et 3DO : « requis » ou « améliore » | « améliore » ; « requis » reste pour le Mega CD | partie 2, étape 3 |
| F3 | `[interface] console = true` converti en `[app] show_terminal = true`, puis retiré du fichier | oui (alias à sens unique) | partie 1 étape 2, partie 6 |
| F4 | Disquettes à chiffre final regroupées pour tous les ordinateurs | tous, règle stricte | partie 4, étape 2 |

## Ce qu'il faut rapporter

Pour chaque partie : « OK » ou le symptôme, avec le texte copié du message ou du Journal (bouton « Copier » [À VÉRIFIER APRÈS L5]) et
une capture si l'affichage est en cause. Relevez surtout les **libellés réels** qui diffèrent de ceux de ce guide.

## Retour en arrière

La 7.5 ne change pas le schéma de `library.sqlite` (v7). La clé `[library] cd32x` est ignorée par les versions
antérieures ; `show_terminal` remplace `console`.
