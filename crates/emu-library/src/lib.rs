//! Game library (ADR 0006, ADR 0014): system catalog, ROM scan, CRC32/SHA1, No-Intro DAT identification, SQLite.
//! No network here: DAT files are downloaded by `emu-assets` and imported from disk.
//! Contract: `docs/design/v1.0-bibliotheque.md` §6.1.

mod catalog;
mod custom;
mod dat;
mod db;
mod disc;
mod dos;
mod error;
mod filter;
mod hash;
mod media;
mod scan;
mod systems;
mod tidy;

pub use crate::catalog::{
    Catalog, CatalogLoad, DefinitionProblem, SwitchKind, SwitchProposal, catalog, install_catalog,
};
pub use crate::custom::{
    ARCHIVE_EXTENSIONS, CUSTOM_FORMAT, CustomSystem, CustomSystemDraft, DISC_EXTENSIONS_8_0,
    DefinitionError, DefinitionOrigin, MAX_DEFINITION_FILES, MAX_DEFINITION_SIZE, MachineTier,
    PLAYLIST_EXTENSIONS, RetiredSystem, Retirement, cartridge_extensions, dat_file_stem,
    is_reserved_id, is_safe_relative_path, known_id, known_tier, normalize_name, proposed_id,
    restore_custom, retire_custom, retired_customs, retired_file_name, validate_custom,
    write_custom,
};
pub use crate::dat::{Dat, DatEntry, MAX_DAT_SIZE, load_dat_file, parse_clrmamepro};
pub use crate::db::{
    CoverKind, CoverRecord, CoverState, DatInfo, ForgetDats, Game, GameId, Identification, Library,
    OpenOutcome, SCHEMA_VERSION,
};
pub use crate::disc::{
    BinKind, CD_32X_PROBE_LEN, DISC_EXTENSIONS, DiscFormat, DiscInfo, DiscRegion, cue_references,
    disc_info, disc_needs_32x, disc_serial, name_needs_32x, probe_bin, region_from_name,
};
pub use crate::error::{LibraryError, TidyRefusal};
pub use crate::filter::{GameFilter, SortOrder, filter_games, normalize_title};
pub use crate::hash::{Hashes, MAX_HASH_SIZE, RomHashes, hash_reader};
pub use crate::media::{MAX_MEDIA_IMAGES, MediaGroup, group_media};
pub use crate::scan::{MAX_UNCLASSIFIED_PATHS, ScanOptions, ScanProgress, ScanReport};
pub use crate::systems::{
    DatSource, HeaderRule, InputDefault, SystemInfo, SystemKind, SystemOrigin, system, systems,
    systems_for_extension,
};
pub use crate::tidy::{
    AbsoluteMoves, KeysChoice, MAX_IGNORED_PATHS, PendingSwitch, SwitchChoices, SwitchDirection,
    SwitchMove, TidyBlock, TidyExtraName, TidyJournalInfo, TidyMove, TidyOptions, TidyPlan,
    TidyProgress, TidyReason, TidyReport, TidySection, TidyStatus, TidyUnit, create_system_folders,
    last_tidy,
};
