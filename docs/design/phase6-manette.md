# Phase 6 — Manette PS4 (DualShock 4) : conception et découpage

Décisions : `docs/adr/0016-manette.md` (statut « proposé » jusqu'aux réponses de l'utilisateur, section Questions).
Livrable : version après 1.0 (ordre par rapport à la 2.0 Atari ST à décider ; les deux sont indépendantes, seul
`docs/roadmap.md` et `config.rs` sont des points de contact éventuels).

## 1. Interfaces publiques (posées par T0, corps `todo!("T1"/"T2")`)

Fichiers (`crates/emu-ui/src/`) : `gamepad/mod.rs`, `gamepad/source.rs` (`PadId`, `PadAxis`, `PadEventKind`, `PadEvent`,
`GamepadError`, trait `GamepadSource`, `ScriptedPadSource::{new, push_batch}`), `gamepad/mapping.rs` (`PadButton`,
`PadMap`), `gamepad/input.rs` (`GamepadSettings`, `GamepadNotice`, `GamepadInput<S>`, ports + hotkeys inclus),
`gamepad/gilrs_source.rs` (`GilrsSource`, `#[cfg(feature = "gamepad")]`), `config_gamepad.rs` (`GamepadConfig`,
`GamepadDeviceConfig`, `HotkeyButton`, serde, `extra` flatten). `mod.rs` porte `#![allow(dead_code, unused_imports)]`
jusqu'au branchement (T3, qui le retire). Ajustements par rapport au projet initial : `HotkeyButton` vit dans
`config_gamepad.rs` ; `GamepadNotice` est une enum à champs nommés ; `GamepadInput` gagne `set_map`,
`fast_forward_held()` (l'avance rapide est une action **maintenue** : PS + R2, hors de `take_hotkeys`) et `attached()` ;
`GamepadSettings::from_config`. `GamepadConfig` n'est PAS encore champ de `InputConfig` (T3, `config.rs` réservé à U2).

Contrat des sources : une manette sans mapping de boutons (`MappingSource::None`) n'émet aucun événement ; les manettes
déjà branchées sont annoncées `Connected` au premier `poll_events`.

Compatibilité 2.0 (vérifiée sur U1) : `ComputerKeyboard`/`ComputerMouse`/`KeyboardInput` implémentent `InputBackend`
(OU dans `InputFrame`) ; `GamepadInput` s'ajoute comme backend supplémentaire, sans changement d'`emu-runtime`. Les
raccourcis manette produisent des `HotkeyAction` ; en mode ordinateur `app.rs` les enveloppe dans
`HostAction::Hotkey(..)` (`ToggleVirtualKeyboard`/`NextDisk`... restent clavier). En mode ordinateur la manette sert de
joystick (port 0/1, boutons RetroPad) comme `ComputerKeyboard` en mode Joystick ; aucun conflit de port à gérer
au-delà du OU.

Features : `gilrs 0.11.2` (features par défaut : `wgi` pour Windows) au workspace ; `emu-ui/gamepad = ["dep:gilrs"]`,
`emu-app/gamepad = ["emu-ui/gamepad"]`, **hors `default`** jusqu'à T4 (build Linux : `libudev-dev`, absent du conteneur
de dev : `--features gamepad` ne compile pas ici tant que T5 / l'environnement ne l'installe pas).

## 2. Tâches par agent

