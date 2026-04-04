//! Generate test images for manual testing of the image viewer.
//! Usage: cargo run --example gen_test_image

use image::{Rgb, RgbImage};

fn main() {
    // 1. Four-color grid
    let mut img = RgbImage::new(200, 200);
    for y in 0..200 {
        for x in 0..200 {
            let color = match (x < 100, y < 100) {
                (true, true) => Rgb([255, 0, 0]),     // red
                (false, true) => Rgb([0, 255, 0]),     // green
                (true, false) => Rgb([0, 0, 255]),     // blue
                (false, false) => Rgb([255, 255, 0]),  // yellow
            };
            img.put_pixel(x, y, color);
        }
    }
    img.save("/tmp/test_grid.png").unwrap();
    eprintln!("Created /tmp/test_grid.png");

    // 2. Diagonal gradient
    let mut img = RgbImage::new(200, 200);
    for y in 0..200 {
        for x in 0..200 {
            let r = (x as f32 / 199.0 * 255.0) as u8;
            let b = (y as f32 / 199.0 * 255.0) as u8;
            img.put_pixel(x, y, Rgb([r, 0, b]));
        }
    }
    img.save("/tmp/test_gradient.png").unwrap();
    eprintln!("Created /tmp/test_gradient.png");

    // 3. Diagonal split (sharp edge)
    let mut img = RgbImage::new(200, 200);
    for y in 0..200 {
        for x in 0..200 {
            let color = if x > y {
                Rgb([255, 50, 50])
            } else {
                Rgb([50, 50, 255])
            };
            img.put_pixel(x, y, color);
        }
    }
    img.save("/tmp/test_diagonal.png").unwrap();
    eprintln!("Created /tmp/test_diagonal.png");

    // 4. Circle on contrasting background
    let mut img = RgbImage::new(200, 200);
    let cx = 100.0f32;
    let cy = 100.0f32;
    let r = 80.0f32;
    for y in 0..200 {
        for x in 0..200 {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let color = if dist < r {
                Rgb([255, 200, 50])
            } else {
                Rgb([30, 30, 80])
            };
            img.put_pixel(x, y, color);
        }
    }
    img.save("/tmp/test_circle.png").unwrap();
    eprintln!("Created /tmp/test_circle.png");
}
