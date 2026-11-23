#![allow(dead_code)]
//! Извлечение метаданных: sha256, перцептивный dHash, EXIF (дата, GPS, камера), превью.

use crate::tools::{self, Tool};
use anyhow::{Context, Result};
use image::GenericImageView;
use image_hasher::{HashAlg, HasherConfig};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

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

/// Что ffprobe знает о видео: разрешение с учётом поворота и длительность.
#[derive(Debug, Clone, Copy)]
pub struct VideoMeta {
    pub width: u32,
    pub height: u32,
    pub duration_s: f64,
}

/// Длительность и разрешение через `ffprobe -of json`. Поворот кадра из side data учитывается:
/// у вертикальных видео с телефона иначе ширина и высота меняются местами.
pub fn video_probe(tools: &tools::Tools, src: &Path) -> Result<VideoMeta> {
    let out = tools
        .command(Tool::Ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:stream_tags=rotate:stream_side_data=rotation:format=duration",
            "-of",
            "json",
        ])
        .arg(src)
        .output()
        .with_context(|| format!("запуск ffprobe для {}", src.display()))?;
    if !out.status.success() {
        anyhow::bail!(
            "ffprobe {}: {}",
            src.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    parse_probe(&out.stdout, src)
}

/// Разбор JSON от ffprobe: вынесен отдельно, чтобы проверять поворот без запуска процесса.
fn parse_probe(out: &[u8], src: &Path) -> Result<VideoMeta> {
    let v: serde_json::Value = serde_json::from_slice(out).context("разбор ответа ffprobe")?;
    let stream = &v["streams"][0];
    let mut width = stream["width"].as_u64().unwrap_or(0) as u32;
    let mut height = stream["height"].as_u64().unwrap_or(0) as u32;
    let rotation = stream["side_data_list"][0]["rotation"]
        .as_i64()
        .or_else(|| stream["tags"]["rotate"].as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(0);
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }
    let duration_s = v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| v["format"]["duration"].as_f64())
        .unwrap_or(0.0);
    if width == 0 || height == 0 {
        anyhow::bail!("ffprobe не дал разрешение для {}", src.display());
    }
    Ok(VideoMeta { width, height, duration_s })
}

/// Кадр-обложка из видео: ищем на 10% длительности (быстрый seek до входа читает только нужное).
fn extract_frame(
    tools: &tools::Tools,
    src: &Path,
    dst: &Path,
    seek: f64,
    max_side: u32,
) -> Result<()> {
    let vf = format!("scale={max_side}:{max_side}:force_original_aspect_ratio=decrease");
    let out = tools
        .command(Tool::Ffmpeg)
        .args(["-nostdin", "-v", "error", "-ss", &format!("{seek:.3}"), "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-vf", &vf, "-q:v", "3", "-y"])
        .arg(dst)
        .output()
        .with_context(|| format!("запуск ffmpeg для {}", src.display()))?;
    if !out.status.success() {
        anyhow::bail!(
            "ffmpeg {}: {}",
            src.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    if !dst.exists() {
        anyhow::bail!("ffmpeg не создал кадр для {}", src.display());
    }
    Ok(())
}

/// Полный проход по видео: ffprobe, кадр-обложка, dHash и резкость по кадру.
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration_s: f64,
    pub dhash: u64,
    pub sharpness: f32,
    pub thumb: String,
}

pub fn analyze_video(
    tools: &tools::Tools,
    src: &Path,
    thumbs_dir: &Path,
    key: &str,
    max_side: u32,
) -> Result<VideoInfo> {
    let meta = video_probe(tools, src)?;
    let sub = &key[..2.min(key.len())];
    let dir = thumbs_dir.join(sub);
    std::fs::create_dir_all(&dir)?;
    let rel = format!("{sub}/{key}.jpg");
    let path = thumbs_dir.join(&rel);
    let empty = std::fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);
    if empty {
        let seek = if meta.duration_s > 1.0 { meta.duration_s * 0.1 } else { 0.0 };
        if let Err(e) = extract_frame(tools, src, &path, seek, max_side) {
            // У коротких или битых файлов seek может не найти кадр — пробуем с начала.
            if seek > 0.0 {
                extract_frame(tools, src, &path, 0.0, max_side)?;
            } else {
                return Err(e);
            }
        }
    }
    let img = image::ImageReader::open(&path)?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("декодирование кадра {}", path.display()))?;
    let gray = img.to_luma8();
    Ok(VideoInfo {
        width: meta.width,
        height: meta.height,
        duration_s: meta.duration_s,
        dhash: dhash_of(&gray),
        sharpness: sharpness(&gray),
        thumb: rel,
    })
}

