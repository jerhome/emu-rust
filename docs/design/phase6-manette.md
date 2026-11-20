# Phase 6 — Manette PS4 (DualShock 4) : conception et découpage

Décisions : `docs/adr/0016-manette.md` (statut « proposé » jusqu'aux réponses de l'utilisateur, section Questions).
Livrable : version après 1.0 (ordre par rapport à la 2.0 Atari ST à décider ; les deux sont indépendantes, seul
`docs/roadmap.md` et `config.rs` sont des points de contact éventuels).

## 1. Interfaces publiques (à poser par l'architecte au lancement de la phase)

Dans `crates/emu-ui/src/gamepad/` (fichiers `mod.rs`, `source.rs`, `mapping.rs`, `input.rs`, `ports.rs`,
`hotkeys.rs`, `gilrs_source.rs` sous `#[cfg(feature = "gamepad")]`). Signatures cibles (corps `todo!()` avant remplissage) :

```rust
/// Stable-for-the-session identity of a connected pad.
pub struct PadId(pub u32);

/// Physical buttons by position (South = Cross on a DS4).
pub enum PadButton { South, East, West, North, L1, R1, L2, R2, Select, Start, Mode, L3, R3,
                     DPadUp, DPadDown, DPadLeft, DPadRight }
pub enum PadAxis { LeftX, LeftY, RightX, RightY, L2, R2 }   // [-1,1] ; L2/R2 in [0,1]

pub enum PadEventKind { Connected { name: String, guid: String }, Disconnected,
                        Button { button: PadButton, pressed: bool }, Axis { axis: PadAxis, value: f32 } }
pub struct PadEvent { pub id: PadId, pub kind: PadEventKind }

/// Injectable source of normalised events (real = gilrs, tests = scripted).
pub trait GamepadSource {
    fn poll_events(&mut self, out: &mut Vec<PadEvent>);
}
pub struct ScriptedPadSource;              // events per call, for tests
#[cfg(feature = "gamepad")] pub struct GilrsSource;
#[cfg(feature = "gamepad")] impl GilrsSource { pub fn new() -> Result<Self, GamepadError>; }

pub struct GamepadSettings { pub enabled: bool, pub deadzone: f32, pub trigger_threshold: f32,
                             pub hotkey_button: HotkeyButton, pub pause_on_disconnect: bool }
pub struct PadMap;                          // PadButton -> JoypadButton for one profile
impl PadMap { pub fn default_for(style: PadStyle) -> Self; }

pub struct GamepadInput<S: GamepadSource> { /* ports, states, map, settings */ }
impl<S: GamepadSource> GamepadInput<S> {
    pub fn new(source: S, map: PadMap, settings: GamepadSettings) -> Self;
    pub fn take_hotkeys(&mut self) -> Vec<HotkeyAction>;     // PS + button actions since last call
    pub fn take_notices(&mut self) -> Vec<GamepadNotice>;    // Connected(name, port) / Disconnected(port)
}
impl<S: GamepadSource> emu_runtime::InputBackend for GamepadInput<S> { /* poll drains the source */ }
```

`HotkeyAction` (existant) est réutilisé tel quel. `emu-ui/src/config.rs` : `InputConfig` gagne
`gamepad: GamepadConfig` (serde, `#[serde(default)]`, `extra` flatten) selon le TOML de l'ADR 0016.

`emu-runtime` : **aucun changement requis** pour le numérique. Différé (non planifié ici) : axes analogiques dans
`PadState`/`InputFrame` (déjà `non_exhaustive`) pour `RETRO_DEVICE_ANALOG`, à ouvrir avec un core 3D.

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
