# Guide d'essai — version 6.0 (Game Boy Advance, 32X, Mega CD)

Version : **v6.0-rc.1** (2026-11-21).

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v6.0.0-rc.1`).

Durée : environ 1 heure. But : vérifier un jeu GBA, un jeu 32X et un jeu Mega CD au clavier 60 % AZERTY (Windows),
sur macOS Intel (où sont vos BIOS Mega CD) et dans Docker (WSLg), puis à la manette DS4. Notez pour chaque étape
« OK » ou ce qui s'est passé (capture d'écran bienvenue).

Rappel : emu-rust ne télécharge **aucun** jeu ni **aucun** BIOS commercial. Le BIOS Mega CD vient de **vos**
fichiers (`bios_CD_E.bin`, `bios_CD_U.bin`, `bios_CD_J.bin`) ; le GBA n'en a pas besoin (BIOS de remplacement
intégré à mgba, `gba_bios.bin` facultatif) ; le 32X non plus (BIOS intégrés à picodrive). Seul le DAT Redump du
Mega CD (liste de jeux, sans contenu) est téléchargé, avec les autres DAT, par « Télécharger les bases ».

## À savoir avant de commencer

- **Cores** : `mgba` (GBA ; `gpsp` et `vbam` en alternative si le Mac est lent), `picodrive` (32X),
  `genesis_plus_gx` (Mega CD ; `picodrive` en alternative). Écran Cores → « Core utilisé » pour changer.
- **Rangement** : `roms/gba/`, `roms/32x/`, `roms/megacd/`. Pour le Mega CD, **un dossier par jeu** de préférence ;
  le `.cue` et **tous** ses fichiers de pistes (`.bin`, `.wav`…) côte à côte, avec leurs noms d'origine.
- **CHD conseillé** pour le Mega CD : un seul fichier par disque, plus petit, musique CD comprise
  (`chdman createcd -i jeu.cue -o jeu.chd`). Un `.chd` doit être rangé sous `roms/megacd/` (son contenu n'est pas lu
  pour deviner le système) ; son nom donne la région : gardez l'étiquette `(Europe)`, `(USA)` ou `(Japan)`.
- **Zip** : les jeux GBA et 32X peuvent rester zippés ; **les CD jamais** (message « Les CD ne se lisent pas dans un
  zip : décompressez-le ou convertissez-le en CHD. »).
- **Vos fichiers ne sont jamais modifiés** : sauvegardes GBA dans `saves/mgba/<jeu>.srm`, 32X dans
  `saves/picodrive/`, Mega CD dans `saves/genesis_plus_gx/<jeu>.brm` (RAM interne, par jeu) et `cart.brm`
  (cartouche RAM, partagée, jamais effacée).

## Installer les cores

Écran **Cores** → section des cores recommandés → **installer** `mgba`, `picodrive` et `genesis_plus_gx`. Ou en
ligne de commande : `emu-rust --install-core recommended`. Les cores ne sont jamais fournis avec l'application.

## Limites connues

- Le suffixe `(CD N)` n'est pas reconnu pour regrouper les disques : utiliser `(Disc N)` ou un `.m3u`.
- La région d'un `.chd` rangé hors de `roms/megacd/` n'est pas lue (le BIOS est alors choisi par le core).
- Une bibliothèque ouverte par la 6.0 n'est plus lisible par une 5.x : gardez une copie si vous voulez revenir.

## Partie 1 — Game Boy Advance

| GBA | Touche |
|---|---|
| Croix | Flèches ou ZQSD |
| A / B | K / J |
| L / R | O / L |
| Start / Select | Entrée / Maj droite |

Manette DS4 : Croix = B, Rond = A, L1 / R1 = L / R (Carré et Triangle sans effet).

1. **Cores** → installer `mgba` (BIOS facultatif : rien n'est demandé au lancement).
2. Copier un jeu `.gba` (ou `.zip`) dans `roms/gba/`, **Scanner** : il apparaît sous « Game Boy Advance ».
3. Lancer : le jeu démarre **sans BIOS** (pas de logo Nintendo, c'est normal). Avec `gba_bios.bin` importé
   (« Importer un BIOS… ») et l'option `mgba_use_bios` des Réglages GBA, le logo apparaît.
4. Menu (Ctrl droit) → **Réglages GBA** en tête des options du core (correction des couleurs, mélange d'images…).
5. **Sauvegarde** : sauvegarder dans le jeu, quitter, relancer : la partie est là (`saves/mgba/<jeu>.srm`).
6. Sauvegarde d'état (menu → Sauvegarder l'état) puis chargement.

## Partie 2 — 32X

Touches : celles de la Mega Drive (profil picodrive).

1. **Cores** → installer `picodrive`. Ligne BIOS : « intégré au core » et la notice.
2. Copier un jeu `.32x` (ou `.zip`) dans `roms/32x/`, **Scanner** : il est classé « 32X » (un `.bin` 32X est aussi
   reconnu par son en-tête).
3. Lancer, jouer une minute, vérifier le son. Menu → **Réglages 32X** en tête des options.
4. Un jeu **Mega Drive** lancé avec picodrive ou genesis_plus_gx : **aucune** demande de BIOS Mega CD, pas de section
   « Mega CD » dans les options (seulement pour les jeux Mega CD).

## Partie 3 — Mega CD (BIOS par région)

La région du jeu est lue dans l'en-tête du disque (comme le fait le core), sinon dans la base Redump, sinon dans
l'étiquette du nom. Le BIOS voulu est celui de cette région.

1. **Cores** → installer `genesis_plus_gx`. Sa ligne BIOS signale `bios_CD_U.bin` absent tant que rien n'est
   importé (avec « Importer un BIOS… ») ; `emu-rust --check-bios genesis_plus_gx` liste les trois régions.
2. Copier un jeu (`.cue` + pistes, ou `.chd`) sous `roms/megacd/`, **Scanner**.
3. **Sans BIOS**, lancer le jeu : dialogue « BIOS manquant : bios_CD_E.bin (Mega CD / Sega CD, Europe) » avec le
   **nom exact**, la région, le **chemin du dossier `system/`** (sélectionnable) et les boutons **Importer un
   BIOS…**, **Réessayer** (focus par défaut), **Copier le chemin**, **Annuler**, **Lancer quand même** (jamais par
   défaut). Tout se fait au clavier (Tab, Entrée, Échap) et à la manette.
4. Si un BIOS d'une **autre région** est présent, le dialogue le dit (« bios_CD_U.bin est présent, mais ce jeu est
   Europe ») sans proposer de forcer la région.
5. **Importer un BIOS…** : choisir votre fichier ; il est vérifié (empreinte) et copié sous le nom attendu ; le jeu
   démarre. Variante : copier le fichier à la main dans le dossier affiché, puis **Réessayer**.
6. BIOS d'une autre révision (empreinte inconnue) : avertissement non bloquant, « Lancer quand même » démarre.
7. **`.chd` sans étiquette de région** : il démarre dès qu'**un** BIOS Mega CD est présent (le core choisit).
8. Si le core refuse quand même le disque, le message affiche **l'erreur du core** (par exemple « Unable to open
   CD BIOS: …/bios_CD_U.bin ») au lieu d'un conseil générique.
9. **Musique CD** : un `.cue` avec pistes audio (ou un `.chd`) joue la musique ; vérifier le son.
10. **Sauvegarde interne** : sauvegarder dans le jeu, quitter, relancer : la sauvegarde est là
    (`saves/genesis_plus_gx/<jeu>.brm`). La RAM de sauvegarde est par jeu (option `genesis_plus_gx_system_bram`,
    Réglages Mega CD).
11. **Jeu à plusieurs CD** (si vous en avez un) : fichiers `Jeu (Disc 1).cue`, `Jeu (Disc 2).cue` (ou un `.m3u`)
    dans le même dossier : **un seul** jeu dans la bibliothèque. Au changement de disque demandé par le jeu : menu
    → **CD…** → « Éjecter » puis le CD 2. Les fichiers ne sont jamais copiés.

## Ligne de commande

- `emu-rust --core mgba --rom "Jeu.gba"`
- `emu-rust --core genesis_plus_gx --rom "Jeu (Europe).cue"` : le système est déduit de l'extension (Mega CD) ;
  sans BIOS, l'erreur du core est affichée.
- `emu-rust --check-bios genesis_plus_gx` : une ligne par région ; code de sortie 1 si aucun BIOS Mega CD n'est
  utilisable.
- `emu-rust --list-bios` : « fichiers attendus par région » pour le Mega CD.

## Docker (WSLg)

1. Lancer l'image comme pour les versions précédentes (`docker/README.md`). Les données sont dans le volume `/data`.
2. Cores : `docker compose -f docker/compose.yaml run --rm --no-deps emu-wslg --install-core recommended`
   (ou écran Cores).
3. ROMs : `/data/roms/gba`, `/data/roms/32x`, `/data/roms/megacd`. Copie par `docker cp` ou montage
   `${EMU_HOST_ROMS_DIR}:/data/roms` (voir `docker/compose.yaml`). Le bouton « Ouvrir le dossier des ROMs » affiche
   le chemin.
4. BIOS Mega CD : dans `/data/system` (`bios_CD_E.bin`, `bios_CD_U.bin`, `bios_CD_J.bin`). **Importer un BIOS…**
   ouvre le navigateur intégré : choisir le fichier dans le dossier monté.
5. Sauvegardes : `/data/saves/mgba/`, `/data/saves/genesis_plus_gx/`, `/data/saves/picodrive/`.
6. Vérifier un jeu de chaque système, le son et la manette si elle est visible dans WSL.

## macOS (Intel)

1. Dossiers : `~/Library/Application Support/emu-rust/` (`roms/gba`, `roms/32x`, `roms/megacd`, `system/`).
2. Vos BIOS Mega CD sont sur ce Mac : **Importer un BIOS…** (fenêtre de fichiers macOS) ou copie dans `system/`.
3. Si le GBA est lent : essayer `gpsp` ou `vbam` (Cores → « Core utilisé par système »).
4. Noter la fluidité du son pour chaque système.

## En cas de problème

- **« BIOS manquant »** : vérifier le nom exact et la région affichés ; `emu-rust --check-bios genesis_plus_gx`.
- **Le jeu ne démarre pas** : lire l'erreur du core affichée dans le message ; la copier dans le rapport.
- **CD refusé en zip** : décompresser ou convertir en CHD (`chdman createcd -i jeu.cue -o jeu.chd`).
- **Jeu absent après un scan** : « Voir les fichiers non classés » donne le dossier conseillé.
- **GBA sans logo** : normal sans `gba_bios.bin`.
- **Rapport** : système (Windows, macOS, Docker), jeu, core, étapes, capture d'écran, et la sortie de
  `emu-rust --check-bios genesis_plus_gx`.
