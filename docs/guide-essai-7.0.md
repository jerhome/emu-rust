# Guide d'essai — version 7.0 (PlayStation, 3DO)

Version : **v7.0-rc.1** (à venir).

Builds : https://github.com/jerhome/emu-rust/releases (pré-release `v7.0.0-rc.1`).

Durée : environ 1 h 30. But : vérifier GTA (PlayStation) **avec et sans** votre BIOS `scph5502.bin` (Europe), le
mode **DualShock**, le **changement de core**, puis Road Rash (3DO) **avec et sans** `panafz10.bin`, au clavier
60 % AZERTY (Windows), sur macOS Intel et dans Docker (WSLg), puis à la manette DS4. Notez pour chaque étape « OK »
ou ce qui s'est passé (capture d'écran bienvenue).

Rappel : emu-rust ne télécharge **aucun** jeu ni **aucun** BIOS commercial. Les BIOS PlayStation et 3DO viennent
de **vos** fichiers ; ils sont **recommandés, jamais requis** : sans eux, chaque core démarre avec son BIOS de
remplacement intégré (compatibilité réduite) et l'application le dit **une fois par jeu**.

## À savoir avant de commencer

- **Cores** : PlayStation = `pcsx_rearmed` (défaut, rapide), `swanstation` (rendu logiciel seulement) et
  `mednafen_psx` (Beetle PSX, précis, plus lent) ; 3DO = `opera` (licence non commerciale : usage personnel).
  Écran **Cores** → « Core utilisé par système » pour changer ; un core non installé y propose « Installer ».
- **Rangement** : `roms/psx/` et `roms/3do/` (un dossier par jeu conseillé). Le `.cue` et **tous** ses `.bin`
  côte à côte, noms d'origine. Un titre qui existe sur plusieurs machines (Road Rash : 3DO, PlayStation, Mega CD)
  **doit** être rangé dans le bon dossier si c'est un `.chd`.
