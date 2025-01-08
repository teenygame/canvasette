#[derive(Clone)]
pub struct Img<Pixels> {
    pixels: Pixels,
    size: glam::UVec2,
    layers: u32,
}

impl<Pixels> Img<Pixels> {
    pub fn new(pixels: Pixels, size: glam::UVec2, layers: u32) -> Self {
        Self {
            pixels,
            size,
            layers,
        }
    }

    pub fn size(&self) -> glam::UVec2 {
        self.size
    }

    pub fn layers(&self) -> u32 {
        self.layers
    }
}

impl<Pixels> Copy for Img<Pixels> where Pixels: Copy {}

/// Converts an image to a reference to the image.
pub trait AsImgRef<Pixel> {
    /// Gets the image as a reference.
    fn as_ref(&self) -> Img<&[Pixel]>;
}

impl<Pixel> AsImgRef<Pixel> for Img<&[Pixel]> {
    fn as_ref(&self) -> Img<&[Pixel]> {
        *self
    }
}

impl<Pixel> AsImgRef<Pixel> for Img<Vec<Pixel>> {
    fn as_ref(&self) -> Img<&[Pixel]> {
        Img::new(self.pixels.as_slice(), self.size, self.layers)
    }
}

impl<Pixel> AsImgRef<Pixel> for &Img<Vec<Pixel>> {
    fn as_ref(&self) -> Img<&[Pixel]> {
        Img::new(self.pixels.as_slice(), self.size, self.layers)
    }
}

impl<Pixel> Img<&[Pixel]> {
    pub fn as_buf(&self) -> &[Pixel] {
        self.pixels
    }
}
