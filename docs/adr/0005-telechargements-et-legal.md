# ADR 0005 — Téléchargements, sources et règles légales

Statut : accepté (2026-11-19) ; amendé le 2026-11-19 (version 1.0) et le **2026-11-21** (AROS intégré au core, SoundFont
libre, règle « Builtin » : voir les amendements en fin de document, qui prévalent sur le tableau ci-dessous)

## Ce que l'application télécharge

| Contenu | Source | Notes |
|---|---|---|
| Cores | buildbot libretro (`buildbot.libretro.com/nightly/<os>/<arch>/latest/`) | + fichiers `.info` (extensions, BIOS requis) |
| Base d'identification | `libretro-database` (DAT No-Intro / Redump, `.rdb`) | identification par CRC32 / SHA1 |
| BIOS libres | EmuTOS (ST) ; ~~AROS Kickstart (Amiga)~~ : **non téléchargé**, intégré au core puae (amendement 2026-11-21) | EmuTOS : installé sur action de l'utilisateur, SHA-256 épinglé (ADR 0015) |
| SoundFont libre | GeneralUser GS (General MIDI, DOS) | sur action de l'utilisateur, SHA-256 épinglé (amendement 2026-11-21, version 4.0) |
| Covers | 1) libretro-thumbnails (sans compte) 2) ScreenScraper (compte utilisateur + identifiants dev) 3) IGDB (Client ID / secret Twitch) | ordre de repli configurable |
| ROMs (phase 8) | domaine public / homebrew uniquement (archive.org collections PD, PDRoms, itch.io…) | |

**Jamais** : ROMs ou BIOS commerciaux sous droits. L'utilisateur les place lui-même ; l'app vérifie leurs hashes
(md5 des `.info`) et indique ce qui manque.

## Règles de sécurité

- HTTPS uniquement (`reqwest` + `rustls`), délais d'expiration, taille maximale par téléchargement.
- Écriture dans un fichier temporaire puis renommage atomique ; vérification de checksum quand la source en fournit.
- Extraction d'archives : refus des chemins absolus et des `..` (zip slip), limite de taille décompressée.
- Les cores sont du code natif exécuté : uniquement depuis le buildbot officiel, URL non modifiable sans action explicite.
- Respect des quotas d'API (ScreenScraper, IGDB) : file d'attente, cache local, `User-Agent` générique.
- Identifiants dans la config utilisateur (permissions `600`), jamais loggés ni commités.

## Amendement 2026-11-19 — version 1.0

- Pile HTTP/TLS exacte (`reqwest` + `rustls` / `ring`), DAT No-Intro au format clrmamepro, limites des images : **ADR 0014**.
- Gestionnaire de cores (canal nightly, intégrité CRC32 d'index + SHA-256 local vérifié avant chargement, version
  précédente conservée, extraction en mémoire, aucune requête au démarrage) : **ADR 0013**.

## Amendement 2026-11-21 — AROS, SoundFont, ROM intégrées aux cores

Décisions de l'utilisateur du 2026-11-21 (`docs/questions-ouvertes.md`) et corrections de la revue croisée des
conceptions 3.0 à 6.0. `CLAUDE.md` (contraintes non négociables) porte déjà la SoundFont.

1. **AROS n'est ni téléchargé ni installé par emu-rust.** Il n'existe en amont qu'une nightly m68k non épinglable
   (ADR 0017 §Contexte) ; l'AROS utilisé est celui **compilé dans le core puae**, livré par le buildbot avec le core.
   La ligne « BIOS libres : EmuTOS, AROS — installés automatiquement » du tableau est donc corrigée : seul EmuTOS est
   un BIOS libre **téléchargé** (catalogue `free_bios.toml`, URL, taille et SHA-256 épinglés, sur action de
   l'utilisateur). `free_bios.toml` n'a pas d'entrée `aros` ; une entrée pourra être ajoutée si une archive stable à
   hash épinglable apparaît (nouvelle décision). Là où `CLAUDE.md` et la feuille de route écrivent « BIOS libres
   (EmuTOS, AROS) », il faut lire : AROS = intégré à puae.
2. **SoundFont libre GeneralUser GS : téléchargement autorisé** (version 4.0, ADR 0018 §5). Conditions : uniquement
   **sur action de l'utilisateur** ; version figée, URL, taille et **SHA-256 épinglés** dans `free_bios.toml`
   (`kind = "soundfont"`) ; HTTPS, écriture atomique, vérification avant installation (règles de sécurité
   ci-dessus) ; jamais empaquetée ni redistribuée ; licence « GeneralUser GS License v2.0 » citée (README de release,
   écran Cores). C'est une ressource libre **nommément autorisée**, pas une catégorie ouverte : toute autre ressource
   téléchargeable demande une nouvelle décision. **Taille** : le code livré borne une archive de BIOS à 16 Mio et une
   image extraite à **4 Mio** (`MAX_BIOS_ARCHIVE_BYTES`, `MAX_BIOS_IMAGE_BYTES`, `crates/emu-assets/src/bios.rs`) ;
   le fichier fait 32 Mo. La 4.0 doit donc prévoir une **limite propre aux SoundFont** (64 Mio), sans relâcher
   celles des BIOS.
3. **Règle « Builtin » : ROM système compilées dans un core.** Une ROM système **intégrée au binaire d'un core
   officiel du buildbot** n'est pas un « téléchargement de BIOS » au sens de cette ADR : emu-rust télécharge le core
   tel quel (ADR 0013), ne l'ouvre pas, n'en extrait rien, ne copie aucune ROM dans `system/` et ne redistribue
   jamais le binaire. L'application l'affiche comme tel (`FirmwareState::Builtin` / `CoreFallback`, notice dans
   l'écran Cores et le README de release). Cas connus :

   | Core | ROM intégrées | Statut |
   |---|---|---|
   | puae | AROS (remplaçant libre du Kickstart) | libre (AROS Public License 1.1) |
   | hatarib | EmuTOS | libre (GPL-2.0-or-later) |
   | cap32 | ROM Amstrad CPC (OS, BASIC, AMSDOS) | **permission d'Amstrad** de redistribuer, mention obligatoire reproduite (ADR 0019 §2) |
   | vice_x64, vice_x64sc | KERNAL, BASIC, CHARGEN, DOS 1541 © Commodore | pas de permission connue ; **accepté par l'utilisateur le 2026-11-21** (usage personnel) |
   | mgba | BIOS HLE de la GBA | réimplémentation du projet mGBA — **à vérifier** (ADR 0020 §2) |
   | picodrive | remplaçant du BIOS 32X (code de démarrage 68000/SH2 généré par le core) ; aucune image du BIOS Sega | code original de picodrive, licence MAME non commerciale du core (vérifié par GV1 le 2026-11-25, ADR 0020 §2) |

   Un core dont les ROM intégrées n'ont ni licence libre, ni permission, ni acceptation explicite de l'utilisateur
   n'est pas proposé par défaut : la question lui est posée (comme pour le C64).
4. **Inchangé** : aucun téléchargement de ROM ou de BIOS commercial ; Kickstart, TOS, BIOS Mega CD, MT-32, JiffyDOS,
   BIOS PlayStation / 3DO… sont fournis par l'utilisateur (import avec vérification de hash, ADR 0017 §3).
5. **Notices** : `packaging/THIRD-PARTY-DATA.txt` ne décrit que les données **embarquées** dans le binaire et les
   archives (dérivées de `System.dat`). Ce qui est téléchargé à l'exécution ou intégré à un core (DAT, cores,
   SoundFont, ROM intégrées) est cité dans le README de release et l'écran Cores.
