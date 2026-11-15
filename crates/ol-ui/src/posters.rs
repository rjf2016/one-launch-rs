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

    // Forgets every poster, for a list of other games. What comes back is still held by the window that drew it.
    pub fn clear(&mut self) -> Vec<Arc<RenderImage>> {
        self.slots
            .drain()
            .filter_map(|(_, slot)| match slot {
                Slot::Ready { image, .. } => Some(image),
                Slot::Pending | Slot::Unreadable => None,
            })
            .collect()
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
        Shape::Cover => picture.resize_to_fill(side(WIDTH), side(HEIGHT), FilterType::CatmullRom),
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
