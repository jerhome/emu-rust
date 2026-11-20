# tests/data — hand-written fixtures (Q1, ADR 0014)

All content is fictional and generated deterministically (ROM bodies are repeated ASCII text
`EMU-RUST FIXTURE <label>`; no real or commercial data, no network, no keys). Hashes below can be checked with
`sha1sum` / `crc32` (for headered files, hash the stripped bytes: `tail -c +17` for iNES, `tail -c +513` for the copier).
`tests/roms/` (real public-domain homebrew, fetched by `scripts/fetch-test-assets.sh`) is unrelated.

## dat/

`dat/sample.dat` — one clrmamepro DAT mixing all systems (real DATs are per system; tests import it as any one).
Hashes in the DAT are those of the **headerless** content. Contains: 9 normal games (one per ROM above except the unknown homebrew), 1 game with escaped quotes and
backslash in `name`/`description`, 1 game with CRC only (no sha1/md5), 1 game with 2 ROMs (`disc1.bin`, `disc2.bin`),
unknown header field (`unknownheaderfield`), unknown game fields (`serial`, `releaseyear`), unknown rom field
(`status`), and an unknown top-level block `resource ( ... )` with nested blocks (must be skipped; its ROM ignored).
Expected: parse OK, 12 games. The quoted game (`Fictional "Quoted" Game (World)`) has no file on disk.

`dat/malformed/` (each: no panic; error or ignored entry, as noted):

| file | defect | expected |
|---|---|---|
| `unterminated-string.dat` | `name "Never closed` | error |
| `unclosed-block.dat` | `game (` never closed | error |
| `too-deep.dat` | 5000 nested `game (` | error (depth limit), no stack overflow |
| `invalid-hex.dat` | `crc ZZZZ0000`, `sha1 nothex` | error or entry ignored |
| `negative-size.dat` | `size -5` | error or entry ignored |
| `missing-size.dat` | rom without `size` | error or entry ignored |
| `non-utf8.dat` | bytes `\xE9 \xFF \xFE` in a name | error or lossy decode, no panic |
| `bom.dat` | UTF-8 BOM then valid DAT (`Bom Game`, crc `0000abcd`) | parse OK, 1 entry |
| `empty.dat` | 0 bytes | error or 0 entries |

## roms/

Layout follows the folder rule (`roms/<id>/`); `roms/gbc/` holds a `.gb` that must be classified `gbc` (folder wins).

| path | system | file size / CRC32 / SHA1 | hashed content (DAT) |
|---|---|---|---|
| `megadrive/Fictional Racer (World).md` | megadrive | 256 / 57AAAEB3 / EE8D429C4E613EEE2052CB40F20D468E6741550F | same |
| `snes/Fictional Quest (USA).sfc` | snes | 1024 / 514D1A6F / F807F29F4C3CF58C5568C1D58C6AA40C5FC7A7E6 | same |
| `snes/Fictional Quest (USA) [copier].smc` | snes | 1536 (512 zero bytes + above) / D2E4543F / D516133FFDF161B049AA110CBAACA3009B3C4017 | headerless = row above (identified via no-header hash) |
| `nes/Fictional Bros (Europe).nes` | nes | 80 (16-byte iNES `NES\x1A 01 01`) / E19510AE / A6E8C676F1F06F079B235D3087E20823F679D14D | 64 / C0201158 / 895083DB96C25A78D10DF311DD11E1301BB6D626 |
| `gb/Fictional Blocks (World).gb` | gb | 128 / DB21216F / 3BE53B7C3888312D40A0AF9D3BB883610052FB3F | same |
| `gbc/Fictional Color Quest (World).gb` | gbc | 128 / 13AEFFEC / EEEE3C45E4A30C02D155488E9C9EB0034FDEDED9 | same |
| `gbc/Fictional Color Run (World).gbc` | gbc | 128 / D47896A3 / B47581612B2799C815B8DC2644DF11466020E50E | same |
| `gamegear/Fictional Gear (Japan).gg` | gamegear | 64 / BB9F9A04 / 15D05B50EFF9E10A4C2962F37C2B5DFC3E2965E4 | same |
| `mastersystem/Fictional Master (Brazil).sms` | mastersystem | 64 / 0050B1AB / 6154B5E83288774661A3A3CE3FD7AEFD63B97C46 | same |
| `gb/Fictional Zipped (World).zip` | gb | zip 184 / 0774D74A / 1C23A01A229825DB263319E2E2C872F1B8F78AFE | entry `Fictional Zipped (World).gb`: 128 / 2423166A / 6A0C65DDDF264F9EEA2EF2C64081B9E0770D70F9 |
| `gb/unknown-homebrew.gb` | gb | 96 / AA37A8AA / 4A3661A389870912B66CC377EB6CFFD360485736 | not in DAT: title = file name, playable |
| `misc/notes.txt` | — | text | wrong extension, ignored |
| `misc/empty.gb` | — | 0 bytes | edge case: empty file, no panic |
| `evil/evil.zip` | — | single entry named `../evil.gb` (64 bytes, CRC32 DDCA035B, SHA1 63804975AB441635F9721288A37BB22AFB8019B7) | path traversal: must never write outside any dir; scanner may hash in memory but must not extract by name |

