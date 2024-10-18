pub use cosmic_text::{Family, Metrics, Stretch, Style, Weight};

pub struct Attrs<'a> {
    pub family: Family<'a>,
    pub stretch: Stretch,
    pub style: Style,
    pub weight: Weight,
}

impl<'a> Default for Attrs<'a> {
    fn default() -> Self {
        Self {
            family: Family::SansSerif,
            stretch: Default::default(),
            style: Default::default(),
            weight: Default::default(),
        }
    }
}
