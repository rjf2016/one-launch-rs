use std::collections::HashMap;
use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, Rgba, RgbaImage};

const WIDTH: u32 = 240;
const HEIGHT: u32 = 360;

pub enum Lookup {
    Ready(Arc<RenderImage>),
    Pending,
    Missing,
}

enum Slot {
    Pending,
    Ready { image: Arc<RenderImage>, used: u64 },
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
            None => {
                self.slots.insert(game, Slot::Pending);
                Lookup::Missing
            }
        }
    }

    pub fn insert(&mut self, game: usize, image: Arc<RenderImage>) {
        let used = self.clock;
        self.slots.insert(game, Slot::Ready { image, used });
    }

    pub fn evict(&mut self, keep: usize) -> Vec<Arc<RenderImage>> {
        let mut ready: Vec<(u64, usize)> = self
            .slots
            .iter()
            .filter_map(|(game, slot)| match slot {
                Slot::Ready { used, .. } => Some((*used, *game)),
                Slot::Pending => None,
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

pub fn placeholder(game: usize) -> RenderImage {
    let hue = (game as f32 * 0.618_034).fract();
    let top = hsl(hue, 0.55, 0.46);
    let bottom = hsl((hue + 0.07).fract(), 0.6, 0.14);
    let band = (game % 5) as f32 * 0.12 + 0.2;
    let buffer = RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
        let across = x as f32 / WIDTH as f32;
        let down = y as f32 / HEIGHT as f32;
        let slant = down + across * 0.3;
        let glow = (1. - ((slant - band) * 6.).abs()).clamp(0., 1.) * 0.18;
        let [red, green, blue] = mix(top, bottom, (slant / 1.3).clamp(0., 1.))
            .map(|channel| ((channel + glow).clamp(0., 1.) * 255.) as u8);
        // GPUI's render images are BGRA.
        Rgba([blue, green, red, 255])
    });
    RenderImage::new(vec![Frame::new(buffer)])
}

fn mix(from: [f32; 3], to: [f32; 3], amount: f32) -> [f32; 3] {
    [0, 1, 2].map(|channel| from[channel] + (to[channel] - from[channel]) * amount)
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let chroma = (1. - (2. * lightness - 1.).abs()) * saturation;
    let sector = hue * 6.;
    let second = chroma * (1. - (sector % 2. - 1.).abs());
    let [red, green, blue] = match sector as u32 {
        0 => [chroma, second, 0.],
        1 => [second, chroma, 0.],
        2 => [0., chroma, second],
        3 => [0., second, chroma],
        4 => [second, 0., chroma],
        _ => [chroma, 0., second],
    };
    let lift = lightness - chroma / 2.;
    [red + lift, green + lift, blue + lift]
}
