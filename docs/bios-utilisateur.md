# BIOS et ROMs commerciaux utiles (fournis par l'utilisateur)

emu-rust ne télécharge **jamais** de BIOS ni de ROM commerciale (ADR 0005). Les fichiers ci-dessous sont à fournir par
l'utilisateur (idéalement extraits de son propre matériel ou achetés, ex. Amiga Forever). L'application vérifie leur
hash et les place sous le nom attendu par le core (bouton « Importer un BIOS… » prévu en 3.0 ; d'ici là, copie à la
main dans le **dossier des BIOS** affiché par le gestionnaire de cores).

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
| Roland SC-55 (Sound Canvas) | Référence General MIDI des jeux 1993+ (Doom…) |
| *Alternative libre* : SoundFont General MIDI (ex. GeneralUser GS) | Non commerciale ; bon rendu General MIDI sans ROM |

DOSBox Pure cherche ces fichiers dans le dossier système (nécessite l'interface VFS, prévue en 4.0).

## Amstrad CPC et Commodore 64 (5.0)

| Système | Fichier | Intérêt |
|---|---|---|
| CPC | — | ROMs système fournies avec le core (permission d'Amstrad) ; rien d'indispensable |
| C64 | KERNAL / BASIC / CHARGEN | Fournies avec VICE ; rien à faire |
| C64 | JiffyDOS (KERNAL + ROM du lecteur 1541) | Commercial (toujours vendu) : chargements beaucoup plus rapides ; facultatif |

## Plus tard (phase 7 : consoles 32 bits), pour mémoire

| Système | Fichier(s) | Intérêt |
|---|---|---|
| PlayStation | `scph5501.bin` (US), `scph5502.bin` (EU), `scph5500.bin` (JP) | Requis en pratique |
| Saturn | `sega_101.bin`, `mpr-17933.bin` | Requis |
| 3DO | `panafz10.bin` | Requis |
| Game Boy Advance | `gba_bios.bin` | Facultatif ; meilleure compatibilité |

## Ordre d'intérêt (résumé)

1. Kickstart 1.3 et 3.1 (Amiga) · 2. MT-32 (DOS) · 3. TOS 1.04 (ST) · 4. `capsimg` (ST/Amiga) · 5. BIOS GBC ·
6. le reste, surtout cosmétique.
