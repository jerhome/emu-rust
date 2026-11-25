# Guide d'essai — version 7.5 (finition PC : terminal, Journal, fichiers d'origine, CD 32X)

Version : **v7.5.0** (guide réconcilié avec le code livré par L5, 2026-11-25). Les libellés d'interface cités ici
sont ceux des fichiers `text.rs` du code ; si l'écran affiche autre chose, notez le libellé réel.

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
- Les captures Xvfb du pied de page, du Journal et du bandeau (lot L5) ont été relues par l'agent qualité ; l'essai
  réel (fenêtre, presse-papiers, manette, son, relance de l'application, icône) est le vôtre.

## Partie 1 — Windows : terminal, Journal, copie, pied de page

1. Installez la 7.5.0 par-dessus votre version et lancez `emu-rust.exe`. Aucune fenêtre de console noire ne doit
   s'ouvrir. L'icône est celle de la 7.5 (voir la partie 4 pour la comparer à celle du Mac).
   Vos jeux, favoris et jaquettes sont retrouvés.
2. **Terminal** : Réglages → section « Application » → case « Afficher le terminal (après redémarrage) ». Cochez,
   quittez, **relancez à la main** : un terminal s'ouvre avec les messages (sous Windows, `emu-rust.exe` se relance
   lui-même en `emu-rust-cli.exe` : vérifiez que la relance ne laisse pas de fenêtre vide et que l'application
   démarre bien une seule fois). Décochez, relancez : le terminal a disparu. Dans `config.toml`, la clé est
   `[app] show_terminal`.
3. **`emu-rust-cli.exe`** (livré à côté) : lancé depuis un terminal Windows, `emu-rust-cli --version` répond **dans
   ce terminal** et rend la main. `emu-rust-cli --help` cite `emu-rust-cli.exe` (et non plus
   `emu-rust-console.exe`).
4. **Journal** : le **pied de page** de chaque page (bibliothèque, Réglages, Cores, Aide) affiche
   « emu-rust 7.5.0 — dev by claude@jerhome » à gauche et un bouton « Journal » à droite. Provoquez une erreur
   (lancez un jeu dont le core n'est pas installé et refusez l'installation) : le bouton affiche un compteur
   des messages non lus, par exemple « Journal (1 erreur) » (en rouge) ou « Journal (2 avertissements) » (en
   orange) ; le compteur repart à zéro après ouverture du Journal. Vérifiez que le compteur est exact (il ne
   compte ni les messages d'information ni deux fois le même message).
5. Ouvrez le Journal (bouton du pied de page, ou lien « Ouvrir le journal » dans Réglages, section
   « Application »). Échap revient à la page précédente ; PgUp / PgDn font défiler. Le volet Journal des Réglages
   de la 7.1 Mac n'existe plus. Relevez : les filtres « Erreurs », « Avertissements » et « Tout », les lignes des
   cores marquées comme telles, les boutons « Copier le journal » et « Ouvrir le dossier des journaux ».
6. **Copier** : collez le texte copié dans le Bloc-notes. Le texte d'un message d'erreur (bandeau) est aussi
   sélectionnable à la souris et copiable avec son bouton « Copier » (le libellé passe à « Copié »). C'est
   le point qui ne marchait pas sur la 7.4 (aucun texte dans le presse-papiers) : à vérifier **sur Windows et sur
   Mac**.
7. **Fichier du journal** : `logs/emu-rust.log` existe dans le dossier de données (et `emu-rust.log.1` après un
   second lancement). Lancez une **seconde instance** pendant que la première tourne : l'avertissement « fichier
   journal utilisé par une autre instance : cette session n'est pas enregistrée » apparaît dans le Journal de la
   seconde, et le fichier de la première n'est pas abîmé. Quittez la seconde, puis la première : le fichier est
   intact et lisible.
8. **En jeu** : lancez un jeu, ouvrez le menu en jeu (Ctrl droit ou bouton PS) : le pied de page est en bas du menu,
   son bouton « Journal » ouvre le panneau (jeu en pause) ; le menu liste aussi les « Derniers messages », copiables
   d'un seul « Copier ».
9. **Erreur fatale** (facultatif) : le message de plantage s'affiche dans une boîte Windows (`MessageBoxW`) et non
   dans une console qui se ferme aussitôt.

## Partie 2 — Fichiers d'origine, écran Cores, BIOS PlayStation

1. Écran **Cores** : pour `mgba`, `gambatte` et `dosbox_pure`, le détail du core affiche « Peut être amélioré
   par un fichier d'origine : » puis une ligne par fichier du type « gba_bios.bin (Game Boy Advance) — absent ·
   écran de démarrage… » (états : « présent et vérifié », « absent », « présent, md5 inattendu », « présent,
   non vérifiable »), et « emu-rust ne télécharge jamais ce fichier ». Les systèmes à core unique n'ont pas de
   menu de choix ; le champ « Recherche » filtre la liste.
2. **Bandeau du jeu sélectionné** (bibliothèque) : choisissez un jeu Game Boy, un jeu GBA sans `gba_bios.bin`, un
   jeu Amiga sans Kickstart, un jeu DOS (ROM MT-32 / CM-32L proposées) : le bandeau dit « Ce core fonctionne mieux avec un fichier d'origine que vous devez fournir vous-même (…) »,
   annonce le fichier absent (« gba_bios.bin : absent. … » avec son bénéfice) et propose « Importer… ». Un fichier
   au md5 inconnu s'affiche « présent, non vérifié (md5 inconnu) » ; depuis l'écran Cores, « Importer un BIOS… »
   le refuse avec « Fichier inconnu (md5 …) » sauf « Importer quand même (non vérifié)… ». Pas d'avertissement
   « une fois par core et fichier ».
   **Casse des ROM Roland** : après import de `MT32_CONTROL.ROM` et `MT32_PCM.ROM` (ou `CM32L_*`), les noms
   sous `system/` doivent être **en majuscules** (le système de fichiers de Linux et du Mac est sensible à la
   casse ; dosbox_pure les cherche ainsi). Si vos fichiers sont en minuscules, notez ce que fait l'import
   (renomme-t-il ? la ligne passe-t-elle à « présent » ?) et si la musique MT-32 se déclenche.
3. **PlayStation et 3DO** : sans BIOS, le bandeau dit que le fichier **améliore** (le core démarre avec son
   remplaçant), pas qu'il est « requis » (« fonctionne mieux avec ») ; pour le Mega CD il reste « exige ». (Question F2.)
   Note : `mednafen_psx_hw` n'a plus de bandeau de fichier requis (pas de `required_firmware` dans
   `cores.toml`) : dites si vous le regrettez.