/// Установленные языковые пакеты tesseract. `None` — программы нет.
pub fn tesseract_langs(tools: &tools::Tools) -> Option<Vec<String>> {
    let out = tools
        .command(Tool::Tesseract)
        .arg("--list-langs")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(parse_langs(&text))
}

/// Разбор `tesseract --list-langs`: первая строка — заголовок, дальше коды по одному.
fn parse_langs(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .filter(|l| *l != "osd")
        .map(str::to_string)
        .collect()
}

/// Декодировать снимок с учётом EXIF-поворота: всё, что ищет координаты (лица, OCR),
/// должно видеть кадр так же, как человек.
pub fn load_oriented(src: &Path) -> Result<image::DynamicImage> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(src)?
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut img = image::DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Быстрый кадр для полноэкранного просмотра: JPEG декодируется сразу в уменьшенном
/// масштабе через libjpeg-turbo (Linux), поворот EXIF применяется после. Остальные форматы
/// и платформы идут через `load_oriented` + `thumbnail`.
pub fn fast_view(src: &Path, max_side: u32) -> Result<(usize, usize, Vec<u8>)> {
    #[cfg(target_os = "linux")]
    if let Ok(frame) = fast_jpeg(src, max_side) {
        return Ok(frame);
    }
    let img = load_oriented(src)?.thumbnail(max_side, max_side).to_rgba8();
    Ok((
        img.width() as usize,
        img.height() as usize,
        img.into_raw(),
    ))
}

/// DCT-масштабирование TurboJPEG: для 108 МП снимка 1/4 даёт 3008 px за ~0,5 с вместо
/// полного декода 12 032×9 024 и последующего ресайза (~1,3 с).
#[cfg(target_os = "linux")]
fn fast_jpeg(src: &Path, max_side: u32) -> Result<(usize, usize, Vec<u8>)> {
    use turbojpeg::{Decompressor, PixelFormat, ScalingFactor};
    let data = std::fs::read(src)?;
    let mut d = Decompressor::new()?;
    let head = d.read_header(&data)?;
    let long = head.width.max(head.height);
    let target = max_side.max(64) as usize;
    let factor = if long / 8 >= target {
        ScalingFactor::new(1, 8)
    } else if long / 4 >= target {
        ScalingFactor::new(1, 4)
    } else if long / 2 >= target {
        ScalingFactor::new(1, 2)
    } else {
        ScalingFactor::ONE
    };
    d.set_scaling_factor(factor)?;
    let scaled = head.scaled(factor);
    let mut img = turbojpeg::Image {
        pixels: vec![0u8; scaled.width * scaled.height * 4],
        width: scaled.width,
        pitch: scaled.width * 4,
        height: scaled.height,
        format: PixelFormat::RGBA,
    };
    d.decompress(&data, img.as_deref_mut())?;
    let orientation = exif_orientation(src).unwrap_or(1);
    Ok(orient_rgba(scaled.width, scaled.height, img.pixels, orientation))
}

/// EXIF-поворот без декодирования пикселей.
pub fn exif_orientation(src: &Path) -> Option<u8> {
    use std::io::BufReader;
    let file = std::fs::File::open(src).ok()?;
    let r = exif::Reader::new();
    let exif = r.read_from_container(&mut BufReader::new(file)).ok()?;
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?
        .value
        .get_uint(0)
        .map(|v| v as u8)
}

/// Применить EXIF-поворот к уже декодированным RGBA-пикселям.
fn orient_rgba(w: usize, h: usize, data: Vec<u8>, orientation: u8) -> (usize, usize, Vec<u8>) {
    if orientation <= 1 {
        return (w, h, data);
    }
    let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, data) else {
        return (w, h, Vec::new());
    };
    let out = match orientation {
        2 => image::imageops::flip_horizontal(&img),
        3 => image::imageops::rotate180(&img),
        4 => image::imageops::flip_vertical(&img),
        5 => {
            let t = image::imageops::rotate90(&img);
            image::imageops::flip_horizontal(&t)
        }
        6 => image::imageops::rotate90(&img),
        7 => {
            let t = image::imageops::rotate90(&img);
            image::imageops::flip_vertical(&t)
        }
        8 => image::imageops::rotate270(&img),
        _ => img,
    };
    let (w, h) = out.dimensions();
    (w as usize, h as usize, out.into_raw())
}

/// Вход для OCR: повёрнутый по EXIF кадр не больше `max_side`, PNG в памяти.
/// Декодируем сами: `tesseract` не читает EXIF-поворот, а снимок с телефона иначе ляжет боком.
fn ocr_input_png(src: &Path, max_side: u32) -> Result<Vec<u8>> {
    let mut img = load_oriented(src)?;
    if img.width() > max_side || img.height() > max_side {
        img = img.resize(max_side, max_side, image::imageops::FilterType::Triangle);
    }
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .with_context(|| format!("подготовка PNG для OCR {}", src.display()))?;
    Ok(png)
}

