//! User-facing strings of the first-run wizard, in one place for a future translation.

pub const TITLE: &str = "Bienvenue dans emu-rust";
pub const RELAUNCH: &str = "Relancer l'assistant de premier lancement";
pub const QUIT: &str = "Quitter l'assistant";
pub const BACK: &str = "Retour";
pub const SKIP: &str = "Passer";
pub const NEXT: &str = "Suivant";
pub const START: &str = "Commencer";
pub const FINISH: &str = "Terminer";
pub const HELP: &str = "Aide";
pub const KEYS_HINT: &str =
    "Tab ou flèches : se déplacer · Entrée : valider · Échap : étape précédente · F1 : aide";

pub const STEP_WELCOME: &str = "Bienvenue";
pub const STEP_ROMS: &str = "Dossier des ROMs";
pub const STEP_CORES: &str = "Cores";
pub const STEP_CONTROLS: &str = "Commandes";

pub fn step_counter(n: usize, total: usize) -> String {
    format!("Étape {n} sur {total}")
}

// ---- 1. welcome ----------------------------------------------------------------------------------------------

pub const WELCOME: &str = "Quelques réglages pour commencer : le dossier de vos jeux, les cores qui les \
                           font tourner, puis les commandes. Chaque étape peut être passée et l'assistant \
                           se relance depuis Réglages.";
pub const WELCOME_LEGAL: &str = "emu-rust ne télécharge jamais de jeu ni de BIOS commercial : vous \
                                 fournissez vos fichiers.";

// ---- 2. ROM folder -------------------------------------------------------------------------------------------

pub const ROMS_INTRO: &str = "Choisissez le dossier qui contient vos jeux (sous-dossiers compris). Il est \
                              seulement lu : rien n'y est modifié.";
pub const ROMS_FIXED: &str = "Ce dossier est toujours analysé lui aussi :";
pub const ROMS_CHOOSE: &str = "Choisir le dossier…";
pub const ROMS_DIALOG_TITLE: &str = "Dossier des ROMs";
pub const ROMS_DIALOG_OPEN: &str = "Fenêtre de choix du dossier ouverte…";
pub const ROMS_USE_THIS: &str = "Utiliser ce dossier";
pub const ROMS_TYPED_HINT: &str = "ou saisissez le chemin du dossier";
pub const ROMS_ADD: &str = "Ajouter";
pub const ROMS_SCAN_WAIT: &str = "Recherche des jeux…";

pub fn roms_folder(path: &str) -> String {
    format!("Dossier : {path}")
}

pub fn roms_not_a_folder(path: &str) -> String {
    format!("Dossier introuvable : {path}. Vérifiez le chemin ou choisissez-le avec le bouton.")
}

pub fn roms_found(games: usize, systems: usize) -> String {
    match games {
        0 => "Aucun jeu reconnu pour l'instant. Vérifiez le dossier (formats acceptés : voir l'Aide), \
              ou passez : vous pourrez l'ajouter dans Réglages."
            .to_owned(),
        1 => "1 jeu trouvé.".to_owned(),
        n => format!("{n} jeux trouvés pour {systems} système(s)."),
    }
}

// ---- 3. cores ------------------------------------------------------------------------------------------------

pub const CORES_INTRO: &str = "Un core est le programme qui fait tourner les jeux d'un système. Cochez ceux \
                               à installer ; ils viennent du serveur officiel libretro (buildbot).";
pub const CORES_NO_GAMES: &str = "Aucun jeu trouvé : pas de core à proposer. Vous pourrez en installer plus \
                                  tard depuis la page Cores.";
pub const CORES_NO_MANAGER: &str = "Gestionnaire de cores indisponible sur cette plate-forme : passez cette \
                                    étape.";
pub const CORES_OPEN_PAGE: &str = "Ouvrir la page Cores";
pub const CORES_RETRY: &str = "Réessayer";
pub const CORES_NOTE: &str =
    "Seuls les cores sont téléchargés, jamais de jeu ni de BIOS. Rien ne part sans votre clic.";
pub const CORES_INSTALLED: &str = "installé";
pub const CORES_QUEUED: &str = "en attente du catalogue…";
pub const CORES_DOWNLOADING: &str = "téléchargement…";
pub const CORES_UNKNOWN: &str = "aucun core connu pour ce système";
pub const CORES_CAPTURE: &str = "Téléchargement désactivé dans ce mode.";

pub fn cores_install(n: usize) -> String {
    format!("Installer la sélection ({n})")
}

pub fn cores_row(system: &str, core: &str) -> String {
    format!("{system} — {core}")
}

pub fn cores_offline(e: &str) -> String {
    format!(
        "Serveur des cores injoignable ({e}). Vérifiez la connexion puis « Réessayer », ou installez-les plus \
         tard depuis la page Cores."
    )
}

pub fn cores_failed(e: &str) -> String {
    format!("échec : {e}")
}

// ---- 4. controls ---------------------------------------------------------------------------------------------

pub const KEYBOARD_TITLE: &str = "Clavier";
pub const KEYBOARD_ROWS: [(&str, &str); 6] = [
    ("Flèches ou ZQSD", "croix de la manette"),
    ("Entrée / Maj droite", "Start / Select"),
    ("Échap", "menu du jeu (touches, sauvegardes, quitter)"),
    ("F2 / F4 ou Ctrl+S / Ctrl+L", "sauvegarder / charger l'état"),
    ("Espace (maintenu)", "avance rapide"),
    ("F1, ? ou Ctrl+H", "aide"),
];
pub const KEYBOARD_CHANGE: &str =
    "Pour changer les touches : menu du jeu, puis configuration des touches.";

pub fn host_key(key: &str) -> String {
    format!(
        "Ordinateurs (Atari ST, Amiga, DOS…) : touche hôte « {key} », seule pour le menu, maintenue avec une \
         lettre pour les raccourcis."
    )
}

pub const PAD_TITLE: &str = "Manette";
pub const PAD_DISABLED: &str = "Manettes désactivées : réactivez-les dans Réglages > Manette.";
pub const PAD_NONE: &str = "Aucune manette détectée. Branchez-la maintenant, ou configurez-la plus tard dans \
                            Réglages > Manette.";
pub const PAD_NOTE: &str = "Une DualShock 4 fonctionne sans réglage. « Configurer » apprend les boutons d'une \
                            autre manette.";
pub const PAD_CONFIGURE: &str = "Configurer";
pub const JOURNAL_REMINDER: &str = "En bas de chaque écran, le bouton « Journal » montre les messages techniques, \
                                    à copier pour signaler un problème.";
pub const BIOS_REMINDER: &str = "Certains systèmes demandent un BIOS que vous fournissez : la bibliothèque le \
                                 signale sur le jeu concerné, avec la marche à suivre.";