4. **BIOS PlayStation** : importez une révision autre que `scph5502.bin` (SCPH-1001, 1002, 7002…) avec
   « Importer un BIOS… » (écran Cores) : elle est reconnue, et l'écran Cores indique sous quel nom de région elle sert
   (alias). Un BIOS d'une autre région que celle du jeu est signalé.
5. **Buildbot en mise à jour** : si le téléchargement d'un core échoue parce que le buildbot se met à jour, le
   message le dit (« le buildbot est en cours de mise à jour, réessayez plus tard ») au lieu d'une erreur de CRC. Non reproductible à
   volonté : notez-le si cela arrive.

## Partie 3 — CD 32X : Night Trap (Europe), `.chd`, marque manuelle

Fichiers : `Night Trap (Europe) (Disc 1) (Mega-CD 32X).chd` et `… (Disc 2) …`, sous `roms/megacd/`. BIOS Mega CD
Europe (`bios_CD_E.bin`) importé. Core : `picodrive`.

1. **Marque par le nom** : l'étiquette « Mega-CD 32X » entre parenthèses est reconnue. Après scan : un seul jeu
   « Night Trap » avec la pastille « 32X ».
2. **Étiquettes** : renommez une copie en `(Sega CD 32X)`, `(Mega CD 32X)`, `(32X CD)`, `(CD 32X)` : chacune est
   reconnue (casse et tiret indifférents). **Changement 7.5** : un `(32X)` seul entre parenthèses n'est **plus**
   reconnu ; il faut alors la marque manuelle.
3. **Marque manuelle** : renommez une copie en retirant toute étiquette, rescannez. La ligne d'aide invite :
   « Jeu Sega CD 32X ? Clic droit > Ce jeu est un CD 32X (ou touche X) ». Clic droit sur la vignette : **un seul
   menu** avec « Réinitialiser la jaquette » (touche R) et « Ce jeu est un CD 32X » (touche X). Après la marque,
   une fenêtre « CD 32X » dit : « … sera lancé avec le core picodrive, et non plus avec celui du Mega CD. Ses
   sauvegardes faites avec genesis_plus_gx (.brm) ne sont pas reprises. » (plus, s'il y en a, « N jeux Mega CD
   portent ce nom de fichier : tous seront lancés avec picodrive » et « Si le jeu a d'autres disques listés à
   part, marquez-les aussi »). Puis : pastille « 32X », invitation disparue.
