# ADR 0016 — Manette (DualShock 4) : bibliothèque, mapping, configuration

Statut : accepté (2026-11-20) avec les défauts recommandés de `docs/design/phase6-manette.md` §6. **Version de
livraison : 2.1** (décision de l'utilisateur du 2026-11-21 : T2 à T7 juste après la 2.0 ; T0 et T1 sont faits).
**Amendée le 2026-11-21** (section « Amendement 2026-11-21 » en fin de document) : champ `analog = true` de
`cores.toml`, raccourcis manette du mode ordinateur, colonne GBA du mapping. **Amendement 2026-11-23 (7.2, accepté)** : manettes non
reconnues ou mal mappées, apprentissage par GUID (remplace le point 5 « ignorée avec un `warn!` »). Précise ADR 0003 (« backend SDL3
candidat, à confirmer par une ADR dédiée »). Conception détaillée et découpage : `docs/design/phase6-manette.md`.

## Contexte

Phase 6 : jouer à la manette PS4 (DualShock 4, USB et Bluetooth) sous Windows et macOS Intel 12.7.6 (cibles de
l'utilisateur), Linux, et si possible Docker. Le clavier reste utilisable en parallèle (contrainte CLAUDE.md : jamais
d'hypothèse « clavier seulement »). Le conteneur de dev n'a pas `/dev/input` : tout doit être testable sans matériel.

État du code : `emu_runtime::InputBackend::poll(frame_index, &mut InputFrame)` ; les backends combinent par OU
(`crates/emu-runtime/src/input.rs`) ; `PadState` = 16 boutons numériques, `MAX_PORTS = 2`. Le clavier est
`emu_ui::KeyboardInput` + `KeyMap` (`keyboard.rs`), config `[input.keyboard.<profil>]` où le profil est
`PadStyle::profile_key` (id du core, suffixé `-sms`, `-gg`... hors système principal). Raccourcis : `HotkeyAction`
(`hotkeys.rs`), clavier uniquement.

## Options examinées

Faits vérifiés le 2026-11-20 avec `cargo info` et les sources téléchargées (crates.io / docs.rs) :

| | `gilrs` 0.11.2 (+ `gilrs-core` 0.6.8) | `sdl3` 0.22.1 (+ `sdl3-sys` 0.7.3, SDL 3.4.20) | `sdl2` 0.39.0 | `hidapi` 2.6.7 |
|---|---|---|---|---|
| Licence | Apache-2.0 / MIT | crate MIT ; SDL3 : Zlib | MIT (SDL2 : Zlib) | MIT |
| Code | Rust pur | liaison C : lib native (système, vcpkg ou `build-from-source` via cmake + compilateur C) | idem (SDL2) | C (hidapi) |
| Linux | evdev + inotify + `libudev-sys` (build : `libudev-dev`) | HIDAPI/evdev | idem | hidraw |
| Windows | **WGI** (`Windows.Gaming.Input.RawGameController`, défaut) ou XInput | HIDAPI (DS4 natif), XInput, WGI | idem | oui |
| macOS | IOKit HID (`objc2-io-kit`) | HIDAPI / GameController.framework | idem | oui |
| Mapping vers boutons « standard » | base SDL_GameControllerDB embarquée (licence Zlib), `Button::South/East/...` | intégré | intégré | aucun (rapports HID bruts à décoder soi-même) |
| Hotplug, GUID | oui (`Connected`/`Disconnected`, `uuid()`) | oui | oui | à écrire |
| Vibration, gyro, tactile, LED | vibration (`ff`) ; pas de gyro/tactile | tout | tout | tout (à la main) |

Sources : https://crates.io/crates/gilrs, https://crates.io/crates/gilrs-core, https://crates.io/crates/sdl3,
https://crates.io/crates/sdl3-sys, https://crates.io/crates/sdl2, https://crates.io/crates/hidapi ;
`Cargo.toml` et `src/platform/*` de `gilrs-core 0.6.8` (backends, dépendances par cible) ; en-tête de
`SDL_GameControllerDB/LICENSE` de `gilrs 0.11.2` (Zlib, © Sam Lantinga).

## Décision

1. **`gilrs 0.11.2`** (défaut `wgi` sous Windows), derrière une feature Cargo `gamepad` (activée par défaut dans
   `emu-app`, relayée `emu-app → emu-ui`), comme `audio-cpal`. Raisons : Rust pur (pas de chaîne C/cmake
   supplémentaire pour les 4 builds, la cross-compilation Windows et l'image Docker), licence compatible avec
   MIT OR Apache-2.0, hotplug et GUID fournis, mapping DS4 → boutons nommés déjà dans la base SDL embarquée.
   `libudev-dev` s'ajoute aux dépendances de **build** Linux (image dev, étage de build, CI) ; `libudev1` au runtime
   de l'image Docker (déjà présent dans la plupart des bases Debian, à vérifier).
2. **Repli prévu : `sdl3`** (features `build-from-source-static`) si l'essai réel montre que gilrs échoue sur la DS4
   (risque identifié : Windows/WGI, où l'identité de la manette est reconstruite depuis VID/PID du
   `RawGameController` ; macOS 12 en Bluetooth). Le repli ne touche qu'**un fichier** : tout le code applicatif
   passe par le trait `GamepadSource` (ci-dessous). Seuil de bascule : DS4 non reconnue ou mal mappée en USB **ou**
   Bluetooth sur Windows ou macOS, après vérification de l'absence de DS4Windows/Steam Input.
3. **Aucun accès direct HID** (`hidapi`) : on ne réécrit pas les pilotes. Les capacités propres à la DS4 (gyro, pavé
   tactile, barre lumineuse) sont **hors périmètre** ; la **vibration** aussi (gilrs `ff` est limité) : possible
   amendement ultérieur si l'utilisateur le demande (les cores libretro la demandent via
   `RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE`, aujourd'hui refusé).
4. **Architecture** (graphe de crates inchangé, pas de nouveau crate) :
   - `emu-runtime` : inchangé pour le numérique ; ajout optionnel des sticks analogiques à `PadState`
     (`RETRO_DEVICE_ANALOG`), nécessaire seulement aux cores 3D (hors 1.x/2.0) : **différé**, voir design
     (posé en 4.0 pour dosbox_pure : amendement du 2026-11-21, point 2).
   - `emu-ui` : module `gamepad/` : trait `GamepadSource` (événements normalisés, **injectable**), implémentation
     `GilrsSource` (seul code touchant gilrs), `GamepadInput: InputBackend` (état → `InputFrame`), table de mapping,
     gestion des ports et des raccourcis. L'API est entièrement sûre ; aucun `unsafe` (gilrs l'encapsule).
   - `emu-app` : option CLI/config, activation de la feature.
5. **Normalisation** : `GamepadSource` produit des `PadEvent { id: PadId, kind }` avec des boutons **nommés
   positionnellement** (`Pad::South/East/West/North, L1, R1, L2, R2, Select, Start, Mode, L3, R3, DPad*`) et des axes
   `[-1, 1]`. Cross = South, Circle = East, Square = West, Triangle = North, Share = Select, Options = Start,
   PS = Mode. Un pad non reconnu par la base de mapping (`MappingSource::None`) est ignoré avec un `warn!`
   (pas de devinette d'indices bruts).

## Mapping DS4 → RetroPad (défauts, positionnel)

RetroPad = disposition SNES : B bas, A droite, Y gauche, X haut.

| DS4 | RetroPad | Mega Drive (genesis_plus_gx) | SMS / Game Gear | NES / Game Boy | SNES | GBA (6.0, `PadStyle::Gba`) |
|---|---|---|---|---|---|---|
| Croix | B | B | 1 | B | B | B |
| Rond | A | C | 2 | A | A | A |
| Carré | Y | A | (rien) | (rien) | Y | (rien) |
| Triangle | X | Y | (rien) | (rien) | X | (rien) |
| L1 / R1 | L / R | X / Z | (rien) | (rien) | L / R | L / R |
| L2 / R2 | L2 / R2 (seuil analogique 0,5) | (rien) | (rien) | (rien) | (rien) | (rien) |
| Share | Select | Mode | (rien) | Select | Select | Select |
| Options | Start | Start | Pause (SMS) / Start (GG) | Start | Start | Start |
| Croix directionnelle | D-pad | idem | idem | idem | idem | idem |
| Stick gauche | D-pad (seuil 0,5, hystérésis) — sauf core `analog = true` (amendement) | idem | idem | idem | idem | idem |
| L3 / R3, PS, pavé tactile | libres (PS + L3 / R3 : raccourcis du mode ordinateur, amendement) | — | — | — | — | — |

Les noms de boutons affichés dans l'UI suivent `PadStyle::button_name` (déjà par système). Le mapping par défaut est
codé par `PadStyle` ; les profils se nomment comme le clavier (`PadStyle::profile_key`) pour que chaque système garde sa
disposition. Le stick droit est inutilisé tant qu'aucun core analogique n'est pris en charge.

## Coexistence clavier + manette, ports, hotplug

- Les backends combinent par OU : clavier et manette fonctionnent **simultanément**, sans bascule de mode.
- Le clavier garde ses ports configurés (joueur 1 par défaut, joueur 2 possible via `KeyMap`).
- Attribution des ports : la première manette connectée prend le **port 0**, la deuxième le **port 1**, les suivantes
  sont ignorées (`MAX_PORTS = 2`). Un port reste attribué tant que sa manette est connectée ; un port libéré est
  réutilisé par la prochaine connexion. Surcharge possible par GUID (voir config).
- Hotplug : connexion/déconnexion à chaud, jamais de panique ; à la déconnexion tous les boutons de ce port sont
  relâchés, une notification « Manette déconnectée » s'affiche et l'émulation **continue** (option : pause
  automatique, désactivée par défaut). Deux manettes identiques ont le même GUID : seul l'ordre de connexion les distingue.
- Au démarrage : manettes déjà branchées détectées via l'itération initiale de gilrs.

## Deadzone

Défaut 0,25 sur les axes (cercle, appliquée avant le seuil D-pad de 0,5 avec hystérésis 0,1) ; réglable 0,05–0,60 dans
la config et l'UI. Les gâchettes L2/R2 deviennent numériques au-delà de 0,5.

## Raccourcis manette

Bouton **PS (Mode)** maintenu = modificateur : tant qu'il est enfoncé, les boutons ne vont **pas** au core et déclenchent
des `HotkeyAction` (mêmes actions que le clavier, sans nouvelle logique de pause/vitesse) :

| PS + | Action |
|---|---|
| Options | Menu |
| Share | Pause |
| R1 / L1 | Sauvegarder / charger l'état |
| Droite / Gauche | Emplacement suivant / précédent |
| R2 (maintenu) | Avance rapide |
| Triangle | Plein écran |
| Carré | Reset |
| Croix | Statistiques |

Appui bref sur PS sans autre bouton : rien (évite un menu accidentel). Le bouton modificateur est configurable
(`hotkey_button = "mode" | "select"`) car le système peut capturer PS (à vérifier sur macOS 12 et Windows : Steam, Xbox
Game Bar) ; repli « Share » + autre bouton. Les actions suivent le comportement existant (une pression = une action, sauf
avance rapide).

## Configuration et persistance (`config.toml`, ADR 0011)

Extension de `InputConfig` ; tout est facultatif et rétrocompatible (clés inconnues conservées via `extra`,
fichier toujours en 0600) :

```toml
[input.gamepad]
enabled = true            # défaut
deadzone = 0.25
trigger_threshold = 0.5
hotkey_button = "mode"
pause_on_disconnect = false

# Surcharge de boutons par profil (même clés que le clavier : id du core, ou core-système)
[input.gamepad.bindings.genesis_plus_gx]
south = "b"   # bouton physique (nommage positionnel) -> bouton RetroPad ; partiel : le reste garde le défaut

# Par type de manette (GUID gilrs en hexadécimal) : port forcé, deadzone propre
[input.gamepad.devices."030000004c050000c405000000000000"]
port = 1
deadzone = 0.15
```

- Défauts = tableau ci-dessus ; une table absente ou vide donne le comportement par défaut (pas d'écriture au démarrage).
- Remappage dans l'UI : même écran que le clavier (« appuyez sur un bouton »), écrit `bindings.<profil>`. Un profil par
  GUID de mapping (pas par instance) évite de dépendre de numéros d'appareil instables.
- Pas de secret ; rien à journaliser au-delà du nom et du GUID de la manette (niveau `info`).

## Tests sans matériel

- Le trait `GamepadSource` est injectable : `ScriptedPadSource` (liste d'événements par frame) pour les tests unitaires
  et d'intégration ; `GilrsSource` est le seul code non testable sans matériel et reste très fin (conversion
  événement → `PadEvent`).
- Cas obligatoires : mapping par système, deadzone et hystérésis, gâchettes, D-pad vs stick, OU avec le clavier,
  attribution et réutilisation des ports, déconnexion (boutons relâchés), raccourcis PS + bouton (le core ne voit pas
  les boutons), aller-retour de config (TOML) et rétrocompatibilité, `ScriptedPadSource` pilotant le core factice
  (empreintes d'image comme `ScriptedInput` aujourd'hui).
- Dans le conteneur de dev : `cargo test` ne doit **jamais** initialiser gilrs (sans `/dev/input`) ; `GilrsSource::new`
  renvoie une erreur gérée (log + backend vide), testée.

## Plateformes

- **Windows (priorité)** : gilrs/WGI. DS4 en USB et en Bluetooth sont vues comme `RawGameController`. Sans pilote
  tiers. DS4Windows/Steam Input émulent une manette Xbox : fonctionne aussi mais peut doubler les événements ; documenté.
  Les événements WGI n'arrivent que fenêtre active : acceptable.
- **macOS Intel 12.7.6** : gilrs/IOKit HID, DS4 en USB et en Bluetooth (appairage par les Réglages Système). Support
  attendu mais **non vérifié** ici (pas de macOS) : essai utilisateur obligatoire. Pas d'autorisation « Surveillance des
  saisies » attendue pour IOKit HID manette (à confirmer à l'essai ; si demandée, le documenter).
- **Linux** : le noyau (`hid-sony`) expose la DS4 en evdev ; gilrs la lit. Droits sur `/dev/input/event*` (groupe `input`).
- **Docker** : *best effort*, hors critère de « terminé ». Il faut passer `/dev/input` (et udev) au conteneur ; sous
  WSL2 les manettes USB demandent `usbipd-win` et un noyau WSL avec les modules HID/evdev, pas disponible pour le Bluetooth ;
  **noVNC ne relaie aucune manette** (navigateur). Recommandation documentée : manette sous Windows/macOS natifs ;
  Docker = clavier. Option `compose` `devices: ["/dev/input:/dev/input"]` fournie en commentaire, non testée.

## Conséquences

- Nouvelle dépendance Rust pure `gilrs` ; `libudev-dev` en build Linux ; `THIRD-PARTY` (licences, dont la base SDL
  Zlib) à régénérer et revue du `reviewer` (nouvelle dépendance, pas de `unsafe` de notre côté, pas de réseau).
- `Config` : nouvelle section `input.gamepad` ; `InputConfig` reste rétrocompatible.
- L'ADR 0003 reste valable ; sa mention « SDL3 candidat » est remplacée par celle-ci (SDL3 = repli).
- Hors périmètre actuel : vibration, gyro, tactile, LED, stick analogique pour cores 3D, manettes non DS4 hors
  mapping SDL automatique (elles fonctionnent si gilrs les mappe, sans garantie), souris/clavier émulés par manette.

## Amendement 2026-11-21 — revue croisée des conceptions 3.0 à 6.0

1. **Version** : la manette est livrée en **2.1** (T2 à T7). La 4.0 (tâche DG1, axes analogiques) et la 6.0 (mapping
   GBA) en dépendent ; sans 2.1 livrée, ces tâches sont reportées sans bloquer leur version.
2. **Axes analogiques : champ `analog = true` de `cores.toml`** (posé en 4.0 avec `PadState::set_analog`, ADR 0018
   §7). La règle « stick gauche ⇒ D-pad, toujours » de cette ADR devient le **défaut** ; pour un core marqué
   `analog = true` (dosbox_pure ; plus tard les cores 3D), la conversion stick ⇒ D-pad est **coupée** et les deux
   sticks sont transmis au core par `RETRO_DEVICE_ANALOG` (la croix directionnelle reste le D-pad). **Zone morte
   commune** : la même valeur `deadzone` (défaut 0,25, radiale, par manette si surchargée) s'applique dans les deux
   cas ; en mode analogique la valeur est remise à l'échelle au-delà de la zone morte (pas de saut à 0,25) puis
   convertie en `i16` ; le seuil 0,5 et l'hystérésis ne servent qu'à la conversion en D-pad. Les gâchettes restent
   numériques (seuil 0,5). Pas de réglage par jeu en 4.0 (le core choisit lui-même numérique ou analogique dans ses
   préréglages).
3. **Raccourcis manette du mode ordinateur** : ADR 0015 §9 proposait « PS + R3 = clavier virtuel », le design de la
   phase 6 laissait le clavier virtuel et la disquette suivante au clavier seul. **Décision : ADR 0015 l'emporte.**
   PS + **R3** = clavier virtuel (`ToggleVirtualKeyboard`), PS + **L3** = disquette / CD suivant (`NextDisk`), liés
   en 2.1 (tâche T3) ; clavier virtuel ouvert : croix directionnelle + Croix naviguent et tapent, le pad ne va plus
   au core (design 2.0 §8). Raison : le clavier virtuel est justement le moyen d'atteindre les touches sans
   clavier ; un joueur à la manette doit pouvoir l'ouvrir et changer de disquette sans reposer la manette.
   `ToggleMouseCapture` et `ToggleKeyboardMode` restent au clavier (sans objet à la manette). Aucun conflit avec les
   cores `analog = true` ni avec le menu interne de dosbox_pure (L3 seul) : tant que PS est tenu, rien ne va au
   core.

   | PS + | Action (systèmes `kind = "computer"`) |
   |---|---|
   | R3 | Clavier virtuel |
   | L3 | Disquette / CD suivant |

4. **GBA (6.0)** : colonne ajoutée au tableau de mapping ; `PadStyle::Gba` doit être traité dans
   `crates/emu-ui/src/gamepad/mapping.rs` (tâche GU1 de la 6.0).

## Sources à revérifier avant implémentation

Comportement réel de gilrs-core 0.6.8 sur DS4 (WGI Windows, IOKit macOS 12) : seul l'essai sur matériel tranche ;
la base SDL_GameControllerDB embarquée contient bien les GUID DS4 (v1/v2, USB/BT) par plateforme : à contrôler par un
test lisant `gilrs::MappingSource` sur un GUID DS4 construit à la main côté `qa-systems`.

## Amendement 2026-11-23 — manettes non reconnues (7.2, statut : accepté le 2026-11-23, défauts validés par l'utilisateur)

Conception et découpage : `docs/design/v7.2-manette-snes.md`. Demande de l'utilisateur : pads USB génériques type
SNES (DragonRise 0079:0011 et similaires), souvent absents de la base SDL embarquée **ou présents mais mal mappés**
(une puce, plusieurs câblages, une seule ligne par GUID), croix rapportée en axes, en hat ou en boutons.

1. **Remplace la dernière phrase du point 5** : une manette `MappingSource::None` n'est plus ignorée. Elle est lue
   en **mode brut** (entrées natives opaques `RawInput::Button(code)` / `RawInput::Axis(code)`, codes gilrs
   `Code::into_u32`) ; le mode brut s'applique aussi à tout modèle (GUID) qui a un mapping appris, qui **prime sur la
   base SDL**. Toujours pas de devinette d'indices : sans mapping appris, le pad ne va jamais au core et ne prend
   pas de port ; l'utilisateur est averti une fois par connexion.
2. **Apprentissage dans l'UI** (Réglages → Manettes) : une invite par touche, touche passable, forme choisie
   (SNES, NES, Mega Drive 3 / 6 boutons) qui ne change que les invites ; résultat positionnel (`PadButton`) donc
   `PadMap` par `PadStyle`, ports, raccourcis et menus inchangés. Axes et hat deviennent des boutons (seuil 0,5,
   relâchement sous 0,4). Sticks analogiques non appris (hors 7.2).
3. **Persistance** dans `[input.gamepad.devices."<guid>"]` : `name`, `layout`, `hotkey_button`, table `map`
   (`<bouton positionnel> = "b12" | "+a3" | "-a3"`) ; champs facultatifs, rétrocompatible, 0600. Codes valables pour
   une plateforme (le GUID diffère d'ailleurs d'une plateforme à l'autre).
4. **Bouton de raccourcis par manette** : `hotkey_button` du périphérique, sinon **Select** pour un pad appris sans
   bouton `Mode`, sinon la valeur globale ; la DS4 garde PS.
5. **Format SDL_GameControllerDB écarté** : gilrs expose des `Code` natifs et non les indices SDL ; produire une
   ligne SDL reproduirait sa numérotation interne et dépendrait de `Gilrs::set_mapping`. Notre couche reste
   indépendante du backend (le repli SDL3 du point 2 la conserve) et testable sans matériel.
6. Graphe de crates inchangé ; `emu-runtime` inchangé ; code dans `emu-ui/src/gamepad/` ; aucun `unsafe`, aucun
   réseau ; journal : nom et GUID en `info`, codes bruts en `debug` seulement.

**Non vérifié** (sources gilrs absentes du registre local, aucun téléchargement) : présence de 0079:0011, 0810:e501,
081f:e401, 12bd:d015 dans la base embarquée par plateforme ; forme exacte des événements d'un pad non mappé et du hat
sous macOS / Windows. À contrôler par `qa-systems` (tâche Q1) et à l'essai ; la décision ne dépend pas du résultat.