fn tesseract_text(tools: &tools::Tools, png: &[u8], lang: &str) -> Result<String> {
    let mut child = tools
        .command(Tool::Tesseract)
        .args(["stdin", "stdout", "-l", lang, "--psm", "3"])
        // Один процесс tesseract не должен занимать все ядра: потоков задаёт наше планирование.
        .env("OMP_THREAD_LIMIT", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("запуск tesseract (нужны tesseract и языки {lang})"))?;
    let mut stdin = child.stdin.take().context("tesseract: stdin недоступен")?;
    let data = png.to_vec();
    // Пишем в stdin параллельно чтению stdout: иначе большой текст упрётся в буфер трубы.
    let writer = std::thread::spawn(move || stdin.write_all(&data));
    let out = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("tesseract: поток записи упал"))??;
    if !out.status.success() {
        anyhow::bail!("tesseract: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Текст с картинки через внешний `tesseract` (ADR 0018).
/// Оригинал только читается: PNG уходит процессу в stdin, временных файлов не появляется.
pub fn ocr_image(
    tools: &tools::Tools,
    src: &Path,
    lang: &str,
    max_side: u32,
) -> Result<String> {
    let png = ocr_input_png(src, max_side)?;
    tesseract_text(tools, &png, lang)
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

/// Вход CLIP: два кадра на снимок — центр-кроп и весь кадр с полями (letterbox).
///
/// Центр-кроп даёт крупный объект, но на вертикальных и широких кадрах отрезает
/// контекст (белые полки без верха и низа CLIP читает как лист бумаги). Весь кадр
/// сохраняет пропорции, поэтому векторы усредняются в `embed`: модель видит и
/// деталь, и обстановку. Превью декодируется один раз на оба кадра.
/// Раскладка — CHW с нормализацией CLIP.
pub fn clip_inputs(thumb: &Path, size: u32) -> Result<(Vec<f32>, Vec<f32>)> {
    let img = image::ImageReader::open(thumb)?
        .with_guessed_format()?
        .decode()
        .with_context(|| format!("декодирование превью {}", thumb.display()))?
        .to_rgb8();
    let (w, h) = img.dimensions();
    let side = w.min(h).max(1);
    let (x0, y0) = ((w - side) / 2, (h - side) / 2);
    let crop = image::imageops::crop_imm(&img, x0, y0, side, side).to_image();
    let center = image::imageops::resize(
        &crop,
        size,
        size,
        image::imageops::FilterType::Triangle,
    );
    Ok((to_chw(&center, size), to_chw(&letterbox(&img, size), size)))
}

/// CLIP-нормализация и раскладка RGB → CHW.
fn to_chw(img: &image::RgbImage, size: u32) -> Vec<f32> {
    const MEAN: [f32; 3] = [0.481_454_7, 0.457_827_5, 0.408_210_7];
    const STD: [f32; 3] = [0.268_629_5, 0.261_302_6, 0.275_777_1];
    let plane = (size * size) as usize;
    let mut out = vec![0f32; plane * 3];
    for y in 0..size {
        for x in 0..size {
            let px = img.get_pixel(x, y);
            let i = (y * size + x) as usize;
            for c in 0..3 {
                out[c * plane + i] = (px[c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    out
}

/// Вписать кадр в квадрат `size`, сохранив пропорции; поля — средний цвет кадра.
/// Средний цвет не вносит в картинку чёрную рамку, которой в архиве нет.
fn letterbox(img: &image::RgbImage, size: u32) -> image::RgbImage {
    let (w, h) = img.dimensions();
    let scale = (size as f32 / w.max(1) as f32).min(size as f32 / h.max(1) as f32);
    let nw = ((w as f32 * scale).round() as u32).clamp(1, size);
    let nh = ((h as f32 * scale).round() as u32).clamp(1, size);
    let small = image::imageops::resize(
        img,
        nw,
        nh,
        image::imageops::FilterType::Triangle,
    );
    let mut canvas = image::RgbImage::from_pixel(size, size, average_color(img));
    image::imageops::overlay(
        &mut canvas,
        &small,
        ((size - nw) / 2) as i64,
        ((size - nh) / 2) as i64,
    );
    canvas
}

/// Средний цвет кадра (по всем пикселям, без прозрачности).
fn average_color(img: &image::RgbImage) -> image::Rgb<u8> {
    let n = (img.width() as u64 * img.height() as u64).max(1);
    let mut sum = [0u64; 3];
    for px in img.pixels() {
        for c in 0..3 {
            sum[c] += px[c] as u64;
        }
    }
    image::Rgb([
        (sum[0] / n) as u8,
        (sum[1] / n) as u8,
        (sum[2] / n) as u8,
    ])
}

#[cfg(test)]
mod tests {
    #[test]
    fn tesseract_langs_parses_header_and_codes() {
        let out = "List of available languages in /usr/share/tessdata/ (3):\neng\nrus\nosd\n";
        assert_eq!(super::parse_langs(out), vec!["eng".to_string(), "rus".to_string()]);
        assert!(super::parse_langs("").is_empty());
    }

    use super::*;

    #[test]
    #[ignore = "нужен PHOTOCAT_BIG_JPEG: путь к тяжёлому JPEG"]
    fn fast_view_быстрее_полного_декода_большого_jpeg() {
        let Some(path) = std::env::var_os("PHOTOCAT_BIG_JPEG").map(std::path::PathBuf::from)
        else {
            return;
        };
        let t0 = std::time::Instant::now();
        let (w, h, raw) = fast_view(&path, 1920).unwrap();
        let fast = t0.elapsed();
        assert!(w > 0 && h > 0 && raw.len() == w * h * 4);
        let t1 = std::time::Instant::now();
        let img = load_oriented(&path).unwrap().thumbnail(1920, 1920);
        assert!(img.width() > 0);
        let full = t1.elapsed();
        println!("fast_view {w}x{h}: {fast:?}; полный декод + thumbnail: {full:?}");
        assert!(fast < full);
    }

    fn probe(json: &str) -> Result<VideoMeta> {
        parse_probe(json.as_bytes(), Path::new("/v.mp4"))
    }

    #[test]
    fn side_data_rotation_swaps_width_and_height() {
        let m = probe(
            r#"{"streams":[{"width":1280,"height":720,
                "side_data_list":[{"rotation":90}]}],"format":{"duration":"12.5"}}"#,
        )
        .unwrap();
        assert_eq!((m.width, m.height), (720, 1280));
        assert!((m.duration_s - 12.5).abs() < 1e-9);
    }

    #[test]
    fn tag_rotation_270_also_swaps() {
        let m = probe(
            r#"{"streams":[{"width":1920,"height":1080,"tags":{"rotate":"270"}}],
                "format":{"duration":"3.0"}}"#,
        )
        .unwrap();
        assert_eq!((m.width, m.height), (1080, 1920));
    }

    #[test]
    fn landscape_without_rotation_is_left_alone() {
        let m = probe(r#"{"streams":[{"width":1920,"height":1080}],"format":{}}"#).unwrap();
        assert_eq!((m.width, m.height), (1920, 1080));
        assert_eq!(m.duration_s, 0.0);
    }

    #[test]
    fn missing_resolution_is_an_error() {
        assert!(probe(r#"{"streams":[{}],"format":{}}"#).is_err());
    }

    #[test]
    fn letterbox_сохраняет_пропорции_и_заливает_поля_средним_цветом() {
        // Вертикальный кадр 100×200: в квадрат 20 он войдёт как 10×20 по центру.
        let tall = image::RgbImage::from_pixel(100, 200, image::Rgb([255, 0, 0]));
        let out = letterbox(&tall, 20);
        assert_eq!(out.dimensions(), (20, 20));
        assert_eq!(out.get_pixel(2, 10), &image::Rgb([255, 0, 0]));
        assert_eq!(out.get_pixel(10, 10), &image::Rgb([255, 0, 0]));
        // Поля слева и справа — средний цвет кадра, а не чёрная рамка.
        assert_eq!(out.get_pixel(1, 10), &image::Rgb([255, 0, 0]));

        // Широкий 200×100: 20×10 по центру, поля сверху и снизу.
        let wide = image::RgbImage::from_pixel(200, 100, image::Rgb([0, 0, 255]));
        let out = letterbox(&wide, 20);
        assert_eq!(out.dimensions(), (20, 20));
        assert_eq!(out.get_pixel(10, 2), &image::Rgb([0, 0, 255]));
        assert_eq!(out.get_pixel(10, 17), &image::Rgb([0, 0, 255]));
    }

    #[test]
    fn вход_clip_даёт_два_кадра_одного_размера() {
        // Временный файл перезаписывается, но не удаляется: архив неприкосновенен,
        // а удаление файлов в `src/` запрещено проверкой границ.
        let path = std::env::temp_dir().join("photocat-clip-inputs.png");
        image::RgbImage::from_pixel(160, 320, image::Rgb([120, 130, 140]))
            .save(&path)
            .unwrap();
        let (center, whole) = clip_inputs(&path, 224).unwrap();
        let plane = (224 * 224) as usize;
        assert_eq!(center.len(), plane * 3);
        assert_eq!(whole.len(), plane * 3);
        // Центр-кроп вертикального кадра — это его середина, не растянутые поля.
        assert!(center.iter().all(|v| v.is_finite()));
        assert!(whole.iter().all(|v| v.is_finite()));
    }
}
