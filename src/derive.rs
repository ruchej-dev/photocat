#![allow(dead_code)]
//! Извлечение метаданных: sha256, перцептивный dHash, EXIF (дата, GPS, камера), превью.

use anyhow::{Context, Result};
use image::GenericImageView;
use image_hasher::{HashAlg, HasherConfig};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

pub const IMG_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "avif",
];
pub const VID_EXT: &[&str] = &["mp4", "mov", "avi", "mkv", "m4v", "3gp", "webm", "mts"];

pub fn kind_of(p: &Path) -> Option<&'static str> {
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    if IMG_EXT.contains(&ext.as_str()) {
        Some("image")
    } else if VID_EXT.contains(&ext.as_str()) {
        Some("video")
    } else {
        None
    }
}

pub fn sha256_file(p: &Path) -> Result<String> {
    let mut f = BufReader::with_capacity(1 << 20, File::open(p)?);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

/// Перцептивный хеш 64 бита (dHash = gradient) — реагирует на структуру, не на цвет.
pub fn dhash(p: &Path) -> Result<u64> {
    let img = image::ImageReader::open(p)?
        .with_guessed_format()?
        .decode()?;
    Ok(dhash_of(&img.to_luma8()))
}

fn dhash_of(gray: &image::GrayImage) -> u64 {
    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::Gradient)
        .hash_size(8, 8)
        .to_hasher();
    let h = hasher.hash_image(gray);
    let mut v = 0u64;
    for b in h.as_bytes() {
        v = (v << 8) | *b as u64;
    }
    v
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Резкость: дисперсия лапласиана по серому. Чем выше, тем больше в кадре чётких границ.
/// Считается на копии не больше 512 px по длинной стороне, чтобы значения снимков разного
/// разрешения были сравнимы между собой.
pub fn sharpness(gray: &image::GrayImage) -> f32 {
    const MAX_SIDE: u32 = 512;
    let (w, h) = gray.dimensions();
    let scale = (MAX_SIDE as f32 / w.max(h) as f32).min(1.0);
    let small = if scale < 1.0 {
        let nw = ((w as f32 * scale).round() as u32).max(1);
        let nh = ((h as f32 * scale).round() as u32).max(1);
        image::imageops::resize(gray, nw, nh, image::imageops::FilterType::Triangle)
    } else {
        gray.clone()
    };
    let (w, h) = small.dimensions();
    if w < 3 || h < 3 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut sum2 = 0.0f64;
    let mut n = 0u64;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let c = small.get_pixel(x, y)[0] as f64;
            let l = small.get_pixel(x - 1, y)[0] as f64;
            let r = small.get_pixel(x + 1, y)[0] as f64;
            let u = small.get_pixel(x, y - 1)[0] as f64;
            let d = small.get_pixel(x, y + 1)[0] as f64;
            let lap = 4.0 * c - l - r - u - d;
            sum += lap;
            sum2 += lap * lap;
            n += 1;
        }
    }
    let mean = sum / n as f64;
    (sum2 / n as f64 - mean * mean).max(0.0) as f32
}

/// Метаданные из EXIF.
#[derive(Default, Debug, Clone)]
pub struct Meta {
    pub taken_at: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub camera: Option<String>,
}

pub fn exif_meta(p: &Path) -> Meta {
    let mut out = Meta::default();
    let Ok(f) = File::open(p) else { return out };
    let r = exif::Reader::new();
    let Ok(exif) = r.read_from_container(&mut BufReader::new(f)) else {
        return out;
    };
    if let Some(fld) = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
        let s = fld.display_value().to_string();
        out.taken_at = parse_exif_dt(&s);
    }
    let lat = exif
        .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
        .and_then(|f| to_deg(&f.value))
        .map(|v| {
            if gps_neg(&exif, exif::Tag::GPSLatitudeRef, 'S') {
                -v
            } else {
                v
            }
        });
    let lon = exif
        .get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY)
        .and_then(|f| to_deg(&f.value))
        .map(|v| {
            if gps_neg(&exif, exif::Tag::GPSLongitudeRef, 'W') {
                -v
            } else {
                v
            }
        });
    if let (Some(a), Some(b)) = (lat, lon) {
        out.lat = Some(a);
        out.lon = Some(b);
    }
    let make = exif
        .get_field(exif::Tag::Make, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());
    let model = exif
        .get_field(exif::Tag::Model, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());
    out.camera = match (make, model) {
        (Some(a), Some(b)) => Some(if b.contains(a.trim()) { b.trim().to_string() } else { format!("{} {}", a.trim(), b.trim()) }),
        (Some(a), None) => Some(a.trim().to_string()),
        (None, Some(b)) => Some(b.trim().to_string()),
        _ => None,
    };
    out
}

fn gps_neg(exif: &exif::Exif, tag: exif::Tag, neg: char) -> bool {
    exif.get_field(tag, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string().to_ascii_uppercase().starts_with(neg))
        .unwrap_or(false)
}

fn to_deg(v: &exif::Value) -> Option<f64> {
    match v {
        exif::Value::Rational(r) if r.len() == 3 => Some(
            r[0].to_f64() + r[1].to_f64() / 60.0 + r[2].to_f64() / 3600.0,
        ),
        _ => None,
    }
}

