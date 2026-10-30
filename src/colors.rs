//! The lyrics page's background: the cover's most colorful shade, kept dark enough for white
//! lines on top (the sung line) and light enough that dark lines (the ones to come) still show.

/// Brightest channel of the background, out of 255.
const LEVEL: f32 = 150.0;

/// When the cover has no color to speak of (or none arrived).
pub const PLAIN: (u8, u8, u8) = (72, 72, 80);

/// From RGBA pixels: greys count a little, so black-and-white covers give a neutral shade.
pub fn lyrics_background(rgba: &[u8]) -> (u8, u8, u8) {
    let (mut r, mut g, mut b, mut w) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for px in rgba.chunks_exact(4).step_by(3) {
        let [pr, pg, pb] = [px[0], px[1], px[2]].map(|c| c as f32 / 255.0);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        let weight = sat * sat * max + 0.02;
        r += pr * weight;
        g += pg * weight;
        b += pb * weight;
        w += weight;
    }
    if w == 0.0 {
        return PLAIN;
    }
    let (r, g, b) = (r / w, g / w, b / w);
    let max = r.max(g).max(b);
    if max < 0.04 {
        return PLAIN;
    }
    let k = LEVEL / max;
    ((r * k) as u8, (g * k) as u8, (b * k) as u8)
}

/// The background for a song: its cover downloaded small and looked at.
pub async fn for_cover(thumb: Option<String>) -> (u8, u8, u8) {
    let Some(url) = thumb else { return PLAIN };
    let url = crate::api::thumb_sized(&url, 60);
    let got = async {
        let bytes = reqwest::get(&url).await.ok()?.error_for_status().ok()?.bytes().await.ok()?;
        tokio::task::spawn_blocking(move || image::load_from_memory(&bytes).ok().map(|i| lyrics_background(i.to_rgba8().as_raw())))
            .await
            .ok()
            .flatten()
    };
    got.await.unwrap_or(PLAIN)
}

/// `a` moved toward `b` by `t` (0 to 1).
pub fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Lines already sung, the line being sung, and lines to come, on `bg`.
pub fn lyric_colors(bg: (u8, u8, u8)) -> [(u8, u8, u8); 3] {
    [mix(bg, (255, 255, 255), 0.72), (255, 255, 255), mix(bg, (0, 0, 0), 0.55)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorful_covers_give_their_color_at_a_set_level() {
        // Red and grey: red wins, scaled so its brightest channel is LEVEL.
        let mut px = Vec::new();
        for i in 0..300 {
            px.extend_from_slice(if i % 2 == 0 { &[128, 128, 128, 255] } else { &[220, 40, 30, 255] });
        }
        let (r, g, b) = lyrics_background(&px);
        assert_eq!(r, LEVEL as u8);
        assert!(g < 60 && b < 60, "{r} {g} {b}");
        // Black covers: the plain shade.
        assert_eq!(lyrics_background(&[0, 0, 0, 255].repeat(50)), PLAIN);
    }
}
