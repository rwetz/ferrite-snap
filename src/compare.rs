use image::{Rgba, RgbaImage};

pub struct Difference {
    pub image: RgbaImage,
    pub changed: u64,
    pub total: u64,
    pub resized: bool,
}

pub fn compare(old: &RgbaImage, new: &RgbaImage, accent: u32, tolerance: u8) -> Difference {
    let (width, height) = (old.width().max(new.width()), old.height().max(new.height()));
    let mut output = RgbaImage::new(width, height);
    let mut changed = 0;
    let resized = old.dimensions() != new.dimensions();
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let a = old.get_pixel_checked(x, y);
        let b = new.get_pixel_checked(x, y);
        let different = match (a, b) {
            (Some(a), Some(b)) => (0..3).any(|i| a[i].abs_diff(b[i]) > tolerance),
            _ => true,
        };
        if different {
            changed += 1;
            *pixel = Rgba([(accent >> 16) as u8, (accent >> 8) as u8, accent as u8, 255]);
        } else if let Some(b) = b {
            // Dim the unchanged image so the accent differences dominate.
            *pixel = Rgba([b[0] / 3, b[1] / 3, b[2] / 3, 255]);
        }
    }
    Difference {
        image: output,
        changed,
        total: u64::from(width) * u64::from(height),
        resized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_changes_and_dimension_mismatch_without_counting_small_noise() {
        let old = RgbaImage::from_pixel(2, 2, Rgba([50, 50, 50, 255]));
        let mut new = old.clone();
        new.put_pixel(0, 0, Rgba([55, 50, 50, 255]));
        assert_eq!(compare(&old, &new, 0x123456, 8).changed, 0);
        new.put_pixel(1, 1, Rgba([80, 50, 50, 255]));
        let diff = compare(&old, &new, 0x123456, 8);
        assert_eq!(diff.changed, 1);
        assert_eq!(diff.image.get_pixel(1, 1).0, [0x12, 0x34, 0x56, 255]);
        let diff = compare(&old, &RgbaImage::new(3, 2), 0, 0);
        assert!(diff.resized);
        assert_eq!(diff.total, 6);
    }
}