| Agent | Tâche | Fichiers |
|---|---|---|
| `architecte` | (T0) Poser les signatures ci-dessus avec `todo!()`, feature `gamepad` dans `Cargo.toml` (workspace, `emu-ui`, `emu-app`), passer l'ADR 0016 à « accepté », `cargo check --workspace` | `crates/emu-ui/src/gamepad/*`, `Cargo.toml` |
| `ui` (T1) | Logique pure : `PadMap` par `PadStyle`, deadzone/hystérésis, gâchettes, D-pad/stick, ports et hotplug, raccourcis PS + bouton, `GamepadInput` ; tests avec `ScriptedPadSource` | `crates/emu-ui/src/gamepad/{mapping,input,ports,hotkeys,source}.rs` |
| `ui` (T2) | `GilrsSource` (conversion d'événements gilrs, `MappingSource::None` ignoré, erreur gérée sans `/dev/input`) | `gamepad/gilrs_source.rs` |
| `ui` (T3) | Config : `GamepadConfig` (+ rétrocompat, 0600 inchangé), branchement dans `app.rs` (backend à côté de `KeyboardInput`, hotkeys manette → mêmes actions), notifications connexion/déconnexion, écran Réglages (deadzone, hotkey_button, état des manettes) et remappage « appuyez sur un bouton » | `config.rs`, `app.rs`, `ui/*` |
| `ui` (T4) | `emu-app` : feature `gamepad` par défaut, drapeau `--no-gamepad`, doc d'aide | `crates/emu-app/*` |
| `platform-build` (T5) | `libudev-dev` (build Linux : image dev, étage de build, CI), `libudev1` runtime ; features dans la matrice CI (`--no-default-features` compile sans gilrs) ; `THIRD-PARTY` régénéré ; compose : `/dev/input` en commentaire ; vérifier build macOS/Windows des archives | `docker/*`, `.github/workflows/*`, `scripts/*` |
| `qa-systems` (T6) | Tests d'intégration headless : `ScriptedPadSource` + core factice (empreintes), clavier+manette OU, test sur `gilrs` seul si possible : GUID DS4 → `MappingSource != None` ; config aller-retour ; vérification complète | `crates/emu-ui/tests/`, `tests/` |
| `reviewer` (T7) | Nouvelle dépendance `gilrs` (+ transitives : `libudev-sys`, `nix`, `objc2-io-kit`, `windows`, licences), absence de `unsafe` chez nous, pas de réseau, GUID/nom seuls journalisés | lecture seule |

## 3. Ordre et parallélisme

```text
T0 (architecte) ──► T1 (ui, logique) ──► T3 (ui, config + UI + app) ──► T6 (qa) ──► T7 (reviewer) ──► essai utilisateur
                └─► T2 (ui, gilrs)  ─┘ (T1 et T2 : fichiers distincts, même agent ou 2 sessions)
                └─► T5 (platform-build) en parallèle de T1/T2 (indépendant du code)
T4 après T3.
```

T1, T2 et T5 sont parallélisables. Tout `ui` touche `emu-ui` : un seul agent `ui` à la fois sur `app.rs`/`config.rs`
(T3), mais T1/T2 sont dans un dossier neuf (pas de conflit). Si la 2.0 (ST) modifie `config.rs`/`app.rs` en même temps :
séquencer T3 après la fusion de la branche ST ou isoler `GamepadConfig` dans `config_gamepad.rs` (comme
`config_library.rs`) — **recommandé** pour limiter les conflits.

## 4. Critères de « terminé »

- `cargo fmt/clippy -D warnings/test` verts sur tout le workspace, avec **et sans** la feature `gamepad`.
- Aucun test n'initialise gilrs ; aucun `unsafe` ajouté ; mapping de l'ADR 0016 couvert par des tests (un par colonne
  de système) ; hotplug, ports, deadzone, raccourcis, config (rétrocompat d'un `config.toml` 1.0) testés.
- Le clavier reste intact (tests existants inchangés) et marche en même temps que la manette.
- `reviewer` a validé la dépendance. Release en pré-release (`-rc.N`), puis essai utilisateur, puis stable.

## 5. Essais manuels pour l'utilisateur (Windows, puis macOS)

1. USB : brancher la DS4, lancer un jeu Mega Drive : la croix directionnelle et Croix/Rond/Carré (B/C/A) répondent ;
   notification « Manette connectée ».
2. Même chose en **Bluetooth** (appairage : maintenir PS + Share jusqu'au clignotement).
3. SNES / NES / Game Boy / Master System : les boutons suivent le tableau de l'ADR (Start = Options, Select = Share).
4. Clavier **et** manette simultanément ; deuxième manette = joueur 2.
5. Raccourcis : PS + R1 (sauver), PS + L1 (charger), PS + Options (menu), PS + R2 maintenu (avance rapide), PS seul = rien.
6. Débrancher en jeu : pas de plantage, jeu continue, notification ; rebrancher : reprise.
7. Stick gauche : D-pad sans dérive ; ajuster la deadzone si la manette est usée.
8. macOS : noter si le bouton PS ouvre autre chose, ou si une autorisation système est demandée.
9. Si DS4 non reconnue sur une plateforme : noter laquelle (USB/BT, DS4Windows ou Steam lancé ?) → bascule SDL3 (ADR 0016 §2).
Docker/noVNC : pas d'essai manette attendu (clavier).

## 6. Questions pour l'utilisateur (recommandation entre parenthèses)

1. **Bibliothèque** : gilrs d'abord, SDL3 en repli seulement si la DS4 échoue à l'essai ? (Oui : pas de lib C à compiler
   pour 4 builds ; coût : pas de gyro/tactile/vibration complète.)
2. **Vibration** : l'inclure dans cette phase ? (Non, plus tard : peu de valeur pour les consoles 8/16 bits.)
3. **Bouton de raccourcis** : PS comme modificateur, avec repli Share ? (Oui ; à confirmer à l'essai macOS/Windows.)
4. **Docker/WSLg** : manette non garantie (usbipd, pas de Bluetooth, noVNC impossible) : acceptable ? (Oui : jeu à la
   manette sous Windows/macOS natifs.)
5. **Ordre** par rapport à la 2.0 Atari ST ? (Manette d'abord : petite (~1 semaine d'agents), utile à toutes les
   consoles déjà livrées, et le ST/Amiga demandent clavier+souris, pas la manette ; mais choix de l'utilisateur.)
6. **Autres manettes** (Xbox, 8BitDo, Switch Pro) : acceptées « si gilrs les mappe », sans garantie ni tests dédiés ? (Oui.)
7. **Ports** : manette 1 = joueur 1 même si le clavier est aussi joueur 1 ? (Oui, combinaison par OU.)
