//! Generated tray icons: one solid disc per health tier.
//!
//! Generating the pixels in code keeps the repository free of binary icon
//! assets and means every platform gets the same shape; only the colour
//! changes with the portfolio's [`Tier`].

use git_check_core::metrics::Tier;
use tray_icon::Icon;

/// Edge length of the generated icon, in pixels.
pub const SIZE: u32 = 32;

/// Raw RGBA pixels for a tier's icon.
pub fn rgba_for_tier(tier: Tier) -> Vec<u8> {
    let (red, green, blue) = match tier {
        Tier::Ok => (0x2e, 0xa0, 0x43),
        Tier::Warn => (0xe0, 0x8a, 0x00),
        Tier::Alert => (0xd1, 0x24, 0x2f),
    };
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let centre = SIZE as f32 / 2.0;
    for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (index as u32 % SIZE) as f32 + 0.5;
        let y = (index as u32 / SIZE) as f32 + 0.5;
        let distance = ((x - centre).powi(2) + (y - centre).powi(2)).sqrt();
        if distance <= 13.0 {
            pixel.copy_from_slice(&[red, green, blue, 0xff]);
        } else if distance <= 14.0 {
            // A half-transparent rim so the disc does not alias on HiDPI panels.
            pixel.copy_from_slice(&[red, green, blue, 0x80]);
        }
    }
    rgba
}

/// Tray icon for a health tier.
pub fn for_tier(tier: Tier) -> Icon {
    Icon::from_rgba(rgba_for_tier(tier), SIZE, SIZE).expect("generated icon has a valid size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_have_distinct_pixels() {
        assert_ne!(rgba_for_tier(Tier::Ok), rgba_for_tier(Tier::Warn));
        assert_ne!(rgba_for_tier(Tier::Warn), rgba_for_tier(Tier::Alert));
    }

    #[test]
    fn disc_is_opaque_at_the_centre_and_clear_at_the_corner() {
        let rgba = rgba_for_tier(Tier::Ok);
        assert_eq!(rgba.len(), (SIZE * SIZE * 4) as usize);
        let centre = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        assert_eq!(rgba[centre + 3], 0xff);
        assert_eq!(rgba[3], 0x00);
    }
}
