pub use cosmic_text::{FamilyOwned as Family, Metrics, Stretch, Style, Weight};

pub struct Attrs {
    pub family: Family,
    pub stretch: Stretch,
    pub style: Style,
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