fn parse_exif_dt(s: &str) -> Option<String> {
    // kamadak выдаёт "2022-09-01 08:47:14"
    let s = s.trim();
    let (d, t) = s.split_once(' ')?;
    let (y, m, day) = {
        let mut it = d.split('-');
        (it.next()?, it.next()?, it.next()?)
    };
    let (hh, mm, ss) = {
        let mut it = t.split(':');
        (it.next()?, it.next()?, it.next().unwrap_or("00"))
    };
    let ss = ss.split('.').next().unwrap_or("00");
    Some(format!("{y:0>4}-{m:0>2}-{day:0>2}T{hh:0>2}:{mm:0>2}:{ss:0>2}"))
}

/// Дата из имени вида IMG_20240517_144243 / P_20190410_102958 / VID_20230107_203018.
pub fn filename_date(p: &Path) -> Option<String> {
    let name = p.file_name()?.to_str()?;
    let b = name.as_bytes();
    let mut i = 0;
    while i + 15 <= b.len() {
        if b[i..i + 8].iter().all(u8::is_ascii_digit)
            && b.get(i + 8) == Some(&b'_')
            && b[i + 9..i + 15].iter().all(u8::is_ascii_digit)
        {
            let d = &name[i..i + 8];
            let t = &name[i + 9..i + 15];
            let (y, m, day) = (&d[0..4], &d[4..6], &d[6..8]);
            let (hh, mm, ss) = (&t[0..2], &t[2..4], &t[4..6]);
            let ok = plausible(y, m, day, hh, mm);
            if ok {
                return Some(format!("{y}-{m}-{day}T{hh}:{mm}:{ss}"));
            }
        }
        i += 1;
    }
    None
}

fn plausible(y: &str, m: &str, d: &str, hh: &str, mm: &str) -> bool {
    let (Ok(y), Ok(m), Ok(d), Ok(hh), Ok(mm)) = (
        y.parse::<i32>(),
        m.parse::<u32>(),
        d.parse::<u32>(),
        hh.parse::<u32>(),
        mm.parse::<u32>(),
    ) else {
        return false;
    };
    (1990..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) && hh < 24 && mm < 60
}

pub fn mtime_date(p: &Path) -> Option<String> {
    let md = std::fs::metadata(p).ok()?;
    let t = md.modified().ok()?;
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let dt = chrono::DateTime::from_timestamp(secs, 0)?;
    Some(dt.format("%Y-%m-%dT%H:%M:%S").to_string())
}

/// Сохранить уменьшенное превью в кэш. Возвращает имя файла превью относительно кэша.
pub fn save_thumb(src: &Path, thumbs_dir: &Path, key: &str, max_side: u32) -> Result<String> {
    let img = image::ImageReader::open(src)?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("декодирование {}", src.display()))?;
    let (w, h) = img.dimensions();
    let scale = (max_side as f32 / w.max(h) as f32).min(1.0);
    let (nw, nh) = ((w as f32 * scale).round() as u32, (h as f32 * scale).round() as u32);
    let small = img.resize_exact(nw.max(1), nh.max(1), image::imageops::FilterType::Triangle);
    let sub = &key[..2.min(key.len())];
    let dir = thumbs_dir.join(sub);
    std::fs::create_dir_all(&dir)?;
    let rel = format!("{sub}/{key}.jpg");
    let path = thumbs_dir.join(&rel);
    if !path.exists() {
        small.to_rgb8().save_with_format(&path, image::ImageFormat::Jpeg)
            .with_context(|| format!("запись превью {}", path.display()))?;
    }
    Ok(rel)
}

pub fn thumb_path(thumbs_dir: &Path, rel: &str) -> PathBuf {
    thumbs_dir.join(rel)
}

/// Один декод на файл: размеры, dHash, превью.
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub dhash: u64,
    pub sharpness: f32,
    pub thumb: String,
}

pub fn analyze_image(
    src: &Path,
    thumbs_dir: &Path,
    key: &str,
    max_side: u32,
) -> Result<ImageInfo> {
    let img = image::ImageReader::open(src)?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("декодирование {}", src.display()))?;
    let (w, h) = img.dimensions();
    let gray = img.to_luma8();
    let dhash = dhash_of(&gray);
    let scale = (max_side as f32 / w.max(h) as f32).min(1.0);
    let (nw, nh) = (
        (w as f32 * scale).round().max(1.0) as u32,
        (h as f32 * scale).round().max(1.0) as u32,
    );
    let small = img.resize_exact(nw, nh, image::imageops::FilterType::Triangle);
    // Резкость — по тому же уменьшенному кадру, что уходит в превью: дёшево и сравнимо.
    let sharpness = sharpness(&small.to_luma8());
    let sub = &key[..2.min(key.len())];
    let dir = thumbs_dir.join(sub);
    std::fs::create_dir_all(&dir)?;
    let rel = format!("{sub}/{key}.jpg");
    let path = thumbs_dir.join(&rel);
    if !path.exists() {
        small
            .to_rgb8()
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .with_context(|| format!("запись превью {}", path.display()))?;
    }
    Ok(ImageInfo { width: w, height: h, dhash, sharpness, thumb: rel })
}