`misc/` and `evil/` are not system folders: tests should point the scanner at them explicitly (or at `roms/` with
extension-based detection). Zip timestamps are fixed (1980-01-01) so the zips are reproducible.

## covers/

`{BASE}` in URLs is a placeholder: tests replace it with the wiremock URI (`http://127.0.0.1:<port>`).

- `screenscraper/jeuInfos.json` — plausible `jeuInfos.php` reply with `box-2D` in jp, us, eu, wor (+ `box-3D`, `ss`).
  Region priority wor > eu > us > jp must pick `{BASE}/ss/media/box-2D-wor.png`.
- `screenscraper/jeuInfos-no-box.json` — no `box-2D` ⇒ absent (not an error).
- `screenscraper/jeuInfos-regions-us-jp.json` — only us and jp ⇒ us.
- `igdb/token.json` — OAuth client-credentials reply (fake token).
- `igdb/games.json` — 3 games: two with `cover.image_id` (`fxcover01`, `fxcover02`), one without cover.
- `igdb/games-empty.json` — `[]` ⇒ absent.
- `images/valid.png` — 4x4 RGBA PNG, 75 bytes, SHA1 D0252735D9177843DDC35E451E179402C0D0C09 (lowercase in `sha1sum`).
- `images/truncated.png` — first half (37 bytes) of the above; decode must fail cleanly.
- `images/huge-header.png` — valid signature and IHDR announcing 100000x100000 (66 bytes); must be rejected by the
  `Limits` (≤ 4096x4096) before allocating.
- `images/bad-signature.png` — text with a `.png` name; rejected by the signature check.

Not provided (not requested): `tests/data/library/` (fixture tree for Xvfb captures) and a JPEG fixture.

## Core factice étendu (`tests/mock-core/mock_core.c`, tâche Q1 v2.0)

Protocole complet en tête du fichier C (source de vérité). Résumé : tout est désactivé par défaut (les images et
empreintes existantes ne changent pas) ; des variables d'environnement, lues au `retro_set_environment`, activent :

| Variable | Effet |
|---|---|
| `MOCK_KEYBOARD` = `1` / `cb` / `poll` | `SET_KEYBOARD_CALLBACK` enregistré (événements mémorisés) et/ou `RETRO_DEVICE_KEYBOARD` lu à chaque `retro_run` (ids 0..341) |
| `MOCK_MOUSE` | `RETRO_DEVICE_MOUSE` (port 0) lu à chaque run : dx/dy cumulés, boutons, molette |
| `MOCK_DISK` = `basic` / `ext` | interface disque 13 ; `ext` : version 57 puis 58 (repli sur 13). 8 images max, index == nombre ⇒ « pas de disque », changement refusé hors éjection, `.m3u` lu (chemins relatifs à son dossier), `set_initial_image` appliqué au chargement |
| `MOCK_DISK_DUMP` | réécrit `mock-disk.txt` dans `GET_SAVE_DIRECTORY` à chaque changement (interface, éjection, index, nombre, initial, `image<i>=chemin\|label`) |
| `MOCK_NO_GAME` | `SET_SUPPORT_NO_GAME` = true et `retro_load_game(NULL)` accepté (graine 0) ; sans elle : false et refus |

