//! Presenting at 3840x2160 (T-071): `nearest`, and `xbrz` with the view changed every
//! frame (the scaler runs) or not (the cached image is repeated). Synthetic frames.

use criterion::{Criterion, criterion_group, criterion_main};
use xeno_render::{Aspect, Framebuffer, Layout, Palette, Rgb, Scale, Size, Xbrz, present};

const WINDOW: Size = Size::new(3840, 2160);

/// Diagonal bands of 16 colours with a speck every 7 pixels: edges everywhere.
fn frame(view: Size, seed: usize) -> Framebuffer {
    let mut fb = Framebuffer::new(view);
    for (i, p) in fb.pixels_mut().iter_mut().enumerate() {
        let (x, y) = (i % view.width, i / view.width);
        *p = if (i + seed).is_multiple_of(7) {
            250
        } else {
            (((x + 2 * y + seed) / 9) % 16 * 11) as u8
        };
    }
    fb
}

fn palette() -> Palette {
    let mut colors = [Rgb { r: 0, g: 0, b: 0 }; Palette::LEN];
    for (i, c) in colors.iter_mut().enumerate() {
        *c = Rgb {
            r: i as u8,
            g: (i * 5) as u8,
            b: (255 - i) as u8,
        };
    }
    Palette { colors }
}

fn bench(c: &mut Criterion) {
    let palette = palette();
    let mut dst = vec![0; WINDOW.width * WINDOW.height];
    for (name, view) in [("320x200", Size::MODE_13H), ("320x240", Size::MODE_240)] {
        let layout = Layout::new(view, WINDOW, Scale::Auto, Aspect::Correct);
        let frames = [frame(view, 0), frame(view, 1)];
        c.bench_function(&format!("nearest {name} 4K"), |b| {
            b.iter(|| present(&frames[0], &palette, &layout, &mut dst, WINDOW))
        });
        let mut xbrz = Xbrz::default();
        let mut n = 0;
        c.bench_function(&format!("xbrz {name} 4K, new frame"), |b| {
            b.iter(|| {
                n += 1;
                xbrz.present(&frames[n % 2], &palette, &layout, &mut dst, WINDOW)
            })
        });
        c.bench_function(&format!("xbrz {name} 4K, same frame"), |b| {
            b.iter(|| xbrz.present(&frames[0], &palette, &layout, &mut dst, WINDOW))
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
