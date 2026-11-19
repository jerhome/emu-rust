# ADR 0013 — Gestionnaire de cores : source, intégrité, emplacement, choix du core

Statut : accepté (2026-11-19). Précise ADR 0005 (téléchargements) et ADR 0006 (dossier `cores/`).
Conception détaillée : `docs/design/v1.0-cores.md`. Bibliothèque et pile HTTP commune : ADR 0014.

## Contexte

Version 1.0 : installer, mettre à jour et supprimer les cores depuis l'UI (phase 3), pour Mega Drive, SNES,
Game Boy (+ Color), NES, Game Gear, Master System, sur Linux, Windows, macOS Intel et Docker. Jusqu'en 0.1.x,
l'utilisateur télécharge les cores à la main.

Constat sur le buildbot (2026-11-19) :
- `nightly/<os>/<arch>/latest/` contient `<id>_libretro.<ext>.zip` (une seule entrée : la bibliothèque) et
  `.index-extended` : `<AAAA-MM-JJ> <crc32> <fichier>.zip` par ligne. Le CRC32 est celui **du core décompressé**
  (vérifié sur `gambatte`), pas du zip.
- `assets/frontend/info.zip` contient les `<id>_libretro.info` (≈ 360 fichiers, 300 Kio).
- Aucune signature ni hash cryptographique publié. Les nightlies changent presque chaque jour.
- Les 8 cores retenus ci-dessous existent pour `linux/x86_64`, `windows/x86_64` et `apple/osx/x86_64`.

## Décision

1. **Source** : buildbot officiel `https://buildbot.libretro.com`, canal **nightly** (ADR 0005), chemins
   `linux/x86_64`, `windows/x86_64`, `apple/osx/x86_64`. Autre URL seulement si l'utilisateur l'écrit dans
   `config.toml` (`[cores] buildbot_url`) ; l'UI l'affiche alors en permanence. Aucune autre plate-forme (aarch64 :
   cores manuels uniquement).
2. **Client HTTP** : celui d'ADR 0014 (`emu_assets::http`, `reqwest` blocking + `rustls`/`ring` + certificats du
   système). Corps lus jusqu'à une limite (index 1 Mio, `info.zip` 16 Mio, core 128 Mio) ; délai de connexion
   10 s, délai total 10 min pour un core ; 3 tentatives (erreurs réseau, délais, 5xx) ; **pas de reprise par
   `Range`** (zips de quelques Mio : on recommence à zéro) ; annulation entre deux blocs.