Affichage des registres (si au moins une variable ci-dessus est définie) : dans l'image 64×48 XRGB8888, lignes 0..39 =
5 bandes de 8 lignes × 8 cellules de 8 px. Une cellule = un octet `v` en gris `0x00vvvvvv`, lu au centre
(`x = 8*cellule+4`, `y = 8*bande+4`) ; les valeurs 16 bits sont little-endian sur 2 cellules. Lignes 40..47 : motif de base.

| Bande | Cellules |
|---|---|
| 0 clavier (callback) | 0 événements reçus ; 1-2 dernier code ; 3 drapeaux (bit0 appui, bit1 caractère ≠ 0) ; 4-5 caractère ; 6-7 modificateurs |
| 1 clavier (compteurs / lecture) | 0 touches lues enfoncées ; 1-2 plus petit code ; 3-4 plus grand code ; 5 appuis reçus ; 6 relâchements ; 7 événements avec caractère |
| 2 souris | 0-1 dx cumulé (int16) ; 2-3 dy cumulé ; 4 boutons (bit0 gauche, 1 droit, 2 milieu, 3 molette haut, 4 bas) ; 5 appuis gauche ; 6 appuis droit ; 7 crans de molette |
| 3 disque | 0 index (255 si ≥ 255) ; 1 nombre d'images ; 2 éjecté ; 3 `set_image_index` réussis ; 4 / 5 octet bas FNV-1a du chemin / label courant ; 6 image initiale + 1 ; 7 drapeaux (bit0 interface 58, bit1 `load_game(NULL)`, bit2 interface 13, bit3 `replace_image_index` appelé, bit4 `add_image_index` appelé) |
| 4 clavier (historique) | 4 derniers codes du callback, le plus récent d'abord, u16 sur 2 cellules |

Les compteurs sont remis à zéro par `retro_init` (pas par `retro_reset`). Les valeurs ne sont exactes qu'en XRGB8888
(variantes 16 bits : gris quantifié). Les variables sont lues dans l'environnement du processus : les tests qui les
positionnent doivent le faire avant `Core::load` et ne pas tourner en parallèle d'autres tests du même binaire qui
chargent le mock (`std::env::set_var` global ; sérialiser, ou un binaire de test dédié).

## library/

`library/library-2.0.sqlite` — a real `library.sqlite` (schema v3, `user_version = 3`) written by the **2.0.0-rc.2** code
(tag `v2.0.0-rc.2`, crate `emu-library`), not by hand: `library/gen20.rs.txt` is the example that produced it (copy it as
`crates/emu-library/examples/gen20.rs` in a checkout of the tag, run `cargo run -p emu-library --example gen20 -- <dir>`).
Afterwards the absolute ROM paths were rewritten to `/r/…` (files do not exist) and the file converted to a single file
(`journal_mode=delete`, `VACUUM`). Content: 7 games (ST: `Saga (Disk 1 of 2)` with 2 disks, `Solo (1990)(Pub)` identified;
Mega Drive: `Delta`, `Gamma` identified, `Unlisted` unknown; SNES: `Alpha`, `Beta` identified), favorites on 1 and 4,
history on 1 (2 plays) and 6, a cached cover (game 1) and a missing one (game 2), one DAT per system (`snes`, `megadrive`, `atarist`).
Used by `crates/emu-library/tests/migration_v4.rs`.
