//! The tests of the frame conversion, the sound formats, the resampler and the clock.

use super::audio::{audio_to_f32, map_channels, Resampler};
use super::decode::{codec_registry, coefficients, map_rot, to_rgba};
use super::demux::{format_registry, Media};
use super::present::WallClock;
use rff_core::{CodecId, ColorInfo, ColorRange, MediaType, PixelFormat, Rational,
    VideoFrame as RawFrame};
use rff_format::Stream;
use std::time::Duration;


/// The codecs the owner's archive actually holds: H.264/HEVC/AAC and, from a Panasonic
/// Lumix, MJPEG video in a `.MOV` with PCM sound. The registry is the only place that
/// decides; a missing entry is an honest "no decoder" in the viewer.
#[test]
fn the_registry_holds_the_codecs_of_the_archive() {
    let reg = codec_registry();
    for id in [
        CodecId::H264,
        CodecId::Hevc,
        CodecId::Aac,
        CodecId::Jpeg,
        CodecId::Pcm,
    ] {
        assert!(
            reg.find_decoder(id).is_ok(),
            "no decoder registered for {id:?}"
        );
    }
}

fn stream(w: u32, h: u32, matrix: u16, range: ColorRange) -> Stream {
    let mut s = Stream::new(0, CodecId::H264);
    s.media_type = MediaType::Video;
    s.time_base = Rational::new(1, 30);
    s.width = w;
    s.height = h;
    s.color = ColorInfo {
        primaries: 1,
        transfer: 1,
        matrix,
    };
    s.color_range = range;
    s
}

fn yuv420(w: usize, h: usize, y: u8, u: u8, v: u8) -> RawFrame {
    RawFrame {
        width: w as u32,
        height: h as u32,
        format: PixelFormat::Yuv420p,
        planes: vec![
            vec![y; w * h],
            vec![u; (w / 2) * (h / 2)],
            vec![v; (w / 2) * (h / 2)],
        ],
        strides: vec![w, w / 2, w / 2],
        pts: Some(0),
    }
}

#[test]
fn a_black_frame_stays_black() {
    // The limited range: Y=16, Cb=Cr=128 is black.
    let f = yuv420(4, 2, 16, 128, 128);
    let out = to_rgba(&f, &stream(4, 2, 1, ColorRange::Limited), 0).unwrap();
    assert_eq!((out.width, out.height), (4, 2));
    for px in out.rgba.chunks(4) {
        assert!(px[0] <= 1 && px[1] <= 1 && px[2] <= 1, "not black: {px:?}");
    }
}

#[test]
fn a_white_frame_stays_white() {
    let f = yuv420(4, 2, 235, 128, 128);
    let out = to_rgba(&f, &stream(4, 2, 1, ColorRange::Limited), 0).unwrap();
    for px in out.rgba.chunks(4) {
        assert!(px[0] >= 254 && px[1] >= 254 && px[2] >= 254, "not white: {px:?}");
    }
}

#[test]
fn the_full_range_reads_differently() {
    let f = yuv420(2, 2, 16, 128, 128);
    let limited = to_rgba(&f, &stream(2, 2, 1, ColorRange::Limited), 0).unwrap();
    let full = to_rgba(&f, &stream(2, 2, 1, ColorRange::Full), 0).unwrap();
    assert!(limited.rgba[0] <= 1, "limited Y=16 must be black");
    assert!(full.rgba[0] >= 14, "in the full range Y=16 is dark grey");
}

#[test]
fn a_90_degree_rotation_swaps_the_sides() {
    let f = yuv420(4, 2, 128, 128, 128);
    let mut s = stream(4, 2, 1, ColorRange::Limited);
    s.display_matrix = Some([0, 1 << 16, 0, -(1 << 16), 0, 0, 0, 0, 1 << 30]);
    let out = to_rgba(&f, &s, 0).unwrap();
    assert_eq!((out.width, out.height), (2, 4));
}

#[test]
fn the_rotation_map_agrees_with_the_sizes() {
    // 90°: the bottom-left corner of the source becomes the top-left one.
    assert_eq!(map_rot(90, 4, 2, 0, 0), (0, 1));
    assert_eq!(map_rot(90, 4, 2, 1, 3), (3, 0));
    assert_eq!(map_rot(180, 4, 2, 0, 0), (3, 1));
    assert_eq!(map_rot(270, 4, 2, 0, 0), (3, 0));
    assert_eq!(map_rot(0, 4, 2, 2, 1), (2, 1));
}

