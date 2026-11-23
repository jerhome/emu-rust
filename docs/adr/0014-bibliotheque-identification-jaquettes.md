# ADR 0014 — Bibliothèque, identification et jaquettes

Statut : accepté (2026-11-19). Précise ADR 0005 (sources, sécurité réseau) et ADR 0006 (bibliothèque).
Conception détaillée : `docs/design/v1.0-bibliotheque.md`. Gestionnaire de cores : ADR 0013 (en parallèle).

## Contexte

Version 1.0 : bibliothèque de jeux et jaquettes (phase 4) pour Mega Drive, SNES, Game Boy (+ Color), NES,
Game Gear, Master System ; lancement par double-clic sans `--core/--rom`. ADR 0006 fixe SQLite (`rusqlite`
`bundled`), CRC32 + SHA1, la libretro-database et un catalogue des systèmes en fichier de données. Restent ouverts :
format des DAT, sort d'une base corrompue, pile HTTP/TLS exacte, décodage d'images non fiables, secrets, threads.

## Décision

1. **Catalogue des systèmes** : `crates/emu-library/data/systems.toml`, embarqué (`include_str!`), versionné avec le
   code. Par système : `id` court et stable (`megadrive`, `snes`, `nes`, `gb`, `gbc`, `gamegear`, `mastersystem` :
   base, dossiers), extensions, règle d'en-tête, **nom de base libretro** (`Nintendo - Game Boy` = nom du DAT = dossier
   libretro-thumbnails = clé système du gestionnaire de cores), identifiants ScreenScraper et IGDB. **Aucun core
   ici** : la liste et le choix des cores par système relèvent d'ADR 0013 (`emu-assets/data/cores.toml`,
   `emu_assets::cores::choose_core`), joints par le nom de base libretro.
2. **Identification** : DAT **No-Intro au format clrmamepro** (texte) de `libretro-database/metadat/no-intro/`,
   téléchargés à l'exécution par `emu-assets` (jamais embarqués ni redistribués) puis importés en SQLite par
   `emu-library`. Le format binaire `.rdb` n'est pas lu. Correspondance : SHA1 > CRC32 + taille > nom de fichier.
   Les hashes sont calculés sur le contenu entier **et** sans en-tête (iNES 16 octets, copieur SNES 512 octets).
3. **SQLite** : `rusqlite` + `bundled` (MIT ; SQLite domaine public ; compilateur C déjà requis, ADR 0009).
   La base est un **cache dérivé** des fichiers + quelques données utilisateur (favoris, dernières parties).
   Migrations par `PRAGMA user_version` ; version plus récente que le code ⇒ refus d'écrire. Base illisible ou
   `quick_check` en échec ⇒ renommée `library.sqlite.corrupt-<unix>` et recréée vide, avec un avertissement.
4. **Pile HTTP commune (avec ADR 0013)** : `reqwest` 0.13 `blocking`, `default-features = false`,
   `rustls-no-provider` + `rustls` avec le fournisseur **`ring`** (pas `aws-lc-rs` : CMake/NASM sous Windows et licence
   « OpenSSL » dans son expression SPDX), certificats via `rustls-platform-verifier` (`ca-certificates` dans l'image
   Docker). Un seul module `emu_assets::http` pour cores, DAT et jaquettes ; HTTPS seul, sauf `http://127.0.0.1`
   activable par les tests (wiremock). Les erreurs passent par `reqwest::Error::without_url()` (les URL ScreenScraper
   portent des identifiants).
5. **Jaquettes** : sources dans l'ordre configurable libretro-thumbnails → ScreenScraper → IGDB. Images non fiables :
   ≤ 8 Mio téléchargés, PNG ou JPEG seulement (signature vérifiée), décodées par `image` (features `png`, `jpeg`)
   avec `Limits` (≤ 4096 × 4096, ≤ 64 Mio alloués), dans un thread de travail. Cache disque
   `<data>/covers/<système>/boxart/<sha1>.<ext>` ; absences mémorisées en base (nouvel essai après 30 jours ou à la
   demande).
6. **Secrets** : type `emu_assets::Secret` (`Debug` = `***`, pas de `Display`, sérialisé tel quel dans
   `config.toml` 0600). Jamais loggés, jamais affichés dans l'UI (champ masqué, « défini / non défini »), jamais
   présents dans un message d'erreur. Jeton IGDB en mémoire seulement.
7. **Threads** : l'émulation reste mono-thread (ADR 0010). Scan, import des DAT et file de jaquettes tournent dans
   des threads `std` (canaux `std::sync::mpsc`), chacun avec sa connexion SQLite (mode WAL) ; pas de runtime async dans
   `emu-ui`. Le thread de jaquettes vit dans `emu-ui` : il relie `emu-library` et `emu-assets`, qui ne se connaissent
   pas (graphe de `CLAUDE.md` inchangé).
