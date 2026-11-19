//! Cover art seen from the home screen. Extension point for task L2: a provider backed by
//! `emu_assets::{CoverCache, CoverChain}` will run the download / decode queue on its own thread (ADR 0014 §5,
//! §7), store `CoverState` through its own `Library` connection, and upload textures for visible cards only
//! (LRU ~256). Until then `Placeholder` draws every card with its title and system.

use egui::TextureHandle;
use emu_library::{Game, GameId};

pub(crate) trait CoverProvider {
    /// Texture for `game` if it is decoded and uploaded; `None` draws the placeholder card.
    fn texture(&mut self, ctx: &egui::Context, game: &Game) -> Option<TextureHandle>;
    /// Games on screen this frame, first to fetch (called once per frame, cheap).
    fn visible(&mut self, games: &[GameId]);
    /// One-line status for the bottom bar (offline, queue length…), if any.
    fn status(&self) -> Option<String>;
}

/// No cover source yet: placeholder cards, nothing downloaded.
pub(crate) struct Placeholder;

impl CoverProvider for Placeholder {
    fn texture(&mut self, _ctx: &egui::Context, _game: &Game) -> Option<TextureHandle> {
        None
    }

    fn visible(&mut self, _games: &[GameId]) {}

    fn status(&self) -> Option<String> {
        None
    }
}