#[test]
fn an_unknown_frame_format_is_not_promised() {
    let mut f = yuv420(2, 2, 128, 128, 128);
    f.format = PixelFormat::Yuv444p;
    assert!(to_rgba(&f, &stream(2, 2, 1, ColorRange::Limited), 0).is_none());
}

#[test]
fn the_matrix_from_the_container_beats_the_size() {
    let hd = stream(1920, 1080, 2, ColorRange::Limited);
    let sd = stream(640, 480, 2, ColorRange::Limited);
    assert_eq!(coefficients(&hd).rv, 459, "HD without a tag is BT.709");
    assert_eq!(coefficients(&sd).rv, 409, "SD without a tag is BT.601");
    let tagged = stream(640, 480, 1, ColorRange::Limited);
    assert_eq!(coefficients(&tagged).rv, 459, "the 709 tag beats the size");
}

#[test]
fn a_resampler_without_changes_copies() {
    let mut r = Resampler::new(48_000, 2, 48_000, 2);
    let mut out = Vec::new();
    r.push(&[0.1, 0.2, 0.3, 0.4], &mut out);
    assert_eq!(out, vec![0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn the_resampler_doubles_the_rate() {
    let mut r = Resampler::new(24_000, 1, 48_000, 1);
    let mut out = Vec::new();
    r.push(&[0.0, 1.0, 0.0, 1.0], &mut out);
    // Roughly two output samples per input sample.
    assert!((out.len() as i64 - 8).abs() <= 1, "got {}", out.len());
}

#[test]
fn channels_are_laid_out_by_the_device() {
    let mut mono = [0.0];
    map_channels(&[0.5], &mut mono);
    assert_eq!(mono[0], 0.5);
    let mut stereo = [0.0, 0.0];
    map_channels(&[0.5], &mut stereo);
    assert_eq!(stereo, [0.5, 0.5]);
    let mut one = [0.0];
    map_channels(&[0.2, 0.4], &mut one);
    assert!((one[0] - 0.3).abs() < 1e-6);
}

#[test]
fn an_f32_audio_packet_reads_as_is() {
    let f = rff_core::AudioFrame {
        sample_rate: 48_000,
        channels: 1,
        format: rff_core::SampleFormat::F32,
        planes: vec![0.25f32.to_le_bytes().to_vec()],
        samples: 1,
        pts: Some(0),
    };
    let out = audio_to_f32(&f);
    assert_eq!(out.len(), 1);
    assert!((out[0] - 0.25).abs() < 1e-6);
}

#[test]
fn s16_audio_is_converted_to_f32() {
    let f = rff_core::AudioFrame {
        sample_rate: 44_100,
        channels: 1,
        format: rff_core::SampleFormat::S16,
        planes: vec![i16::MAX.to_le_bytes().to_vec()],
        samples: 1,
        pts: None,
    };
    let out = audio_to_f32(&f);
    assert!((out[0] - 1.0).abs() < 0.001);
}

#[test]
fn the_clock_counts_the_position_and_the_pause() {
    let mut w = WallClock::new();
    w.set(10.0);
    assert!((w.position() - 10.0).abs() < 1e-6);
    w.play();
    std::thread::sleep(Duration::from_millis(20));
    assert!(w.position() > 10.0);
    w.pause();
    let p = w.position();
    std::thread::sleep(Duration::from_millis(20));
    assert!((w.position() - p).abs() < 1e-6, "the position does not grow on pause");
}

#[test]
fn the_registry_knows_the_corpus_codecs() {
    let reg = codec_registry();
    for id in [CodecId::H264, CodecId::Hevc, CodecId::Aac] {
        assert!(reg.find_decoder(id).is_ok(), "no decoder for {id:?}");
    }
}

#[test]
fn the_duration_comes_from_the_container() {
    let mut s = stream(1920, 1080, 1, ColorRange::Limited);
    s.duration = Some(90); // 90 ticks of 1/30 s = 3 s
    let media = Media {
        demuxer: format_registry().open_demuxer("mp4", Box::new(std::io::empty())).unwrap(),
        video: Some(s),
        audio: None,
    };
    assert!((media.duration_s() - 3.0).abs() < 1e-9);
}
