# BIOS et ROMs commerciaux utiles (fournis par l'utilisateur)

emu-rust ne télécharge **jamais** de BIOS ni de ROM commerciale (ADR 0005). Les fichiers ci-dessous sont à fournir par
l'utilisateur (idéalement extraits de son propre matériel ou achetés, ex. Amiga Forever). L'application vérifie leur
hash et les place sous le nom attendu par le core (bouton « Importer un BIOS… » prévu en 3.0 ; d'ici là, copie à la
main dans le **dossier des BIOS** affiché par le gestionnaire de cores).

Import (3.0) : un fichier **reconnu** par son hash est installé sous le bon nom ; un fichier **inconnu** est refusé
par défaut, avec un bouton « Importer quand même (non vérifié) » qui demande de choisir le nom parmi les fichiers
attendus ; un fichier dont le nom n'est attendu par aucun core connu (ex. ROM SC-55) ne passe pas par l'import : il
se dépose à la main.

Ce que l'application fournit ou trouve **sans rien demander** : EmuTOS (libre, téléchargé sur action), AROS (libre,
**intégré au core puae**, rien à installer), ROM système du CPC et du C64 (**intégrées aux cores**, voir plus bas),
SoundFont GeneralUser GS (libre, téléchargée sur action, 4.0).

Les noms de fichiers sont ceux attendus par les cores libretro ; ils sont revérifiés lors de la conception de chaque
version (hashes dans `crates/emu-assets/data/firmware.toml` et les `.info` des cores).

Légende : **requis** (le core ne fonctionne pas sans) · **recommandé** (gros gain de compatibilité) ·
facultatif (cosmétique ou cas particuliers).

## Consoles (1.0) — tout est facultatif

| Système | Fichier(s) | Intérêt |
|---|---|---|
| Mega Drive | `bios_MD.bin` (TMSS) | Écran « Produced by Sega » ; cosmétique |
| Master System | `bios_E.sms`, `bios_U.sms`, `bios_J.sms` | Animation de démarrage, jeux intégrés (Hang-On, Snail Maze…) |
| Game Gear | `bios.gg` | Écran de démarrage ; cosmétique |
| Game Boy / Color | `gb_bios.bin`, `gbc_bios.bin` | Logo de démarrage ; sur GBC, **palettes de couleurs automatiques** pour les jeux Game Boy |
| SNES, NES | — | Rien d'utile pour l'usage prévu |

## Atari ST (2.0)

| Fichier | Intérêt |
|---|---|
| TOS 1.02 / 1.04 (ST), installé sous `tos.img` | **Recommandé** : compatibilité maximale ; certains jeux plantent sous EmuTOS (libre, installé par défaut) |
| TOS 1.62 / 2.06 (STE) | Jeux et démos STE |
| `capsimg` (voir Amiga) | Lecture des disquettes `.stx` / `.ipf` |

Un `tos.img` fourni par l'utilisateur n'est jamais écrasé par EmuTOS sans son accord (sauvegarde renommée).

## Amiga (3.0)

| Fichier | Intérêt |
|---|---|
| Kickstart 1.3 — `kick34005.A500` | **Recommandé, le plus important** : la grande majorité des jeux A500 (AROS, intégré au core, en est loin) |
| Kickstart 3.1 — `kick40068.A1200` | **Recommandé** : jeux AGA (A1200) et WHDLoad |
| Kickstart 2.04 / 2.05 (A500+ / A600) | Quelques jeux |
| Kickstart CD32 + extension | Jeux CD32 uniquement |
| `capsimg` (bibliothèque IPF/SPS) | Pas commerciale mais licence non libre (à récupérer soi-même) : **disquettes `.ipf` protégées**, format de préservation le plus fidèle |

Les ROMs **Amiga Forever** (Cloanto) conviennent, avec leur fichier `rom.key`.

## PC-DOS (4.0) — son MIDI

| Fichier | Intérêt |
|---|---|
| Roland MT-32 — `MT32_CONTROL.ROM` + `MT32_PCM.ROM` | **Recommandé** pour les jeux Sierra / LucasArts 1988-1992 (King's Quest, Monkey Island…), composés pour le MT-32 |
| Roland CM-32L (ROMs équivalentes) | Variante plus récente, effets sonores en plus |
| Roland SC-55 (Sound Canvas) | Référence General MIDI des jeux 1993+ (Doom…) ; **pas de hash de référence** : non reconnu par « Importer un BIOS… », à déposer à la main dans le dossier des BIOS, **non vérifié**, sans garantie |
| *Alternative libre* : SoundFont **GeneralUser GS** | Libre ; **téléchargée par l'application sur votre action** (autorisée le 2026-11-21, ADR 0005) ; bon rendu General MIDI sans ROM |

DOSBox Pure cherche ces fichiers dans le dossier système (nécessite l'interface VFS, prévue en 4.0). MT-32 et
CM-32L sont reconnus par leur hash à l'import ; une autre version de ces ROM s'importe « non vérifiée » sous l'un
des quatre noms `MT32_CONTROL.ROM`, `MT32_PCM.ROM`, `CM32L_CONTROL.ROM`, `CM32L_PCM.ROM`.

## Amstrad CPC et Commodore 64 (5.0)

| Système | Fichier | Intérêt |
|---|---|---|
| CPC | — | ROM système **intégrées au core cap32** (permission d'Amstrad, mention affichée) ; rien à fournir |
| C64 | KERNAL / BASIC / CHARGEN / DOS 1541 | **Intégrées à VICE, acceptées** (décision de l'utilisateur du 2026-11-21 ; © Commodore, statut de licence incertain, usage personnel) ; rien à fournir |
| C64 | JiffyDOS (KERNAL + ROM du lecteur 1541) | Commercial (toujours vendu) : chargements beaucoup plus rapides ; facultatif |

## Game Boy Advance, 32X et Mega CD (6.0)

| Système | Fichier(s) | Intérêt |
|---|---|---|
| Mega CD / Sega CD | `bios_CD_E.bin` (Europe), `bios_CD_U.bin` (USA), `bios_CD_J.bin` (Japon) | **Requis** : aucun jeu Mega CD ne démarre sans le BIOS **de la région du jeu** ; l'application le contrôle avant le lancement et propose l'import |
| Game Boy Advance | `gba_bios.bin` | Facultatif (le core mgba a un BIOS de remplacement intégré) ; logo de démarrage, compatibilité un peu meilleure |
| 32X | — | Rien à fournir (BIOS intégrés au core picodrive) |

## PlayStation et 3DO (7.0), pour mémoire

| Système | Fichier(s) | Intérêt |
|---|---|---|
| PlayStation | `scph5501.bin` (US), `scph5502.bin` (EU), `scph5500.bin` (JP) | Requis en pratique, par région (même contrôle que le Mega CD) |
| 3DO | `panafz10.bin` | Requis |

## Plus tard (sans version fixée), pour mémoire

| Système | Fichier(s) | Intérêt |
|---|---|---|
| Saturn | `sega_101.bin`, `mpr-17933.bin` | Requis |

## Ordre d'intérêt (résumé)

1. Kickstart 1.3 et 3.1 (Amiga) · 2. BIOS Mega CD de vos régions (6.0, requis) · 3. MT-32 (DOS) · 4. TOS 1.04 (ST) ·
5. `capsimg` (ST/Amiga) · 6. BIOS GBC · 7. le reste, surtout cosmétique.
