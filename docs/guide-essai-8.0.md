# Guide d'essai — version 8.0 (systèmes non testés)

Version : **8.0.0** (build CI de `main`). Les libellés cités sont ceux du code
(`crates/emu-ui/src/library/text.rs`, `crates/emu-ui/src/cores_ui/text.rs`) ; si l'écran affiche autre chose, notez
le libellé réel. Ce guide couvre l'ajout, les Réglages et le lancement des systèmes non testés ; les étapes de la
bascule vers un système officiel (bandeau, rangement) viennent avec le lot PU4.

Durée : environ 1 h. Windows d'abord. Notez pour chaque étape « OK » ou ce qui s'est passé (capture bienvenue).

## Avant de commencer

- **Fermez emu-rust, puis copiez tout le dossier de données** (base, `config.toml`, `systems/`, `rangement/`,
  jaquettes, sauvegardes) et le dossier `roms/` ailleurs. La 8.0 migre la base vers le schéma 8 : **une version 7.x
  refuse ensuite cette base**. Pour revenir à une 7.x, remettez la copie de la base faite ici ; si une bascule a
  été faite, mettez aussi de côté le journal de bascule du dossier `rangement/` (une 7.x refuse tout rangement tant
  qu'il y est).
- Gardez sous la main votre BIOS Lynx `lynxboot.img` (jamais téléchargé par emu-rust) et le jeu California Games.

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

## Limites connues

- Un core qui demande le rendu matériel est refusé (« prévu en 9.0 »). Un core qui l'a demandé en option et dont le
  jeu se charge mal pour une autre raison (jeu abîmé) est lui aussi rapporté « prévu en 9.0 » au lieu de « échec du
  chargement » : on ne sait pas distinguer les deux causes.
- L'annulation d'un rangement ne remonte qu'au dernier rangement. Après une annulation partielle, les jeux qui n'ont
  pas pu revenir sont à déplacer à la main vers `roms/<identifiant du système officiel>/`.
- Si la fenêtre se ferme pendant un essai à blanc, l'essai peut continuer en arrière-plan jusqu'à 35 s, et son
  dossier (`cache/dry-run/`) reste jusqu'au démarrage suivant, qui le supprime.
- « Effacer toutes les jaquettes » laisse sur le disque les jaquettes des jeux masqués (système retiré) : seulement
  de la place perdue.
- L'anglais vient avec la 8.2 : tous les textes sont en français.

## Après l'essai

Pour revenir à l'état d'avant : fermez emu-rust et remettez les copies faites au début. Notez dans votre retour les
étapes en échec avec le libellé exact et, si possible, le contenu du Journal.
