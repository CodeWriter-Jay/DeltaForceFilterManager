use crate::{storage, Result};
use std::path::Path;
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
fn u16be(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([b[i], b[i + 1]])
}
fn u32be(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
pub fn read(path: &Path) -> Result<Option<Vec<u16>>> {
    parse(&storage::read_bytes(path, MAX_BYTES)?)
}
pub fn parse(bytes: &[u8]) -> Result<Option<Vec<u16>>> {
    if bytes.len() < 132 || bytes.len() as u64 > MAX_BYTES || &bytes[36..40] != b"acsp" {
        return Err("文件不是有效的 ICC 配置（最大 16 MB）".into());
    }
    let size = u32be(bytes, 0) as usize;
    if !(132..=bytes.len()).contains(&size) {
        return Err("ICC 文件已截断".into());
    }
    let b = &bytes[..size];
    if &b[12..16] != b"mntr" || &b[16..20] != b"RGB " {
        return Err("只支持 RGB 显示器 ICC（mntr）".into());
    }
    let count = u32be(b, 128) as usize;
    if count > (b.len() - 132) / 12 {
        return Err("ICC 标签表损坏".into());
    }
    let mut ramp = None;
    for i in 0..count {
        let entry = 132 + i * 12;
        let offset = u32be(b, entry + 4) as usize;
        let length = u32be(b, entry + 8) as usize;
        if offset < 132 + count * 12 || offset > b.len() || length > b.len() - offset {
            return Err("ICC 标签超出文件边界".into());
        }
        if &b[entry..entry + 4] == b"vcgt" {
            if ramp.is_some() {
                return Err("ICC 包含重复 vcgt".into());
            }
            ramp = Some(calibration(&b[offset..offset + length])?);
        }
    }
    Ok(ramp)
}
fn calibration(b: &[u8]) -> Result<Vec<u16>> {
    if b.len() < 12 || &b[..4] != b"vcgt" {
        return Err("vcgt 标签无效".into());
    }
    let mut ramp = vec![0_u16; 768];
    match u32be(b, 8) {
        0 => {
            if b.len() < 18 {
                return Err("vcgt 表已截断".into());
            }
            let channels = u16be(b, 12) as usize;
            let entries = u16be(b, 14) as usize;
            let width = u16be(b, 16) as usize;
            if ![1, 3].contains(&channels)
                || !(2..=4096).contains(&entries)
                || ![1, 2].contains(&width)
                || 18 + channels * entries * width > b.len()
            {
                return Err("vcgt 表结构无效或已截断".into());
            }
            let value = |i| {
                if width == 1 {
                    b[i] as f64 * 257.0
                } else {
                    u16be(b, i) as f64
                }
            };
            for ch in 0..3 {
                for i in 0..256 {
                    let source = if channels == 1 { 0 } else { ch };
                    let p = i as f64 * (entries - 1) as f64 / 255.0;
                    let lo = p.floor() as usize;
                    let hi = (lo + 1).min(entries - 1);
                    let t = p - lo as f64;
                    ramp[ch * 256 + i] = (value(18 + (source * entries + lo) * width) * (1.0 - t)
                        + value(18 + (source * entries + hi) * width) * t)
                        .round() as u16;
                }
            }
        }
        1 => {
            if b.len() < 48 {
                return Err("vcgt 公式已截断".into());
            }
            for ch in 0..3 {
                let offset = 12 + ch * 12;
                let gamma = u32be(b, offset) as f64 / 65536.0;
                let min = u32be(b, offset + 4) as f64 / 65536.0;
                let max = u32be(b, offset + 8) as f64 / 65536.0;
                if !(0.0..=10.0).contains(&gamma) || gamma == 0.0 || min > max || max > 1.0 {
                    return Err("vcgt 公式参数无效".into());
                }
                for i in 0..256 {
                    ramp[ch * 256 + i] = ((min + (max - min) * (i as f64 / 255.0).powf(gamma))
                        * 65535.0)
                        .round() as u16;
                }
            }
        }
        _ => return Err("不支持此 vcgt 类型".into()),
    }
    if ramp
        .as_chunks::<256>()
        .0
        .iter()
        .any(|ch| ch.windows(2).any(|p| p[1] < p[0]))
    {
        return Err("vcgt 必须为单调递增曲线".into());
    }
    Ok(ramp)
}
