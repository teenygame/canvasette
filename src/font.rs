//! Various types for fonts.

pub use cosmic_text::{FamilyOwned as Family, Metrics, Stretch, Style, Weight};

/// Font attributes.
pub struct Attrs {
    /// Font family (e.g. sans-serif, serif).
    pub family: Family,
    /// Font stretch (e.g. condensed, regular).
    pub stretch: Stretch,
    /// Font style (e.g. normal, italic, oblique).
    pub style: Style,
    /// Font weight.
    pub weight: Weight,
}

impl Default for Attrs {
    fn default() -> Self {
        Self {
            family: Family::SansSerif,
            stretch: Default::default(),
            style: Default::default(),
            weight: Default::default(),
        }
    }
}
