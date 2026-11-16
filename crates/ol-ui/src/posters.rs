use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use gpui::RenderImage;
use image::imageops::FilterType;
use image::{DynamicImage, Frame, ImageDecoder, ImageReader, Limits};

const WIDTH: f32 = 240.;
const HEIGHT: f32 = 360.;
const ICON: f32 = 128.;
const LARGEST_FILE: u64 = 32 * 1024 * 1024;
const LARGEST_SIDE: u32 = 8192;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Cover,
    Icon,
}

pub enum Lookup {
    Ready(Arc<RenderImage>),
    Pending,
    Missing,
    Unreadable,
}

enum Slot {
    Pending,
    Ready { image: Arc<RenderImage>, used: u64 },
    Unreadable,
}

#[derive(Default)]
pub struct Posters {
    slots: HashMap<usize, Slot>,
    clock: u64,
}

impl Posters {
    pub fn tick(&mut self) {
        self.clock += 1;
    }

    pub fn lookup(&mut self, game: usize) -> Lookup {
        match self.slots.get_mut(&game) {
            Some(Slot::Ready { image, used }) => {
                *used = self.clock;
                Lookup::Ready(image.clone())
            }
            Some(Slot::Pending) => Lookup::Pending,
            Some(Slot::Unreadable) => Lookup::Unreadable,
            None => {
                self.slots.insert(game, Slot::Pending);
                Lookup::Missing
            }
        }
    }

    pub fn insert(&mut self, game: usize, image: Option<Arc<RenderImage>>) {
        let used = self.clock;
        let slot = match image {
            Some(image) => Slot::Ready { image, used },
            None => Slot::Unreadable,
        };
        self.slots.insert(game, slot);
    }

    pub fn evict(&mut self, keep: usize) -> Vec<Arc<RenderImage>> {
        let mut ready: Vec<(u64, usize)> = self
            .slots
            .iter()
            .filter_map(|(game, slot)| match slot {
                Slot::Ready { used, .. } => Some((*used, *game)),
                Slot::Pending | Slot::Unreadable => None,
            })
            .collect();
        if ready.len() <= keep {
            return Vec::new();
        }
        ready.sort_unstable();
        let excess = ready.len() - keep;
        ready
            .into_iter()
            .take(excess)
            .filter_map(|(_, game)| match self.slots.remove(&game) {
                Some(Slot::Ready { image, .. }) => Some(image),
                _ => None,
            })
            .collect()
    }
}

// Run off the UI thread. A file that is missing, too large or no picture gives nothing, and the card keeps its letters.
pub fn decode(file: &Path, shape: Shape, scale: f32) -> Option<RenderImage> {
    if fs::metadata(file).ok()?.len() > LARGEST_FILE {
        return None;
    }
    let mut reader = ImageReader::new(Cursor::new(fs::read(file).ok()?))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(LARGEST_SIDE);
    limits.max_image_height = Some(LARGEST_SIDE);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);

    let side = |length: f32| (length * scale.clamp(1., 3.)).ceil() as u32;
    let fitted = match shape {
        Shape::Cover => cover(&picture, side(WIDTH), side(HEIGHT)),
        Shape::Icon if picture.width().max(picture.height()) > side(ICON) => {
            picture.resize(side(ICON), side(ICON), FilterType::CatmullRom)
        }
        Shape::Icon => picture,
    };
    let mut pixels = fitted.into_rgba8();
    // GPUI's render images are BGRA.
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(RenderImage::new(vec![Frame::new(pixels)]))
}

// Cropped to the card's proportions before it is scaled. Scaled first, a picture one pixel tall would be blown up to the card's height at its full width, which is gigabytes.
fn cover(picture: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    let (from_width, from_height) = (u64::from(picture.width()), u64::from(picture.height()));
    let (wide, tall) = (u64::from(width), u64::from(height));
    let (kept_width, kept_height) = if from_width * tall > from_height * wide {
        ((from_height * wide / tall).max(1), from_height)
    } else {
        (from_width, (from_width * tall / wide).max(1))
    };
    let left = (from_width - kept_width) / 2;
    let top = (from_height - kept_height) / 2;
    picture
        .crop_imm(
            left as u32,
            top as u32,
            kept_width as u32,
            kept_height as u32,
        )
        .resize_exact(width, height, FilterType::CatmullRom)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;

    #[test]
    fn a_picture_of_any_proportions_fills_the_card_without_being_scaled_whole() {
        for (from_width, from_height) in [(8192, 1), (1, 8192), (600, 900), (460, 215), (1, 1)] {
            let picture = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
                from_width,
                from_height,
                Rgba([10, 20, 30, 255]),
            ));
            let fitted = cover(&picture, 720, 1080);
            assert_eq!(
                (fitted.width(), fitted.height()),
                (720, 1080),
                "{from_width}x{from_height}"
            );
        }
    }

    #[test]
    fn the_middle_of_a_wide_picture_is_what_is_kept() {
        let mut wide = RgbaImage::from_pixel(300, 100, Rgba([0, 0, 0, 255]));
        for x in 100..200 {
            for y in 0..100 {
                wide.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let fitted = cover(&DynamicImage::ImageRgba8(wide), 60, 90).into_rgba8();
        assert_eq!(fitted.get_pixel(30, 45).0, [255, 255, 255, 255]);
        assert_eq!(fitted.get_pixel(1, 45).0, [255, 255, 255, 255]);
    }
}
