# ADR 0006 — Données, configuration et bibliothèque

Statut : accepté (2026-11-19)

## Emplacements

Racine de données (`EMU_DATA_DIR`, sinon dossier standard via le crate `directories` ; `/data` dans Docker) :

```text
<data>/
├── config.toml        # réglages, touches, identifiants (600)
├── library.sqlite     # bibliothèque
├── cores/<os>-<arch>/ # cores + .info
├── system/            # BIOS (system directory libretro)
├── roms/<système>/    # ROMs de l'utilisateur (ou dossiers externes déclarés)
├── saves/  states/    # SRAM et savestates, par core puis par jeu
└── covers/<système>/  # jaquettes, captures, logos
```

## Bibliothèque

- SQLite via `rusqlite` (feature `bundled` : pas de dépendance système).
- Scan des dossiers déclarés, hash CRC32 + SHA1 (contenu décompressé pour les `.zip`), identification via la base libretro.
- Systèmes, extensions et cores associés définis dans des fichiers de données versionnés (pas codés en dur).

## Amendement 2026-11-19 — phase 2

- Résolution de la racine, dossier `assets/`, identifiant de core, nommage SRAM / savestates, format des savestates,
  écritures atomiques et règles de `config.toml` : **ADR 0011**.
