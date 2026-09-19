//! TypeSafe jEV integration: smart paper highlighting.

pub mod commands;
mod service;

pub use service::{jev_suggest_highlights_for_paper, SuggestedHighlight};
