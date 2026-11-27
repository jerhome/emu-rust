# Guide d'essai — version 8.0 (systèmes non testés)

Version : **8.0.0-rc.1** (build CI de `main`). Les libellés cités sont ceux du code
(`crates/emu-ui/src/library/text.rs`, `crates/emu-ui/src/cores_ui/text.rs`) ; si l'écran affiche autre chose, notez
le libellé réel. Ce guide couvre l'ajout, les Réglages et le lancement des systèmes non testés, puis la bascule
vers un système officiel (étapes 9 à 14) et les noms des boutons du core (étape 15).

Durée : environ 1 h. Windows d'abord. Notez pour chaque étape « OK » ou ce qui s'est passé (capture bienvenue).

## Avant de commencer

- **Fermez emu-rust, puis copiez tout le dossier de données** (base, `config.toml`, `systems/`, `rangement/`,
  jaquettes, sauvegardes) et le dossier `roms/` ailleurs. La 8.0 migre la base vers le schéma 8 : **une version 7.x
  refuse ensuite cette base**. Pour revenir à une 7.x, remettez la copie de la base faite ici ; si une bascule a
  été faite, mettez aussi de côté le journal de bascule du dossier `rangement/` (une 7.x refuse tout rangement tant
  qu'il y est).
- Gardez sous la main votre BIOS Lynx `lynxboot.img` (jamais téléchargé par emu-rust) et le jeu California Games.
- Vérifiez le build : `.\emu-rust-cli.exe --version` (Windows) ou `emu-rust --version` affiche `8.0.0-rc.1+<commit>`,
  et le pied de page de la bibliothèque affiche la même version (`emu-rust 8.0.0-rc.1+<commit>`).

## Étapes

1. **Premier système : Atari Lynx.** Écran Cores → « Vérifier les mises à jour » → « Ajouter un système non
   testé… ». Cherchez « Lynx » : les cores `handy` et `mednafen_lynx` sont proposés (essayez `handy` d'abord). Les
   cores grisés disent pourquoi (rendu matériel : prévu en 9.0, disques : prévu en 8.4, arcade : prévu en 8.6). Les
   machines déjà prises en charge (SNES, Mega Drive…) ne sont pas proposées. Tout au clavier : Tab, flèches, Entrée,
   Échap.
2. **La fiche** : machine cochée, licence, fichiers système annoncés (jamais téléchargés), avertissements. Un core
   absent de `cores.toml` demande « Accepter et continuer » (contenu non vérifié) ; « Annuler » ne laisse rien
   derrière soi. Acceptez : le core s'installe, puis « Lecture des réglages du core… », puis la boîte « Système
   ajouté » qui nomme `roms/<identifiant>/` (« Ouvrir le dossier » l'ouvre). Le dossier existe, un scan a tourné.
3. **Premier lancement** : déposez California Games dans ce dossier, scannez, lancez-le. L'avertissement « Système non
   testé » s'affiche ; « Essayer puis lancer » fait un essai de quelques secondes (annulable), puis le jeu démarre.
   Relancez le même jeu : aucun avertissement. Sans `lynxboot.img`, le bandeau d'un fichier d'origine manquant s'affiche
   et ne bloque pas le lancement ; importez le fichier avec « Importer un BIOS… » puis relancez.
4. **« Ça marche chez moi »** : sur un autre jeu, choisissez ce bouton : le jeu part, et plus aucun jeu de ce système
   n'avertit ni n'est essayé avec ce core (un autre core du même système avertit encore). L'acceptation du core, elle, n'est jamais sautée. Un jeu dans une archive `.zip`, en plusieurs
   fichiers ou de plus de 256 Mio n'est pas essayé : l'avertissement le dit et revient à chaque lancement tant que
   « Ça marche chez moi » n'a pas été choisi.
5. **Réglages → Systèmes non testés** : le système est listé. « Modifier » : nom, extensions, numéros ScreenScraper
   et IGDB, fichiers système à ne plus signaler, ordre des cores ; « Recalculer depuis le core » relit les extensions ;
   « Chercher la base d'identification » fait au plus trois requêtes (no-intro, tosec, `dat/`) et importe ce qu'elle
   trouve. Un système sans nom libretro n'en fait aucune.
6. **Supprimer** : « Supprimer » puis confirmez. Les jeux disparaissent de la grille (masqués, pas effacés) ; le
   dossier `roms/<identifiant>/` et les sauvegardes sont intacts. « Systèmes retirés » : « Restaurer » ramène le
   système et ses jeux avec leurs favoris. Supprimez de nouveau, puis « Oublier ses jeux » : les jeux sont retirés de
   la bibliothèque pour de bon (jamais les fichiers).
7. **Rangement** : « Ranger les ROMs… » avec un système retiré dont le dossier contient des jeux : l'aperçu dit
   « Dossier d'un système retiré ou en erreur : laissé tel quel » et n'y touche pas.
8. **Définition abîmée** : dans le dossier `systems/`, abîmez un fichier `.toml` (retirez un guillemet) : au redémarrage
   le système est listé sous « Systèmes en erreur » avec la raison, et ses jeux sont intacts. Réparez le fichier : tout
   revient.

9. **Bascule : préparer** (aucune version 8.0 ne propose de bascule réelle : on la provoque à la main). Fermez
   emu-rust, copiez le dossier de données. Dans `systems/`, copiez la définition de votre système en `snes.toml` en y
   mettant `id = "snes"`, la base `database = "Nintendo - Super Nintendo Entertainment System"`, les extensions
   `sfc`, `smc` et `xyz` (étape 10) et un core SNES installé autre que `snes9x` (par exemple `bsnes`, pour voir le
   core gardé) : tant qu'elle est active, cette définition prend la place du Super Nintendo officiel (cas **même
   dossier**). Relancez : un bandeau « emu-rust prend désormais en charge Super Nintendo officiellement.
   Basculer (recommandé) » s'affiche en bas de la bibliothèque, et la ligne du système dans les Réglages a un bouton
   « Basculer… ». « Fermer » cache le bandeau jusqu'au prochain démarrage (le bouton des Réglages reste).
10. **Cas particuliers à essayer avant d'accepter** : une ligne que le système officiel ne reconnaît pas (déposez un
    fichier d'une extension propre à votre définition, par exemple `.xyz`, scannez) ; un jeu multi-disquette tiré
    d'une archive `.zip` (Maniac Mansion C64) et un CD 32X (Night Trap) dans une bibliothèque qui a aussi des jeux
    du système ; vos touches personnalisées pour ce système (profil `<core>@<id>`) et un autre profil déjà présent
    pour le système officiel ; une sauvegarde d'état d'un jeu du système.
11. **Bascule en place** : « Basculer ». L'aperçu dit « aucun fichier n'est déplacé », « définitive », liste les jeux
    « que (système officiel) ne reconnaît pas : gardés, masqués » et, si vos touches diffèrent de celles du système
    officiel, demande « Garder vos touches » (défaut) ou « Garder celles du système officiel » ; « Plus tard »
    (qui a le focus) ne change rien. Acceptez : le système personnalisé disparaît des Réglages et apparaît dans
    « Systèmes retirés » sous « remplacé par… » (sans « Restaurer »). Les jeux, favoris, historique, jaquettes et
    sauvegardes sont intacts ; les lignes non reconnues sont masquées ; « Oublier ses jeux » (ligne « remplacé
    par… ») ne retire qu'elles. Vos touches fonctionnent pour le système officiel et votre core y reste le premier.
12. **Bascule avec déplacement** : même exercice avec une définition d'identifiant `mysnes` et la même base
    (cas **autre dossier**), des jeux dans `roms/mysnes/`. L'aperçu compte les jeux à déplacer et dit « Annulable tant
    que vous ne rangez rien d'autre ». Si un jeu est en conflit (même nom déjà dans `roms/snes/`), la bascule n'est
    pas proposée et l'aperçu liste ce qu'il faut régler. Acceptez : les jeux sont dans `roms/snes/` avec leurs
    favoris et leurs sauvegardes, le système personnalisé est retiré, un scan suit. Sans jeu à déplacer, la bascule
    est définitive, comme en place. Les fichiers qui ne sont pas des jeux restent dans `roms/mysnes/` (listés).
13. **Annuler la bascule** (juste après, sans autre rangement) : « Ranger les ROMs… » → « Annuler la bascule… » →
    confirmer. Les jeux reviennent dans `roms/mysnes/`, le système personnalisé est restauré (Réglages), vos touches
    et votre core d'avant sont rendus. Après un autre rangement, seul ce dernier rangement s'annule (la bascule est
    définitive). Si la définition a été recréée entre-temps, ou si son fichier retiré a disparu de
    `systems/.retired/`, l'annulation n'est pas proposée et l'aperçu dit pourquoi.
14. **Coupure puis reprise** : pendant une bascule avec déplacement de beaucoup de jeux, arrêtez emu-rust (fermez la
    fenêtre, ou tuez le processus). Relancez : la reprise se fait au démarrage, sans question ; si tous les jeux
    sont à destination, la bascule se termine (le système personnalisé est retiré, rien n'est affiché : le Journal le
    note) ; sinon le système personnalisé reste, le bandeau revient et « Basculer » reprend les jeux restants (si
    vous arrêtez la bascule par son bouton, le rapport dit « Bascule interrompue » ou « Bascule incomplète : N jeu(x)
    restent dans roms/mysnes/ »). Cas de la fin
    impossible (le fichier `systems/mysnes.toml` existe déjà à côté de son retiré) : un bandeau dit pourquoi et
    propose « Retirer ce système et terminer la bascule » ; rien n'est supprimé, le système retiré est listé
    « Systèmes retirés » et restaurable.
15. **Noms des boutons du core** : lancez un jeu d'un système non testé dont le core annonce ses boutons (essai « à
    blanc » réussi ou une partie) ; menu → touches : chaque ligne montre « A — Tir » quand le core l'a annoncé.
    Sans annonce, A, B, X, Y comme avant.

## Limites connues

- Un core qui demande le rendu matériel est refusé (« prévu en 9.0 »). Un core qui l'a demandé en option et dont le
  jeu se charge mal pour une autre raison (jeu abîmé) est lui aussi rapporté « prévu en 9.0 » au lieu de « échec du
  chargement » : on ne sait pas distinguer les deux causes.
- L'annulation d'un rangement ne remonte qu'au dernier rangement. Après une annulation partielle, les jeux qui n'ont
  pas pu revenir sont à déplacer à la main vers `roms/<identifiant du système officiel>/`.
- Une bascule a un seul cas annulable (avec déplacement) et seul le dernier rangement s'annule : tout rangement fait
  après la bascule la rend définitive. Les jaquettes renommées ne sont pas défaites par l'annulation (elles se
  retéléchargent sur action). Après une annulation partielle, les jeux revenus restent rattachés au système officiel
  (même machine) ; déplacez-les à la main vers `roms/<identifiant du système officiel>/`.
- L'annulation d'une bascule ne rend que le profil clavier du premier core du système : si vous aviez choisi
  « Garder vos touches », les attributions de la manette et les profils des autres cores du système officiel ne
  reviennent pas.
- Après « Oublier ses jeux » d'un système retiré par une bascule avec déplacement, l'annulation de la bascule
  restaure le système sans ses bases d'identification importées : relancez « Chercher la base d'identification ».
- Pour revenir à une 7.x après une bascule avec déplacement, mettez de côté son journal (`rangement/journal-*.txt`
  qui commence par `emu-rust tidy 2`) : une 7.x le refuse en entier.
- Si la fenêtre se ferme pendant un essai à blanc, l'essai peut continuer en arrière-plan jusqu'à 35 s, et son
  dossier (`cache/dry-run/`) reste jusqu'au démarrage suivant, qui le supprime.
- « Effacer toutes les jaquettes » laisse sur le disque les jaquettes des jeux masqués (système retiré) : seulement
  de la place perdue.
- L'anglais vient avec la 8.2 : tous les textes sont en français.

## Après l'essai

Pour revenir à l'état d'avant : fermez emu-rust et remettez les copies faites au début. Notez dans votre retour les
étapes en échec avec le libellé exact et, si possible, le contenu du Journal.
