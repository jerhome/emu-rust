# Données de test libres, versions 4.0 à 6.0 (Mega CD en tête)

Inventaire du 2026-11-20, établi par `qa-systems`. Objectif : lignes prêtes pour `scripts/fetch-test-assets.sh`
(agent `platform-build`). Aucun binaire n'est commité ; tout est téléchargé avec URL https + SHA-256 épinglé.
Tous les SHA-256 ci-dessous ont été **calculés par téléchargement réel** (sauf mention « verify.txt officiel »),
les fichiers ont été inspectés (`strings`, `unzip -l`) mais **jamais exécutés**, puis supprimés.

Périmètre : la **7.0 (PS1, 3DO) est hors périmètre** (décision utilisateur, consigne du coordinateur) : rien n'a été
cherché pour PS1 ni 3DO.

Règle générale : le contenu retenu est sous licence libre vérifiée sur le fichier `LICENSE` ou le fichier de
métadonnées du projet. Les cores sont en TOFU (`tests/cores/*.sha256`) comme les autres.

---

## 1. Mega CD (6.0, livré en avance dans la 2.1) — priorité

### 1.1 Contenu libre : 240p Test Suite, version Sega CD / Mega CD 1.21

Trouvé : **un vrai disque Mega CD libre, avec pistes audio**, dans le dossier SourceForge déjà utilisé par le
script pour la version Mega Drive (même projet, même auteur). C'est la seule image de disque libre trouvée
(recherches GitHub `sega cd`, `mega cd homebrew`, `megacd` : uniquement des sources sans binaire ou des lecteurs
nécessitant d'être construits).

| Élément | Valeur |
|---|---|
| Projet | 240p Test Suite (Artemio Urbina et al.), https://github.com/ArtemioUrbina/240pTestSuite |
| Licence | **GPL-2.0** (fichier `LICENSE` du dépôt = GNU GPL v2). Les ISO contiennent en plus le texte standard de licence Sega du secteur système (boot header « PRODUCED BY OR UNDER LICENSE ») : c'est le gabarit du format, pas du code Sega redistribué ; à mentionner dans `THIRD-PARTY-DATA.txt`. |
| URL stable | `https://sourceforge.net/projects/testsuite240p/files/OldFiles/Sega_Genesis-MegaDrive-SegaCD_MegaCD/240pSuite-SegaCD-1.21.zip/download` (même schéma que `ROM_URL` du script ; redirections https) |
| Taille zip | 5 524 628 o (< `--max-filesize 64M` du script) |
| SHA-256 zip | `61e7a67d08cddfff9d8f6b5a901daec87d004a80d28cd1854b104901abe4bde3` |
| Décompressé | 107 492 142 o au total (3 ISO de 32 MiB) : sous `MAX_UNZIPPED_BYTES` (128 MiB) mais proche ; **ne extraire que les 4 fichiers USA** (≈ 40 Mio) ou relever la limite pour ce zip |

Contenu du zip (SHA-256 des membres) :

| Membre | Octets | SHA-256 | Rôle |
|---|---|---|---|
| `240p_SegaCD_USA.iso` | 33 554 432 | `f0b2a2849b269bdbd1d7f2092197109ff186b96f9617bf55595bb23105ae4428` | piste 1, MODE1/2048, en-tête `SEGADISCSYSTEM`, région U |
| `240pSuite_USA.cue` | 286 | `6e832cc64eee9782da21562f6869a5b67c482aecbb0dc9311de7baa8ee6fda73` | 3 pistes : données + 2 pistes audio WAV, `PREGAP` / `POSTGAP` |
| `240p_SegaCD_Track-02.wav` | 2 645 956 | `9d6003f937bb4d7105a7d2e740c3b306efe18e3dbb6db11157fab1528fba418d` | piste audio 2 |
| `240p_SegaCD_Track-03.wav` | 4 182 032 | `2d819faa2f65f6792c212a79bf846a135430228a57e2d7cf76bffdb915e2042e` | piste audio 3 |
| `240p_MegaCD_EU.iso` | 33 554 432 | `009cbf842740ca3b911d2014112984a096313435768a7719d9d00c1be43993fe` | région E |
| `240pSuite_EU.cue` | 287 | `945f5fb47997d1a63057db0b7285a66a7ebba0c41bbd4a60991e62b6b90eec70` | idem, même WAV |
| `240p_MegaCD_JP.iso` | 33 554 432 | `3bc887e3c3c5e6970bb939a5d8abded07ebdb1f9ce9fdf1ad1f155b04ccbb7b5` | région J |
| `240pSuite_JP.cue` | 285 | `3bb2b2bf64f0e7e032432771bafc12162080244b3a4bd9a9eec38219dc29c427` | idem |

Format : **cue + iso (données) + wav (audio)**. Le `.cue` fait référence à des `.wav` : à vérifier côté
`emu-library` (pistes masquées, `.cue` hors dossier ⇒ `megacd` par l'en-tête `SEGADISCSYSTEM`) que des pistes
`WAVE` sont bien rattachées au `.cue` et non classées en jeux orphelins. **Pas de `.chd` libre prêt à l'emploi** :
pour couvrir `.chd`, le test peut générer un CHD à partir de ces fichiers avec `chdman createcd` si l'outil est
installé (sinon, test sauté) ; non vérifié ici.

Identification : titre d'en-tête « 240P TEST SUITE » / série `GM 00-2501-14`, région J/U/E selon l'ISO. Utile
pour les tests `DiscRegion`, `disc_serial` et `probe_bin` **sur du vrai contenu** (en plus des disques factices
générés par `mkcd.py`).

Recommandation : épingler le zip entier (SHA-256 ci-dessus), extraire les fichiers USA vers
`tests/roms/megacd/` (dossier, pour que les règles « cue sous dossier » s'appliquent), garder EU/JP en option
(test `DiscRegion` multi-région : même contenu, trois en-têtes).

### 1.2 Ce que chaque test peut vérifier

Sans BIOS (CI et conteneur, **le cas par défaut**) :

| Test | Entrée | Résultat attendu (déjà constaté dans `docs/essais-gba-32x-megacd.md` §Mega CD) |
|---|---|---|
| Contrôle préalable de firmware | dossier système vide + `240pSuite_USA.cue` | `emu-assets` : `Missing` pour `bios_CD_U.bin` (`EU` ⇒ `bios_CD_E.bin`, `JP` ⇒ `bios_CD_J.bin`) |
| Lancement par `genesis_plus_gx` | idem | `retro_load_game` échoue ; `core_error` contient `Unable to open CD BIOS` / `bios_CD_U.bin` ; le frontend affiche « BIOS Mega CD manquant : bios_CD_U.bin », sans plantage, code de sortie propre |
| Région lue dans l'en-tête | les trois `.cue` | contrôle demande `bios_CD_U.bin`, `bios_CD_E.bin`, `bios_CD_J.bin` respectivement |
| Bibliothèque | dossier `megacd/` avec cue+iso+2 wav | un seul jeu « Mega CD », les `.iso` et `.wav` masqués, région et série lues, aucune lecture complète de l'ISO de 32 MiB (seulement l'en-tête) |
| Identification Redump | — | non applicable (disque homebrew absent de Redump) ; l'extrait de DAT reste factice |
| picodrive | même `.cue` | même erreur de BIOS (`bios_CD_*.bin` aux mêmes noms) ; picodrive peut aussi lire le Mega CD |

Les cores déclarent les BIOS `bios_CD_*` « optionnels » dans leur `.info`, mais l'essai réel montre que le
chargement d'un disque échoue sans eux : se fier à l'observation, pas au `.info`.

Avec BIOS (**test `#[ignore]` manuel**, variable `EMU_TEST_MEGACD_BIOS` ⇒ BIOS **de l'utilisateur**, jamais
copié dans le dépôt, jamais téléchargé ; md5 `.info` : U `2efd74e3232ff260e371b99f84024f7f`,
E `e66fa1dc5820d254611fdcdba0662372`, J `278a9397d192149e84e820ac621a8edd`) :

- le BIOS puis le menu de la 240p Test Suite apparaissent : **empreinte d'image** après N images (N à mesurer :
  démarrage CD > 300 images, viser 600-1 200) ; la résolution et l'empreinte du menu sont stables par exécution
  mais **dépendent de la version du core nightly** (épingler le SHA-256 du `.so` ou tolérance) ;
- entrée : D-pad bas + bouton C/Start ⇒ le menu change (empreinte différente) ;
- son : les pistes audio CD-DA et le son de la suite sont lisibles dans le WAV de sortie (non silencieux) ; à
  mesurer, l'ordre de lecture des pistes audio dans la suite n'est pas documenté ;
- `.m3u` à deux disques : fabriqué en référençant deux fois le même `.cue` (test de `insert_next` ; pas de
  second disque libre).

Ce test n'a de valeur qu'avec le BIOS de l'utilisateur : le rendre **sauté proprement** quand la variable est absente.

### 1.3 Autres pistes Mega CD examinées (non retenues)

| Piste | Licence | Raison |
|---|---|---|
| `jvisser/md-mode1-mcd-asic-test` | CC0-1.0 | ROM « mode 1 » (cartouche + matériel CD) : exige aussi le BIOS ; essai sans résultat exploitable (étude). Peut compléter un test « avec BIOS ». |
| `matteusbeus/ModPlayer` (MIT), `WulfyStylez/megacd-vgm-player` (aucune licence), `drojaazu/mmd_loader` (aucune licence) | — | sources sans binaire ; licence absente pour deux sur trois |
| `paulomanrique/sega-32x-fmv` (MIT, « bootable Sega CD + 32X ») | MIT | requiert BIOS Mega CD **et** 32X ; contenu vidéo à vérifier ; non téléchargé |
| `RetailGameSourceCode/BatmanReturns` | aucune | code commercial : **à ne pas toucher** |

---

## 2. GBA (6.0, core mgba)

mgba fonctionne **sans BIOS** (BIOS HLE intégré, `gba_bios.bin` facultatif, jamais téléchargé).

| Fichier | URL (épinglée au commit) | Licence | Octets | SHA-256 | Test headless |
|---|---|---|---|---|---|
| `ppu/hello.gba` | `https://raw.githubusercontent.com/jsmolka/gba-tests/a7113b67e63f83a9b321696ddd7042ccfad6c881/ppu/hello.gba` | MIT (© 2019 Julian Smolka) | 1 300 | `38aed48b67bc0f701e8aa222b0c3334bd306bd29888707bb7224d81f5576c264` | « Hello world! » : empreinte à 240x160 après ~300 images (déjà prévu). Confirmé. |
| `240pee_mb.gba` | `https://github.com/pinobatch/240p-test-mini/releases/download/v0.23/240pee_mb.gba` | GPL-2.0 (même dépôt que `gb240p` et `240pee` déjà épinglés) | 61 104 | `47844f7140738a06f8f3bc09780da3ab095539a250b870f614feed561d9d6f34` | menu de la suite (multiboot, 240p test mini) : empreinte du menu ; entrée : bas + A change l'écran. **Épingle demandée par le design (« à épingler par GP1 ») : valeur ci-contre.** |
| `save/sram.gba` | `…/gba-tests/a7113b67…/save/sram.gba` | MIT | 2 084 | `a37ad99c31e3f805eb05a00e498b65bd78e6f43a0a139cd695bea1f88229af2c` | écrit en SRAM (marqueur `SRAM_V`) : après exécution, **`.srm` créé** (32 Kio) par le runtime ; test de la persistance (le design disait « aucun homebrew à sauvegarde » : voici un) |
| `save/flash64.gba` | idem | MIT | 3 708 | `7e2aa32e943aedde88bd750eadcdbf55152d3a1ec61385011b7f15cd85b07c02` | idem pour Flash 64 Kio |
| `save/flash128.gba` | idem | MIT | 4 096 | `9ac50e51d3ce4209dbdf85e472e70c067d5827e9af1bb3e707f6bd9059d5f0c6` | idem Flash 128 Kio (`.srm` de 128 Kio) |
| `save/none.gba` | idem | MIT | 1 628 | `edb34ba6590d070c8a50cf0f3566b1e3cc679377b978224ff1b872d27f2b1630` | sans sauvegarde : aucun `.srm` écrit (test négatif) |
| `ppu/shades.gba` | idem | MIT | 352 | `127ce348f17e4d5fd8a0821a7af757eda6109c66c9f7b8f7d1e0fa31831b407d` | dégradés : empreinte, couleurs 15 bits |
| `ppu/stripes.gba` | idem | MIT | 324 | `80d6cc05b5f289b9a2392621fc245270f08e9f0d77fd33982120cef240f5a504` | bandes : empreinte |
| `memory/memory.gba` | idem | MIT | 2 172 | `21024fb6aae6343f5f0466dd54e3149de1fbeb23f78e7d85a015c983684d2f87` | tests mémoire ; affiche le numéro du premier échec (mode 4) : à observer avant d'en faire un test « pass » |
| `nes/nes.gba` | idem | MIT | 1 808 | `d990df112763087d0415b3785c1b4d31c0237794a704d0446fc5f5e474a44f98` | idem (non vérifié « pass » ici) |
| `unsafe/unsafe.gba` | idem | MIT | 1 720 | `bb727d59fa81915a5f5c609f4befb64872d3a3d830bc6ae0149e26410e648f85` | idem |
| `arm/arm.gba`, `thumb/thumb.gba` | idem | MIT | 8 824 / 3 680 | `77ee8866…e5b` / `b5cb2291…9367` (déjà dans l'étude) | **« Failed test 522 / 230 » sans BIOS** (constaté) : à ne pas utiliser en « pass » ; utilisables en non-régression d'empreinte « écran d'échec » seulement |
| `bios/bios.gba` | idem | MIT | 1 908 | non calculé | demande le BIOS réel : exclu |

Tout a été re-téléchargé et recalculé aujourd'hui : l'URL et la licence de `hello.gba` tiennent (dépôt MIT,
dernier push 2025-04-19). Pour la 2.1, ajouter au minimum `sram.gba` + `none.gba` (persistance) et
`240pee_mb.gba`. Les SHA-256 des cinq ROM supplémentaires (shades, stripes, memory, nes, unsafe) sont à ne
retenir qu'après un essai réel de chacune sur le core (aucune n'a été lancée ici).

---

## 3. 32X (6.0, core picodrive)

Sans BIOS (BIOS 32X intégrés au core). **Aucune ROM 32X libre « officielle » n'était connue** (étude) ; une est
maintenant trouvée.

| Fichier | URL | Licence | Octets | SHA-256 | Remarque |
|---|---|---|---|---|---|
| `hexgl32x.0.0.1.32x` dans `hexgl32x.0.0.1.32x.zip` | `https://github.com/haroldo-ok/hexgl-32x/releases/download/v0.0.1/hexgl32x.0.0.1.32x.zip` | **MIT** (© 2026 Haroldo de Oliveira Pinheiro) ; le README précise que les textures sont **originales, procédurales** (les assets du HexGL d'origine, non commerciaux, ne sont pas utilisés) | zip 22 074 ; ROM 262 144 | zip `bb33c17a666f472e65a8cfd99559a98e908a59b99f8920be71830e274bfbdd2a` ; ROM `d01eb2db9b7c5193d4b120ff92c9f31ca6228855dd807d89939b70bf4854c7f2` | identique au fichier `hexgl32x.32x` du dépôt (commit `2aff78e0…`). Port 32X de jeu de course 3D (écran titre, 3 circuits, son PWM). **Non essayé sur picodrive** (consigne : ne pas exécuter) : à valider par l'agent d'intégration ; test headless proposé : empreinte de l'écran titre après ~300 images ; entrée : Start lance le compte à rebours (empreinte différente) ; son : bips non silencieux. Projet récent (2026), « vibe-port » en développement : épingler le SHA-256 de la ROM, pas la branche. |

Écartés : `viciious/d32xr` (licence mixte, code Jaguar Doom, WAD à fournir ; `D32XR.32x` de 32 768 o rejetée par le
core) ; `haroldo-ok/pail-court-of-demon-king-32x` et `raintown-slickers-32x` (le `.32x` contient des données
protégées de tiers, redistribution interdite) ; `viciious/roq32x` (MIT, ROM de 3,5 Mio mais **vidéo embarquée de
provenance non précisée**) ; `viciious/yatssd` (MIT, aucun binaire publié ; à compiler avec marsdev/32XDK).

Autres vérifications sans ROM : `probe_bin` sur un `.bin` 32X factice (`SEGA 32X` à 0x100, `mkcd.py`) ; contrôle
de firmware `Builtin` ; extension `.32x`.

---

## 4. DOS (4.0, core dosbox_pure)

Sans BIOS. Les ROM/BIOS sont inutiles (le core intègre un BIOS DOS). Les fichiers générés (`mkdos.py`,
`HELLO.COM`, `hello.zip`, `bat.zip`, `KEYS.COM`, `MOUSE.COM`, `MIDI.COM`) restent la base ; données tierces ci-dessous.

| Élément | URL | Licence | Octets | SHA-256 | Vérifié ici | Test headless |
|---|---|---|---|---|---|---|
| FreeDOS 1.4 Floppy Edition | `https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files/distributions/1.4/FD14-FloppyEdition.zip` | GPL-2.0 (noyau, FreeCOM) ; licence propre à chaque paquet (`Copying-Policy` du LSM) | 23 624 258 | zip `45b1fa7c52dd996c3bfa5e352ffcd410781b952a6ad629f15a4c9ec4bbaefc5a` ; `144m/x86BOOT.img` `552f7cbb0625960c050a3e682a4d2121cc2ffdfa9dc9d592ccd49edf179333dc` | URL en 200, taille et `last-modified` conformes ; SHA-256 conforme au `verify.txt` officiel du même dossier ; image non rouverte | démarrage de la disquette : écran FreeDOS (empreinte après ~2 500 images) ; fichier à extraire avec `extract_one` (limite 64 Mio de téléchargement OK) |
| Floppy Bird `flpybird.zip` | `https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files/repositories/1.4/games/flpybird.zip` | **MIT** (© 2014 Mihail Szabolcs ; `GAMES/FLPYBIRD/LICENSE`, LSM `Copying-Policy: MIT License`) | 2 073 159 | zip `a6f5c0c1bf5aa1746c9a335714c58835d4ce1ed90caa8de9072d9fb7f1d08ac9` ; `GAMES/FLPYBIRD/FLPYBIRD.COM` (8 704 o) `e9c7f236f8ac2dbe6ba40fd7f69563d47946837895e68d65a928d5a055eb8690` | téléchargé, recalculé : **conforme** | 320x200 : empreinte de l'écran titre ; entrée : Espace ⇒ l'oiseau monte (empreinte différente) ; le jeu est aléatoire : prendre l'empreinte avant l'entrée ou seulement la résolution |
| AttoChess `ATTOCHES.COM` | `https://github.com/Nicholas-afk/AttoChess/releases/download/v272/ATTOCHES.COM` | **MIT** (© 2026 Nicholas Tanner) | 272 | `02b80d2b11ba3c6bea02b112e7009c318aac35b3e869f8bb823dc2aa6cdd6f8e` | téléchargé | mode texte : échiquier affiché (empreinte déterministe, sans hasard) ; entrée : saisie « e2e4 » + Entrée ⇒ le moteur répond (échiquier modifié). Excellent test clavier déterministe, minuscule (aucun zip nécessaire : le placer dans un zip ou un dossier de jeu). |
| matrix62 (`matrix62.com`) | `https://github.com/xeleh/sizecoding/releases/download/matrix62/matrix62.zip` | MIT (`LICENSE` du dépôt `xeleh/sizecoding`, © 2025) | zip 31 957 ; COM 62 | zip `9c4210dc15554e03940c73704e6193e8132faddc2feb206d25ca35f1df45c6e9` ; COM `0bab8a8f5dff2f5b28b45ffdfa716e7d60f5a20e4b1b718ebeac821e0a5f54c4` | téléchargé | animation (« pluie matricielle » : probablement non déterministe) : à n'utiliser qu'en test de **« écran non uniforme / image qui change »**, pas en empreinte exacte ; optionnel |
| KRaptor `kraptor.zip` | `…/repositories/1.4/games/kraptor.zip` | MIT (LSM) | 10 377 798 | `d14e3a780a488be427dbaa577aef8e878f2d62e8b6ab516902c4e9fc99c929cd` (étude) | non retéléchargé | exige `CWSDPMI.EXE`, non récupéré : écarté |
| FreeDOS LiveCD / LegacyCD (test iso/cue) | `…/distributions/1.4/FD14-LiveCD.zip` | GPL-2.0 et licences par paquet | 293 950 337 | `2020ff6b…f6339` (confirmé par `verify.txt`) | `verify.txt` seulement | trop volumineux pour la CI (disque limité) : test ISO couvert par un ISO généré ; LiveCD en essai manuel |

Écartés : `mtuomi/SecondReality` (Unlicense, mais dépôt de sources : seul `U2.EXE` (31 548 o, chargeur) est
précompilé, les parties du démo sont à construire avec la chaîne d'origine : inutilisable tel quel) ;
`baAlex/Sakurai` (MIT, jeu DOS réel mais licence des assets non vérifiée) ; Doom shareware, jeux shareware,
MS-DOS, Windows 9x : **interdits** (non libres / commerciaux).

Son : GeneralUser GS 2.0.3 (déjà prévu, §3.3 du design) pour `MIDI.COM` : aucun contenu supplémentaire.

---

## 5. Amstrad CPC (5.0, core cap32)

Le design n'avait retenu **aucun homebrew tiers** (licences non vérifiables) ; deux sont maintenant vérifiés.
Pas de ROM à fournir (ROM Amstrad intégrées au core, déjà notées dans l'étude).

| Fichier | URL (commit épinglé) | Licence | Octets | SHA-256 | Test headless |
|---|---|---|---|---|---|
| `c/rpg/rpg.dsk` | `https://raw.githubusercontent.com/AmstradGameDevChallenge/rpg-adventure/e6b697b142bc4cf4c1c726b56308df3c9812a7be/c/rpg/rpg.dsk` | **MIT** (© 2019 Diego Freniche Brito ; compilé avec CPCtelera, lui-même libre) | 204 544 | `475870381873dcc570881b792df9ba0ffe2c58d404694769e10639f58505c813` | disque contenant `RPG.BIN` + `PRESENT.TXT` : lancement via `RUN"RPG` (frappé, ou `#COMMAND:` d'un `.m3u`) ⇒ écran du jeu (« Move (O/P/Q/A) … »), empreinte ; entrées : O/P/Q/A déplacent le joueur ; **l'autorun du disque n'est pas vérifié** |
| `c/rpg/rpg.cdt` | idem `…/c/rpg/rpg.cdt` | MIT | 8 881 | `48f5c86810ea245180032d92fbc65e00af8487f13ad0126377819b42fe08b1d7` | cassette : chargement (avance rapide demandée puis rendue) puis écran du jeu |
| `c/rpg/rpg.sna` | idem `…/c/rpg/rpg.sna` | MIT | 65 792 | `e6050313a8b8de30c020d5208c6bff1076f10b438bba82163b0311642bcc3aac` | **seul contenu `.sna` libre** : snapshot 64 Ko, démarre directement : empreinte de l'écran du jeu ; comble le « `.sna` sans donnée de test » du design |
| `basic/rpg.dsk` | idem `…/basic/rpg.dsk` | MIT | 204 544 | `118b3e8520f249b1ede4a90e33ef5b0b926e1825d2ba0b5a97d71cfcd2b88daa` | version BASIC du même jeu (listage lisible, `RUN"RPG`) ; deuxième disque pour `.m3u` / `insert_next` |
| `DskTest.dsk` (+ `.cdt`) | `https://github.com/pacomix/DskTest/releases/download/2.2/DskTest.dsk` | **GPL-3.0** (`LICENSE` du dépôt) | 204 544 (`.cdt` 2 386) | dsk `47274d124d215b525f970d0977dec8a76b407de1ba1301331c547ee026c8c35a` ; cdt `4baf19a7928710e0c1bac2046f14b8b0a62fed7e1972fee7cb0569b4d822dd7d` | outil de diagnostic de lecteur de disquette (menu, touches 1 et 2) : optionnel ; moins utile (dépend d'un vrai lecteur) |

Remarque : les URL « raw » sont épinglées au commit `e6b697b…` (2019-08-05, dépôt figé). `rpg-adventure` n'a pas de
release. Le contenu `.cpr` (CPC+) n'a toujours **aucune** donnée libre trouvée ; l'extension reste testée par
unité.

Écartés : `FabrizioRadica/castledungeoncpc`, `fragarco/dungeon-race`, `jburnhams/bomber` (MIT mais sources `.bas`
seulement, sans disque) ; `edugomz/aliens_n_tanks` (MIT, ni binaire ni release).

---

## 6. Commodore 64 (5.0, core vice_x64)

Les ROM (KERNAL, BASIC, chargen) sont intégrées au core VICE (« ROMs C64 de VICE acceptées », décision utilisateur).
Aucun BIOS à fournir.

| Fichier | URL (commit épinglé) | Licence | Octets | SHA-256 | Test headless |
|---|---|---|---|---|---|
| `uberswap.prg` | `https://raw.githubusercontent.com/EgonOlsen71/uberswap/f94035dcdb714885b21edd9b342decc6b3872a4a/build/uberswap.prg` | **Unlicense / domaine public** (`LICENSE.txt` : « This is free and unencumbered software released into the public domain ») ; BASIC compilé avec MOSpeed (même auteur, `basicv2`, Unlicense) | 25 475 | `ddef3ab6a93095c71eb0289496ad06ddf7b35f838b4d5bf6b1b4939b6da9e7b7` | `.prg` (autostart) : écran titre puis grille de pièces colorées ; entrée : joystick/clavier (déplacements, `x` quitte) ⇒ empreinte différente ; couvre `.prg` libre tiers et le mode joystick |
| `uberswap-c.prg` | idem `…/build/uberswap-c.prg` | Unlicense | 20 275 | `0eb809460b6811eb2a12ae56106683a79a72630191ff79f56fbec58e2c129391` | variante compressée (décompresseur intégré) : même affichage, utile pour comparer |
| `kidelyneen_AD.d64` | `https://raw.githubusercontent.com/EgonOlsen71/Kidelyneen/05a10bd1437f491b5555136e26f5c651945c0666/build/kidelyneen_AD.d64` | Unlicense (`LICENSE` : même texte de domaine public) | 174 848 | `9a151c24619ddc49506f1cca53297c63a6f2c42185f803b091b2f77637eb3bfd` | disque `.d64` libre tiers (Qix en BASIC compilé) : `LOAD"*",8,1` / autostart ⇒ écran titre ; chemin disque réel |
| `Wild.Snake.Boa.d64` | `https://github.com/tstamborski/Wild-Boa-Snake/releases/download/1.0/Wild.Snake.Boa.d64` | **MIT** (© 2021 Tobiasz Stamborski) | 174 848 | `3139cf2d0097104c6414d6872878d8b0a505ee21de0dc89b5a2a5b6663eca07f` | `.d64` ; jeu Snake : écran de jeu, entrée par joystick |

Notes : les cinq dépôts d'Egon Olsen sont publiés sous Unlicense (le plus clair des licences trouvées). Les
supports générés (`mkc64.py` : `hello.prg`, `hello.d64`, `hello.tap`) restent les tests déterministes de référence
(mêmes SHA-256 que l'étude) ; ces homebrews tiers servent de test « vrai programme » et de **fumée** (écran non
vide / image qui change), pas de test d'empreinte exacte tant qu'ils n'ont pas été lancés sur le core. Aucun n'a
été exécuté ici.

Écartés : `libsidplayfp/VICE-testprogs` et `vpelletier/vice-emu-testprogs` (miroirs **sans licence déclarée**,
contenu hétérogène : aucune garantie de redistribution) ; `Retro-Emotions/C64-Video-Diagnostic-Test-Suite`
(licence « NOASSERTION ») ; démos `csdb.dk` / Lemon64 (licences non vérifiables, comme noté dans l'étude).
Aucun contenu libre trouvé pour `.crt`, `.t64`, `.tap` tiers : les supports générés restent la seule source.

---

## 7. Entrées des designs dont l'URL ou la licence ne tient plus ou demande correction

| Document | Entrée | Constat |
|---|---|---|
| `v6.0-gba-32x-megacd.md` §5 | « 32X : aucune ROM libre démarrable » | **Dépassé** : `hexgl32x` (MIT) trouvée (non essayée sur le core). |
| `v6.0` §5 | « Mega CD : aucun disque libre utilisable sans BIOS ; test réel par l'utilisateur » | **À nuancer** : un disque libre existe (240p Test Suite Sega CD, GPL-2.0, cue + iso + wav) ; il reste inutilisable **sans BIOS** pour le rendu, mais permet les tests de bibliothèque, de région et de message d'erreur sur du vrai contenu. |
| `v6.0` §5 | `240pee_mb.gba` « SHA-256 à épingler par GP1 » | Épinglé ici : `47844f7140738a06f8f3bc09780da3ab095539a250b870f614feed561d9d6f34` (61 104 o). |
| `v6.0` §5 / étude §2 | SRAM : « aucun homebrew à sauvegarde » | `sram.gba`, `flash64.gba`, `flash128.gba`, `none.gba` de `jsmolka/gba-tests` couvrent la persistance (MIT). |
| `v5.0` §6 | « Homebrew tiers : aucun retenu » ; `.sna` sans donnée | `rpg-adventure` (MIT) fournit `.dsk`, `.cdt` et `.sna` ; C64 : dépôts Unlicense / MIT. |
| `v4.0` §6 | FreeDOS, Floppy Bird | **Tiennent** : URL, tailles et SHA-256 confirmés (FreeDOS par `verify.txt` officiel, Floppy Bird par téléchargement) ; licences confirmées (MIT dans le LSM et le fichier `LICENSE`). |
| `v6.0` §5 | `hello.gba` (jsmolka) | **Tient** : URL au commit, MIT, SHA-256 inchangé. |
| URL SourceForge (`/download`) | schéma déjà utilisé pour la suite 240p | Fonctionne ; dépendance à un miroir SourceForge : garder l'épingle SHA-256 et un message d'erreur clair en cas d'échec réseau. |

## 8. Points d'attention pour `platform-build`

- Tous les nouveaux zips : conserver `--proto '=https'`, `--max-filesize 64M`, SHA-256 épinglé **avant** extraction,
  extraire uniquement les membres attendus (`extract_one`) ; le zip Sega CD contient 8 membres, dont 3 de 32 MiB.
- URL `raw.githubusercontent.com/<dépôt>/<commit>/…` : immuables ; préférées aux URL de branche.
- URL de release GitHub (`hexgl-32x`, `AttoChess`, `Wild-Boa-Snake`, `DskTest`) : peuvent être supprimées ou
  réécrites par l'auteur ; le SHA-256 protège, mais prévoir un test « sauté proprement » si l'URL disparaît.
- Aucun BIOS ni ROM commerciale n'est téléchargé ; `EMU_TEST_MEGACD_BIOS` (et équivalents) pointent vers les fichiers
  de l'utilisateur.
- Licences à ajouter à `THIRD-PARTY-DATA.txt` (si ces fichiers y sont mentionnés) : GPL-2.0 (240p Sega CD), MIT
  (gba-tests, hexgl32x, rpg-adventure, AttoChess, matrix62, Wild-Boa-Snake, Floppy Bird), Unlicense
  (uberswap, Kidelyneen), GPL-3.0 (DskTest), GPL-2.0 (FreeDOS et ses paquets).