8. **Point d'entrée** : sans `--core/--rom`, `emu-rust` ouvre la bibliothèque (même fenêtre, même contexte GL pour
   les parties lancées, retour à la bibliothèque en quittant le jeu). Avec `--core/--rom` : comportement 0.1 inchangé.

## Conséquences

- Hors ligne : la bibliothèque reste utilisable (base + cache) ; un jeu non identifié est affiché sous son nom de
  fichier. Sans DAT téléchargé, rien n'est identifié mais tout est jouable.
- Nouvelles dépendances (toutes MIT/Apache/ISC/Zlib, déjà acceptées par `about.toml`) : `rusqlite`, `crc32fast`,
  `sha1`, `reqwest`, `rustls`, `ring`, `rustls-platform-verifier`, `image`, `serde_json` ; dev : `wiremock`, `tokio`.
  À vérifier par `reviewer` (`ring` : « Apache-2.0 AND ISC »).
- ScreenScraper exige aussi des identifiants **développeur** : le projet n'en publie pas ; l'utilisateur les saisit
  ou la source reste désactivée.
- Une base perdue ne coûte que favoris et historique ; elle se reconstruit par un nouveau scan.

## Amendement 2026-11-19 (revue L1)

Schéma final : `game.path` UNIQUE, `file_size` vs `size`, `pinned` (`SCHEMA_VERSION = 2`) ; zip > 1 Gio refusé ; `gbc` accepte `.gb`. Détails dans `docs/design/v1.0-bibliotheque.md`.

## Amendement 2026-11-23 (7.0.2 : sources de jaquettes, essai utilisateur)

Constat : `thumbnails.libretro.com` répond 404 pour toutes les jaquettes (index vide) ; les 404 étaient enregistrés
`Missing` pendant 30 jours sans trace. Le dépôt GitHub `libretro-thumbnails` est le miroir complet. Complète le §5.

1. **Sources libretro, dans l'ordre** : (a) miroir GitHub
   `https://raw.githubusercontent.com/libretro-thumbnails/<base libretro, espaces -> _>/master/Named_Boxarts/<nom>.png`
   en **premier** (répond 200 directement) ; (b) `https://thumbnails.libretro.com/<base>/Named_Boxarts/<nom>.png` en
   **secours** (404/erreur sur (a)). Hôtes autorisés pour les jaquettes : ces deux seuls hôtes, HTTPS seul.
   **Redirections toujours désactivées** (`Policy::none()` conservé) ; un 301/302/307/308 est un résultat
   **inconclusif** (jamais « absente »). Même pile HTTP, même validation d'image (8 Mio, PNG/JPEG, limites de décodage).
2. **Politesse envers raw.githubusercontent.com** (pas de jeton) : concurrence ≤ 2 requêtes simultanées sur cet hôte,
   User-Agent générique `emu-rust/<version>` sans donnée personnelle ; un 403/429 (ou `Retry-After`) arrête la source
   pour la session (résultat inconclusif, pas d'insistance). Les jaquettes sont des images de couverture : autorisées
   par l'ADR 0005 (aucune ROM, aucun BIOS).
3. **« Missing » seulement sur absence crédible** : une source dont les réponses sont des 404 sans **aucun** succès
   après N = 8 requêtes consécutives **de jeux identifiés** (titre issu d'un DAT) couvrant **au moins 3 systèmes
   différents** ; les 404 de jeux non identifiés (noms TOSEC d'ordinateurs) restent de simples absences, non comptées est jugée « en panne » pour la session :
   les résultats 404 de cette source deviennent **inconclusifs** (erreur `SourceBroken`, rien n'est écrit en base) et
   les requêtes suivantes sautent la source. Un seul succès remet le compteur à zéro. Seul un 404 d'une source jugée
   saine est enregistré `Missing`.
4. **Réinitialisation unique** : sans migration de schéma (`SCHEMA_VERSION` inchangé). Un champ de configuration
   `[library] covers_missing_reset: u32` (défaut 0) ; au démarrage, si < 2, `Library::clear_missing_covers()` (existe
   déjà) puis écriture de 2. Un futur changement de sources incrémente la constante.
5. **Visibilité minimale** : échec par jeu journalisé en `warn!` (jeu, source, statut HTTP ; l'URL libretro est
   publique, jamais d'URL ScreenScraper/IGDB) ; statut global « Jaquettes : X trouvées, Y introuvables[, source
   injoignable] » dans la bibliothèque. Reportés (7.1+) : texte par jeu, bouton « réessayer », recherche floue pour
   les noms TOSEC/redump.