4. Relancez l'application : la pastille est toujours là (`[library] cd32x` de `config.toml`, nom de fichier en
   minuscules). Le menu propose alors « Ce n'est pas un CD 32X » (touche X) ; la marque se retire avec le message
   « … n'est plus marqué CD 32X : il sera lancé avec le core habituel du Mega CD ». Un jeu marqué par son nom
   ou par la sonde n'a pas cette entrée.
5. **Lancer** : Entrée sur Night Trap. Sans picodrive : le dialogue dit « … est un CD Mega CD 32X : seul le core
   « picodrive » émule la 32X (genesis_plus_gx ne lance que le Mega CD). Installer « picodrive » ? » ; sur refus,
   le jeu ne se lance **pas** (jamais de repli sur `genesis_plus_gx`). Avec picodrive : le jeu démarre. Relevez
   vidéo, son, changement de disque (menu en jeu → entrée des disques, nommée « Disquettes… » pour les
   ordinateurs : relevez son libellé pour un CD → disque 2), sauvegarde dans le jeu puis relance, état du menu.
   Si votre core préféré du Mega CD est `genesis_plus_gx`, le Journal doit noter « le core préféré … n'émule
   pas la 32X, picodrive est utilisé à sa place ».
6. **Écriture impossible** (facultatif) : mettez `config.toml` en lecture seule et marquez un jeu : la marque est
   annulée et l'échec affiché (« Marque CD 32X de … annulée, réglages non enregistrés : … »).
7. **Notice 32X** (écran Cores, détail de picodrive, section BIOS) : « Démarrage 32X assuré par le core picodrive
   (aucun BIOS Sega) » et la notice qui explique que picodrive n'intègre aucune image du BIOS 32X de Sega. Dans la
   bibliothèque, la note du système : « CD Mega CD 32X : lancé avec le core picodrive (BIOS Mega CD requis, BIOS
   32X intégrés au core) ».

## Partie 4 — Manette SNES, MANIACD, icône

1. **Manette SNES** (non touchée par la 7.5 : à vérifier qu'elle l'est bien) : avec votre manette, lancez un jeu SNES.
   Boutons, croix et gâchettes répondent comme en 7.2 ; la configuration des touches est inchangée.
2. **MANIACD** : placez `MANIACD0.D64` et `MANIACD1.D64` dans `roms/c64/` : **une seule tuile** « MANIACD »
   (disque 0 et 1 groupés). Même chose pour une paire à chiffre final sur Atari ST (`JEUD0`, `JEUD1`), Amiga ou CPC :
   regroupée (règle stricte : nom d'au moins 4 caractères, lettre `D` ou mot « disk / side », même dossier).
   (Question F4.)
3. **Icône** : celle de la 7.5 (dessin Mac, manette bleue `#4aa3ff`) dans la barre des tâches, la barre de titre,
   Alt+Tab et le `.exe` dans l'Explorateur (taille 16 à 256 px : nette, sans fond noir). Sous macOS : Dock et
   Finder (partie 5).

## Partie 5 — macOS Intel

1. Lancez la `.app` : l'**icône** s'affiche dans le Dock et le Finder (dessin 7.5).
2. **Terminal** : Réglages → section « Application » → « Afficher le terminal (après redémarrage) », relancez :
   l'application se relance dans **Terminal.app** (à vérifier : une seule fenêtre de l'application, pas de
   boucle de relance, le Terminal ne reste pas ouvert après la sortie) ; décochez : retour à la normale (double
   clic dans le Finder sans Terminal).
3. **Copier** : copiez le Journal et un message d'erreur ; collez dans TextEdit.
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

Pour chaque partie : « OK » ou le symptôme, avec le texte copié du message ou du Journal (bouton « Copier » ou « Copier le journal ») et
une capture si l'affichage est en cause. Relevez surtout les **libellés réels** qui diffèrent de ceux de ce guide.

## Retour en arrière

La 7.5 ne change pas le schéma de `library.sqlite` (v7). La clé `[library] cd32x` est ignorée par les versions
antérieures ; `show_terminal` remplace `console`.
