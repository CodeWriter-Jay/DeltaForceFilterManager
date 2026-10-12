use crate::{models::Color, Result};
pub fn identity() -> Vec<u16> {
    (0..768).map(|i| ((i % 256) * 257) as u16).collect()
}
pub fn map(x: f64, c: &Color) -> f64 {
    let v = ((x - 0.5) * 2_f64.powf(c.contrast as f64 / 82.0)
        + 0.5
        + c.brightness as f64 / 125.0 * 0.15)
        .clamp(0.0, 1.0);
    let mut v = v.powf(1.0 / c.gamma);
    v += c.shadows as f64 / 100.0 * 4.0 * v * (1.0 - v).powi(3);
    let s = ((v - 0.55) / 0.45).clamp(0.0, 1.0);
    v *= 1.0 - c.highlights as f64 / 100.0 * 0.35 * s * s * (3.0 - 2.0 * s);
    v.clamp(0.0, 1.0)
}
pub fn build(c: &Color, calibration: &[u16]) -> Result<Vec<u16>> {
    c.validate()?;
    if calibration.len() != 768 {
        return Err("校准曲线长度无效".into());
    }
    let gains = [c.red_gain, c.green_gain, c.blue_gain];
    Ok((0..768)
        .map(|i| {
            let channel = i / 256;
            let pos = map((i % 256) as f64 / 255.0, c) * 255.0;
            let lo = pos.floor() as usize;
            let hi = (lo + 1).min(255);
            let fraction = pos - lo as f64;
            let value = calibration[channel * 256 + lo] as f64 * (1.0 - fraction)
                + calibration[channel * 256 + hi] as f64 * fraction;
            (value * (1.0 + gains[channel] as f64 / 100.0 * 0.48))
                .round()
                .clamp(0.0, 65535.0) as u16
        })
        .collect())
}
pub fn matches(expected: &[u16], actual: &[u16]) -> bool {
    expected.len() == 768
        && actual.len() == 768
        && expected
            .iter()
            .zip(actual)
            .all(|(a, b)| a.abs_diff(*b) <= 257)
}
