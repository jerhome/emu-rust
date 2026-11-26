# Guide d'essai — version 7.6 (rangement des ROMs)

Version : **v7.6.0**. Les libellés cités sont ceux du code (`crates/emu-ui/src/library/tidy/text.rs`) ; si l'écran
affiche autre chose, notez le libellé réel.

Durée : environ 45 min. Windows d'abord. Sous PowerShell, la ligne de commande se lance par `.\emu-rust-cli.exe`.
Notez pour chaque étape « OK » ou ce qui s'est passé (capture bienvenue).

Hors de cet essai : le glisser-déposer, les archives `.7z`, l'anglais.

## Avant de commencer

- **Fermez emu-rust, puis copiez tout le dossier de données** (base, `config.toml`, `rangement/`, jaquettes,
  sauvegardes) et le dossier `roms/` ailleurs. Tout l'essai se fait sur la vraie bibliothèque, mais vous pouvez la
  restaurer.
- Mettez en vrac dans `roms/` quelques jeux de systèmes différents, dont un jeu à favori avec une jaquette et une
  sauvegarde d'état, et un jeu CD 32X marqué à la main si vous en avez un.

## Étapes

1. **Créer les dossiers** : Réglages → « Créer les dossiers des systèmes ». Des sous-dossiers vides apparaissent dans
   `roms/` ; aucun fichier n'est déplacé. Refaites : rien ne change, aucune erreur.
2. **Aperçu** : Réglages → « Ranger les ROMs… ». Une analyse tourne, puis la page « Ranger les ROMs » liste les jeux :
   sûrs cochés, suggestions avec leur raison, jeux laissés en place avec la raison. Changez le système d'un jeu
   ambigu. Cliquez « Fermer sans ranger » : **aucun fichier n'a bougé**.
3. **Ranger** : rouvrez l'aperçu, puis « Ranger N jeu(x) ». Une progression « Rangement… x / N » s'affiche, puis le
   rapport. Vérifiez dans l'explorateur que les fichiers sont dans `roms/<système>/`, sans renommage. Dans la
   bibliothèque : favori, jaquette, historique, marque 32X et sauvegarde d'état du jeu à favori sont **intacts**.
   « Copier le rapport » colle un texte lisible ; les problèmes éventuels sont dans le Journal.
4. **Annuler** : Réglages → « Annuler le dernier rangement… », confirmez par « Oui, annuler ce rangement » (testez
   aussi « Non, garder le rangement »). Les jeux reviennent à leur place d'origine, favoris et jaquettes toujours
   attachés. Lancez un jeu pour le vérifier. Refaites le rangement ensuite pour l'étape 5.
5. **Arrêt brutal pendant un rangement** (facultatif) : lancez un rangement de nombreux jeux et tuez l'application
   (Gestionnaire des tâches → Fin de tâche) en cours de route. Relancez : un message dit que le rangement précédent a
   été interrompu et repris ; la bibliothèque est cohérente (aucun jeu perdu ni en double).
6. **Jeu déplacé à la main** : déplacez un jeu à favori d'un dossier à un autre avec l'explorateur, puis
   « Scanner ». Le jeu est retrouvé (même fichier) : favori, jaquette, historique et marque 32X sont conservés, sans
   doublon. Lancez un scan, puis **annulez-le** en cours : aucun favori n'est perdu.
7. **Limite connue (deux scans)** : lancez un scan sur une grosse bibliothèque et tuez l'application en cours de scan.
   Relancez, supprimez un jeu du disque (pas de la bibliothèque), puis « Scanner » : le message « Le scan précédent
   avait été interrompu : les jeux dont le fichier est absent sont conservés jusqu'au prochain scan. » s'affiche et le
   jeu supprimé **reste** listé. Scannez une seconde fois : le jeu disparaît. C'est attendu en 7.6.

## Après l'essai

Pour revenir à l'état d'avant : fermez emu-rust et remettez les copies faites au début. Notez dans votre retour les
étapes en échec avec le libellé exact et, si possible, le contenu du Journal.