3. **Intégrité** — pas de TOFU au sens strict (chaque nightly est un binaire nouveau, il n'y a pas d'identité stable
   à mémoriser). Stratégie retenue, par couches :
   - authenticité : confiance dans le buildbot via TLS (certificat vérifié, hôte fixe, redirections refusées hors
     de l'hôte) ;
   - transport : CRC32 du core extrait comparé à `.index-extended` (détecte troncature et corruption, pas une
     attaque du serveur lui-même) ;
   - local : **SHA-256 enregistré** dans `manifest.toml` à l'installation et **recalculé avant chaque chargement** ;
     un core modifié hors de l'application n'est jamais chargé (« réinstaller ») ;
   - repli : la version précédente est gardée (`.prev`, retour arrière en un clic) et un core peut être **épinglé**
     (exclu de « tout mettre à jour »).
   Écartés : hashes maintenus dans le dépôt (nightlies quotidiennes), canal `stable/<version>/` avec hashes
   épinglés (possible plus tard, cf. questions ouvertes), signature (inexistante en amont).
4. **Archives** : lues **en mémoire** par un extracteur dédié dans `emu-assets` (`cores::archive`). Les noms
   d'entrée sont seulement **comparés** (`<id>_libretro.<ext>` exact, à la racine) et jamais utilisés pour construire
   un chemin : le « zip slip » est impossible par construction. Nombre d'entrées vérifié avant l'analyse du
   répertoire central, taille déclarée **et** lue bornée, liens symboliques et entrées chiffrées ignorés.
   **Pas de crate partagé** avec `emu-runtime/src/content.rs` : le graphe de `CLAUDE.md` reste inchangé et le contrat
   ici est plus étroit (copie de la seule vérification du répertoire central, commentée).
5. **Emplacement** (précise ADR 0006) : `<data>/cores/<os>-<arch>/` (`linux-x86_64`, `windows-x86_64`,
   `macos-x86_64`) avec `<id>_libretro.<ext>`, `<id>_libretro.<ext>.prev`, `manifest.toml`, `info/*.info`,
   `.cache/index-extended`. Écritures : fichier temporaire dans le même dossier, `sync_all`, renommages, fsync du
   dossier (Unix) ; résidus d'une installation interrompue nettoyés à l'ouverture. Un fichier `*_libretro.<ext>`
   déposé à la main est listé comme « manuel » (utilisable, jamais vérifié ni mis à jour).
6. **Core chargé** : jamais remplacé, supprimé ni restauré tant qu'il est chargé (`set_in_use`) — obligatoire sous
   Windows (DLL verrouillée), plus sûr partout.
7. **Choix du core** : clé de système = **nom de base libretro** (`Nintendo - Game Boy`), commun aux `.info`, à
   libretro-thumbnails et au catalogue d'`emu-library` (`SystemInfo::database`, ADR 0014). Préférences ordonnées
   dans `crates/emu-assets/data/cores.toml` (embarqué), surchargeables par `[cores.preferred]` :

   | Système | Recommandé | Alternatives | Licences |
   |---|---|---|---|
   | Mega Drive, Master System, Game Gear | genesis_plus_gx | picodrive | non commerciale ; MAME (non commerciale) |
   | Super Nintendo | snes9x | bsnes, mesen-s | non commerciale ; GPLv3 ; GPLv3 |
   | Game Boy, Game Boy Color | **gambatte** | sameboy, mgba | GPLv2 ; MIT ; MPL-2.0 |
   | NES | **mesen** | nestopia, fceumm | GPLv3 ; GPLv2 ; GPLv2 |

   Game Boy : gambatte (référence historique de RetroArch, très léger, intégration SRAM/RTC éprouvée ; le Mac
   Intel de l'utilisateur peut être ancien) ; SameBoy est plus précis et sous MIT, proposé en alternative. NES :
   Mesen (le plus précis, maintenu) ; Nestopia si la machine est lente, FCEUmm pour les mappers rares.
   Les cores sont chargés dynamiquement et jamais redistribués par le projet : les licences GPL et « non
   commerciale » sont compatibles avec l'usage personnel (ADR 0001, roadmap).
8. **BIOS** : jamais téléchargés (sauf BIOS libres, 2.0/3.0). Vérification par md5 : celui des `notes` du `.info`,
   complété par `crates/emu-assets/data/firmware.toml` (extrait de `libretro-database/dat/System.dat`, les `.info`
   ne donnent pas tous les hashes). États : présent et valide, absent, hash différent, présent non vérifiable.
   Un BIOS requis absent ⇒ avertissement avec « lancer quand même » (les `.info` ne sont pas toujours exacts) ;
   tous les BIOS des systèmes 1.0 sont optionnels.
9. **Réseau à la demande** : aucune requête au démarrage. L'index et `info.zip` sont téléchargés quand l'utilisateur
   clique « Vérifier les mises à jour » (ou via la CLI), puis mis en cache ; aucune mise à jour automatique.

## Conséquences

- Un serveur buildbot compromis pourrait livrer un core malveillant : risque accepté (même modèle que RetroArch),
  limité par TLS, l'URL fixe et la conservation de la version précédente ; la vérification SHA-256 protège
  seulement contre les modifications locales.
- Le gestionnaire ne fonctionne que sur les 3 plates-formes x86_64 ; ailleurs, `--core <chemin>` reste possible.
- Le code d'extraction existe en deux exemplaires (runtime : contenu ; assets : cores) ; à factoriser dans un crate
  feuille si un troisième besoin apparaît (ADR requise : graphe de dépendances).
- `emu-ui` relie bibliothèque et cores : système de la bibliothèque → `choose_core` → `verified_path` → chargement.
