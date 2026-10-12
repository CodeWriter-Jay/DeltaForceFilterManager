use crate::Result;
/// Inspect dimensions before WebView decoding so compressed images cannot allocate unbounded surfaces.
pub fn inspect(bytes: &[u8]) -> Result<&'static str> {
    let (mime, width, height) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
            return Err("PNG 头已损坏".into());
        }
        (
            "image/png",
            u32::from_be_bytes(
                bytes[16..20]
                    .try_into()
                    .map_err(|e| format!("PNG 宽度无效：{e}"))?,
            ) as u64,
            u32::from_be_bytes(
                bytes[20..24]
                    .try_into()
                    .map_err(|e| format!("PNG 高度无效：{e}"))?,
            ) as u64,
        )
    } else if bytes.starts_with(b"BM") {
        if bytes.len() < 26 {
            return Err("BMP 头已截断".into());
        }
        let size = u32::from_le_bytes(
            bytes[14..18]
                .try_into()
                .map_err(|e| format!("BMP 头无效：{e}"))?,
        );
        if size == 12 {
            (
                "image/bmp",
                u16::from_le_bytes([bytes[18], bytes[19]]) as u64,
                u16::from_le_bytes([bytes[20], bytes[21]]) as u64,
            )
        } else if size >= 40 {
            let w = i32::from_le_bytes(
                bytes[18..22]
                    .try_into()
                    .map_err(|e: std::array::TryFromSliceError| e.to_string())?,
            );
            let h = i32::from_le_bytes(
                bytes[22..26]
                    .try_into()
                    .map_err(|e: std::array::TryFromSliceError| e.to_string())?,
            );
            if w <= 0 {
                return Err("BMP 宽度无效".into());
            }
            ("image/bmp", w as u64, h.unsigned_abs() as u64)
        } else {
            return Err("不支持此 BMP 头".into());
        }
    } else if bytes.starts_with(b"\xff\xd8") {
        let mut offset = 2;
        let mut dimensions = None;
        while offset < bytes.len() {
            if bytes[offset] != 0xff {
                return Err("JPEG 标记无效".into());
            }
            while offset < bytes.len() && bytes[offset] == 0xff {
                offset += 1;
            }
            if offset >= bytes.len() {
                break;
            }
            let marker = bytes[offset];
            offset += 1;
            if marker == 0xd9 || marker == 0xda {
                break;
            }
            if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
                continue;
            }
            if offset + 2 > bytes.len() {
                break;
            }
            let size = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
            if size < 2 || size > bytes.len() - offset {
                return Err("JPEG 段已截断".into());
            }
            if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
                if size < 8 {
                    return Err("JPEG 帧已截断".into());
                }
                dimensions = Some((
                    u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]) as u64,
                    u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]]) as u64,
                ));
                break;
            }
            offset += size;
        }
        let (w, h) = dimensions.ok_or("无法读取 JPEG 图片尺寸")?;
        ("image/jpeg", w, h)
    } else {
        return Err("请选择 PNG、JPEG 或 BMP 图片".into());
    };
    if width == 0 || height == 0 || width > 32000 || height > 32000 || width * height > 32_000_000 {
        return Err("图片尺寸无效或超过 3200 万像素，请缩小后导入".into());
    }
    Ok(mime)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_and_limit() {
        let mut b = vec![0; 24];
        b[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        b[12..16].copy_from_slice(b"IHDR");
        b[16..20].copy_from_slice(&1920_u32.to_be_bytes());
        b[20..24].copy_from_slice(&1080_u32.to_be_bytes());
        assert_eq!(inspect(&b).unwrap(), "image/png");
        b[16..20].copy_from_slice(&100000_u32.to_be_bytes());
        assert!(inspect(&b).is_err());
        assert!(inspect(b"BM").is_err());
    }
    #[test]
    fn jpeg_and_truncation() {
        let b = [0xff, 0xd8, 0xff, 0xc0, 0, 8, 8, 0, 10, 0, 20, 1];
        assert_eq!(inspect(&b).unwrap(), "image/jpeg");
        assert!(inspect(&b[..9]).is_err());
    }
}