- **Formats** : PlayStation `.cue`, `.chd`, `.iso`, `.pbp`, `.m3u`, piste `.bin` seule ; 3DO `.cue`, `.chd`,
  `.iso`, `.bin`. Les `.chd` et `.pbp` sont reconnus par leur **nom** (gardez l'étiquette `(Europe)`…). **Jamais de
  CD dans un zip.**
- **Vos fichiers ne sont jamais modifiés ni copiés** (un `.pbp` non plus). Carte mémoire PlayStation :
  `saves/<core>/<jeu>.srm`, une par jeu, commune aux disques d'un même jeu. NVRAM 3DO :
  `saves/opera/opera/per_game/<jeu>.*.srm`.
- **Jeux PAL protégés (LibCrypt)** : déposez le fichier `.sbi` à côté de l'image, sous le même nom.

## Installer les cores

Écran **Cores** → cores recommandés → installer `pcsx_rearmed` et `opera` (`swanstation` et `mednafen_psx` plus
tard, étape 3.3). Ou : `emu-rust --install-core recommended` (n'installe que le premier core de chaque système).

## Touches

| PlayStation | Clavier | Manette DS4 |
|---|---|---|
| Croix | Flèches ou ZQSD | Croix (ou stick gauche en mode numérique) |
| ✕ / ○ / □ / △ | J / K / U / I | ✕ / ○ / □ / △ |
| L1 / R1 | O / L | L1 / R1 |
| L2 / R2 | N / M | L2 / R2 |
| Start / Select | Entrée / Maj droite | Options / Share |

| 3DO | Clavier | Manette DS4 |
|---|---|---|
| A / B / C | U / J / K | □ / ✕ / ○ |
| L / R | O / L | L1 / R1 |
| P (lecture) / X (stop) | Entrée / Maj droite | Options / Share |

Menu : Ctrl droit (clavier) ou bouton PS (manette).

## Partie 1 — GTA (PlayStation) sans BIOS

1. Aucun `scph*.bin` dans le dossier `system/`. Écran **Cores** : la ligne de `pcsx_rearmed` dit « BIOS HLE
   intégré — BIOS d'origine recommandé (scph5502.bin, scph5501.bin ou scph5500.bin selon la région) », en bleu
   (pas une erreur).
2. Copier GTA (`.cue` + `.bin`, ou `.chd`) sous `roms/psx/`, **Scanner** : il apparaît sous « PlayStation », titre
   Redump reconnu par sa série (`SLES-00032`) pour un `.cue`.
3. Lancer : dialogue **une seule fois** « BIOS absent : le core utilise son remplaçant intégré… Fichier attendu :
   scph5502.bin » avec **Importer un BIOS…** et **Continuer avec la ROM intégrée** ; choisir Continuer. Le jeu
   démarre. Quitter puis relancer : **plus** de dialogue.
4. Jouer quelques minutes, vérifier le son. Noter si le jeu se comporte mal (c'est le cas à documenter).
5. **Carte mémoire** : sauvegarder dans le jeu, quitter, relancer : la sauvegarde est là
   (`saves/pcsx_rearmed/<jeu>.srm`).

## Partie 2 — GTA avec votre BIOS Europe

1. Écran **Cores** → **Importer un BIOS…** → votre `scph5502.bin` : reconnu par son empreinte, copié sous ce nom.
   La ligne passe à « BIOS présent ».
2. Lancer GTA : aucun dialogue. Le logo PlayStation au démarrage dépend de l'option
   `pcsx_rearmed_show_bios_bootlogo` (Réglages PlayStation).
3. `emu-rust --check-bios pcsx_rearmed` : une ligne par région, Europe « présent » ; code de sortie 0 même sans les
   autres régions.

## Partie 3 — Manette, DualShock, changement de core

1. **Mode numérique** (défaut) : à la DS4, le stick gauche fait la croix dans GTA.
2. Menu → **Manette : Numérique** → Entrée : passe à **DualShock (analogique)**, appliqué tout de suite
   (message « Manette DualShock : sticks analogiques (retenu pour ce jeu) »). Dans un jeu analogique, les sticks
   répondent ; L3 / R3 aussi. Relancer le jeu : le mode est retenu. Revenir à Numérique de la même façon.
3. Réglage par défaut pour tous les jeux : bibliothèque → Réglages → Manette → « Manette PlayStation par défaut ».
   Ligne de commande : `--pad-device dualshock`.
4. **Changement de core** : Cores → « Core utilisé » → PlayStation → **SwanStation (rendu logiciel)** → « Installer
   swanstation » si besoin. Relancer GTA : message « Carte mémoire reprise de pcsx_rearmed » ; la sauvegarde de la
   partie 1 est là. Même essai avec **Beetle PSX (précis, plus lent)**. Revenir à PCSX-ReARMed.
5. **Jeu à plusieurs CD** (si vous en avez un) : `Jeu (Disc 1).cue`, `Jeu (Disc 2).cue` (ou `.m3u`, ou `.pbp`
   multi-disque) : **un seul** jeu. Au changement demandé : menu → **CD…** → le CD 2, ou **PS + L3** (disque
   suivant) à la manette. Une seule carte mémoire pour tous les disques.

## Partie 4 — Road Rash (3DO)

1. Copier Road Rash sous `roms/3do/` (**pas** à la racine si c'est un `.chd` : le titre existe sur trois machines),
   **Scanner** : classé « 3DO ».
2. **Avec `panafz10.bin`** (importé par « Importer un BIOS… ») : lancer ; le jeu démarre ; son, manette, pas de
   menu CD (le core 3DO ne change pas de disque : un jeu par disque).
3. **Sans `panafz10.bin`** (retirez-le du dossier `system/`) : avertissement unique « … (Opera Boot (intégré au
   core)). Fichier attendu : panafz10.bin » ; le core démarre avec **Opera Boot** (libre, très récent). Noter si le
   jeu démarre : c'est la question ouverte de cette version.
4. Sauvegarde dans le jeu, quitter, relancer : elle est là (NVRAM par jeu).
5. Un disque PlayStation rangé par erreur sous `roms/3do/` : message « Ce disque appartient à un autre système… ».

## Ligne de commande

- `emu-rust --core pcsx_rearmed --rom "GTA (Europe).cue"` ; `--pad-device dualshock` pour le mode DualShock.
- `emu-rust --core pcsx_rearmed --rom programme.exe` : un PS-EXE (homebrew) se lance hors bibliothèque.
- `emu-rust --check-bios pcsx_rearmed`, `--check-bios opera` (une ligne « toutes régions »), `--list-bios`.

## Windows

Dossiers : `%APPDATA%\emu-rust\data\` (`roms\psx`, `roms\3do`, `system\`). Essayer GTA au clavier AZERTY puis à la DS4.

## macOS (Intel)

1. Dossiers : `~/Library/Application Support/emu-rust/` (`roms/psx`, `roms/3do`, `system/`).
2. Vos BIOS sont sur ce Mac : **Importer un BIOS…** (fenêtre de fichiers macOS).
3. Si PCSX-ReARMed est lent ou refuse de démarrer : Réglages PlayStation → option du recompilateur
   (`pcsx_rearmed_drc`), ou Beetle PSX / SwanStation. Noter la fluidité de l'image et du son pour chaque core.

## Docker (WSLg)

1. Lancer l'image comme d'habitude (`docker/README.md`). Données dans le volume `/data`.
2. Cores : `docker compose -f docker/compose.yaml run --rm --no-deps emu-wslg --install-core recommended`.
3. ROMs : `/data/roms/psx`, `/data/roms/3do` (`docker cp` ou montage `${EMU_HOST_ROMS_DIR}:/data/roms`).
4. BIOS : `/data/system` (`scph5502.bin`, `panafz10.bin`) ; **Importer un BIOS…** ouvre le navigateur intégré.
5. Vérifier GTA (son, clavier, manette si visible dans WSL) et noter la vitesse.

## Limites connues

- Rendu logiciel seulement : pas de haute résolution ni de PGXP (SwanStation compris).
- `.chd` et `.pbp` reconnus par leur nom seulement ; jeux 3DO à plusieurs CD non regroupés.
- Pas de vibration ; cartes mémoire **copiées** (une fois) d'un core à l'autre, pas synchronisées.
- La seconde carte mémoire est désactivée par défaut (option du core).
- « Réinitialiser les sauvegardes » d'un jeu PlayStation pose `saves/<core>/<jeu>.no-adopt` : la carte d'un autre core n'est plus reprise pour ce jeu et ce core (supprimez ce fichier pour réactiver la reprise).

## En cas de problème

- **Le jeu plante sans BIOS** : importer le BIOS de sa région (le message indique le nom), ou changer de core.
- **Stick sans effet** : passer en DualShock (menu → Manette) ; inversement, en DualShock le stick ne fait pas la
  croix dans un jeu numérique.
- **Jeu absent après un scan** : « Voir les fichiers non classés » donne le dossier conseillé (`roms/psx/`…).
- **Rapport** : système, jeu, core, étapes, capture, et la sortie de `emu-rust --check-bios pcsx_rearmed`.
