# Glossaire — emu-rust

Termes propres à emu-rust employés dans nos échanges (validé le 2026-11-25). Les termes communs à tous les projets
perso sont dans `../agents-home/docs/glossaire.md`.

## Émulation

| Terme | Sens |
| --- | --- |
| Core | Un émulateur libretro existant (`.dll`, `.so`, `.dylib`) qu'emu-rust charge pour faire tourner un système (ex. genesis_plus_gx, picodrive, swanstation). emu-rust n'écrit jamais son propre émulateur. |
| Core recommandé / core utilisé | Le core proposé par défaut pour un système ; celui que tu as choisi dans « Core utilisé » (écran Cores). |
| Buildbot | Le serveur de libretro d'où emu-rust télécharge les cores et leurs fiches `.info`. |
| Fiche `.info` | La fiche d'un core sur le buildbot : nom, systèmes, extensions, BIOS attendus. |
| Système | Une machine que la bibliothèque connaît (Mega Drive, Amiga, PlayStation…), avec son dossier `roms/<système>/`. |
| Système officiel / personnalisé | Officiel : livré et essayé avec emu-rust (17 aujourd'hui). Personnalisé (8.0) : une machine que tu ajoutes toi-même depuis l'application, marquée « non testé ». |
| Bascule | Le passage d'une machine que tu as ajoutée vers le système officiel quand emu-rust la prend en charge ; toujours proposée, jamais automatique. |
| Soft | Le programme entier, jeu ou logiciel (ex. Night Trap, SimCity, System 6.0.8), quel que soit son nombre de supports. |
| ROM | Le contenu d'un support du soft, copié dans un fichier (`.md`, `.adf`, `.cue`…). Tu les fournis ; emu-rust n'en télécharge que du domaine public ou homebrew. |
| Support | Ce qui contenait la ROM à l'origine. Dans l'ordre chronologique : les cassettes, puis les disquettes et les cartouches, puis les CD et DVD. On désigne souvent une ROM par son support (« une ROM cartouche », « les ROMs disquette »). |
| BIOS | Le programme d'origine d'une machine, requis par certains cores (Mega CD, PlayStation). Tu le fournis, emu-rust vérifie son empreinte et ne le télécharge jamais, sauf BIOS libres (EmuTOS). |
| BIOS intégré (Builtin) | Un BIOS de remplacement déjà inclus dans le core (ex. HLE de la PlayStation) : utilisé si tu n'as pas le tien, avec un avertissement. |
| Fichier d'origine | Un fichier facultatif qui améliore un core (ex. `gba_bios.bin`, ROM Roland MT-32) ; sans lui le jeu marche quand même. |
| Alias de région | Une autre révision de BIOS PlayStation que tu importes, copiée sous le nom que le core attend pour sa région. |
| Core résident | Un core qui reste chargé en mémoire d'un jeu à l'autre (ADR 0022). |
| Rendu logiciel / matériel | Logiciel : le core dessine l'image lui-même (tout jusqu'à la 8.x). Matériel (9.0) : il dessine avec la carte graphique (OpenGL, puis DirectX, Vulkan), pour la 3D et la haute résolution. |

## Bibliothèque et rangement

| Terme | Sens |
| --- | --- |
| Bibliothèque | L'écran principal et sa base (`library.sqlite`) : tes jeux, favoris, historique, jaquettes. |
| Scan | La lecture de `roms/` qui ajoute, met à jour ou retire les jeux de la bibliothèque. |
| Identification | Reconnaître un jeu : par son contenu (empreinte comparée à un DAT), sinon par son nom de fichier. |
| DAT | Une liste de référence des jeux d'un système avec leurs empreintes (No-Intro, Redump, TOSEC, libretro-database). |
| Jaquette | L'image d'un jeu dans la bibliothèque, prise sur libretro-thumbnails (miroir GitHub d'abord). |
| Rangement | Le bouton « Ranger les ROMs… » (7.6) : déplace les jeux déposés en vrac vers `roms/<système>/`, après un aperçu et ton accord ; annulable. |
| Aperçu / rapport | Ce que le rangement compte faire (rien n'est déplacé) ; ce qu'il a fait (déplacés, laissés en place, erreurs), copiable. |
| Unité | Ce que le rangement déplace d'un bloc : un soft et tous ses fichiers (ex. un `.cue` et ses `.bin`). |
| Journal de rangement | Le fichier qui note chaque déplacement, pour annuler ou reprendre un rangement interrompu. |
| Journal (page) | La page « Journal » de l'application (7.5, ouverte depuis le pied de page) : les messages, avertissements et erreurs, filtrables et copiables. |
| Marque CD 32X | L'indication qu'un jeu Mega CD est un CD 32X : par le disque ou son nom (7.4), ou mise à la main (clic droit ou touche X, 7.5) ; impose picodrive. |
| Pied de page | La ligne du bas de chaque écran et du menu en jeu : version, « dev by claude@jerhome », bouton Journal. |

## Médias et lancement

| Terme | Sens |
| --- | --- |
| Média | Une des ROMs d'un soft à plusieurs supports (un disque, une disquette, une cassette parmi d'autres) ; ils sont regroupés en une seule vignette. |
| Jeu multi-disques | Un soft en plusieurs médias (ex. Night Trap, deux `.chd`), regroupé en une seule vignette. |
| `.m3u` | Une liste de lecture des disques d'un jeu, donnée au core. |
| Changement de disque (disk control) | Le passage au disque suivant pendant le jeu, par le menu en jeu ; pour certains cores, il remplace le `.m3u`. |
| `.cue` / `.bin` / `.chd` | Formats d'image de CD : une description et ses pistes ; ou un seul fichier compressé (reconnu par son nom). |
| Menu en jeu | Le menu ouvert pendant une partie (Ctrl droit ou bouton PS) : sauvegardes, disques, derniers messages. |
| Sauvegarde (SRAM / carte mémoire) | La sauvegarde faite par le jeu lui-même (`saves/<core>/<jeu>.srm`), propre à chaque core. |
| Save state (état sauvegardé) | Une photo complète de la partie à un instant, faite par emu-rust, rechargeable. |
| Input mode (mode d'entrée) | Comment tes touches ou ta manette sont données au core (Joystick, DualShock, clavier complet). |
| Profil de touches | Les touches réglées pour un core et un style de manette. |
| Dry run (essai à blanc) | Pour une machine ajoutée (8.0) : un lancement caché et court du jeu, avant le premier vrai lancement, pour vérifier que le core ne plante pas. |
| Haute résolution (HD) | Le rendu PlayStation agrandi (×2 à ×8) par swanstation en rendu matériel (9.0). |
| Mesurer 60 s | La mesure de fluidité du menu en jeu (9.0) : images par seconde et saccades, dans un rapport copiable. |
