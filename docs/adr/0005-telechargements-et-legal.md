# ADR 0005 — Téléchargements, sources et règles légales

Statut : accepté (2026-11-19)

## Ce que l'application télécharge

| Contenu | Source | Notes |
|---|---|---|
| Cores | buildbot libretro (`buildbot.libretro.com/nightly/<os>/<arch>/latest/`) | + fichiers `.info` (extensions, BIOS requis) |
| Base d'identification | `libretro-database` (DAT No-Intro / Redump, `.rdb`) | identification par CRC32 / SHA1 |
| BIOS libres | EmuTOS (ST), AROS Kickstart (Amiga) | installés automatiquement |
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
