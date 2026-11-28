//! The container: opening a file, its streams and the packets of one stream.

use anyhow::{Context, Result};
use rff_core::{MediaType, Packet};
use rff_format::{Demuxer, FormatRegistry, Stream};
use std::path::Path;

/// An open container: the demuxer and the streams found.
pub(super) struct Media {
    pub(super) demuxer: Box<dyn Demuxer>,
    pub(super) video: Option<Stream>,
    pub(super) audio: Option<Stream>,
}

impl Media {
    pub(super) fn open(path: &Path) -> Result<Media> {
        let reg = format_registry();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let name = reg
            .by_extension(&ext)
            .map(|f| f.name)
            .or_else(|| {
                // An unfamiliar extension: try by the content.
                let mut head = [0u8; 64];
                let n = std::fs::File::open(path)
                    .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
                    .unwrap_or(0);
                reg.probe(&head[..n]).map(|f| f.name)
            })
            .with_context(|| format!("unknown video format: {}", path.display()))?;

        let file = std::fs::File::open(path)
            .with_context(|| format!("cannot open the video {}", path.display()))?;
        let mut demuxer = reg
            .open_demuxer_seekable(name, Box::new(file))
            .or_else(|_| {
                let f = std::fs::File::open(path)?;
                reg.open_demuxer(name, Box::new(f))
            })
            .with_context(|| format!("cannot parse the container {}", path.display()))?;
        let streams = demuxer
            .read_header()
            .with_context(|| format!("the container has no header: {}", path.display()))?;
        let video = streams.iter().find(|s| s.media_type == MediaType::Video).cloned();
        let audio = streams.iter().find(|s| s.media_type == MediaType::Audio).cloned();
        if video.is_none() && audio.is_none() {
            anyhow::bail!("the file has neither video nor sound: {}", path.display());
        }
        Ok(Media {
            demuxer,
            video,
            audio,
        })
    }

    pub(super) fn duration_s(&self) -> f64 {
        let of = |s: &Stream| {
            s.duration
                .map(|d| d as f64 * s.time_base.as_f64())
                .unwrap_or(0.0)
        };
        self.video
            .as_ref()
            .map(of)
            .filter(|d| *d > 0.0)
            .or_else(|| self.audio.as_ref().map(of).filter(|d| *d > 0.0))
            .unwrap_or(0.0)
    }
}

pub(super) fn format_registry() -> FormatRegistry {
    let mut reg = FormatRegistry::new();
    rff_format_mp4::register(&mut reg);
    rff_format_mkv::register(&mut reg);
    reg
}

/// Which stream is wanted: video or audio.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Video,
    Audio,
}

/// Where the packets come from: the sample index (MP4/MKV) or sequential reading.
pub(super) enum Source {
    Stream,
    Index { v: u64, a: u64 },
}

pub(super) fn new_source(media: &mut Media) -> Source {
    if media.demuxer.sample_index().is_some() {
        Source::Index { v: 0, a: 0 }
    } else {
        Source::Stream
    }
}

/// The next packet of the wanted stream. In index mode the video and the audio have
/// their own cursors, so a full frame queue does not stop the sound and vice versa.
pub(super) fn next_for(media: &mut Media, source: &mut Source, kind: Kind) -> Option<Packet> {
    match source {
        Source::Stream => loop {
            let pkt = media.demuxer.read_packet().ok()?;
            let want = match kind {
                Kind::Video => media.video.as_ref(),
                Kind::Audio => media.audio.as_ref(),
            };
            if want.is_some_and(|s| s.index == pkt.stream_index) {
                return Some(pkt);
            }
        },
        Source::Index { v, a } => {
            let idx = media.demuxer.sample_index()?;
            let (stream, cursor) = match kind {
                Kind::Video => (media.video.as_ref()?.index, v),
                Kind::Audio => (media.audio.as_ref()?.index, a),
            };
            if *cursor >= idx.sample_count(stream) {
                return None;
            }
            let pkt = idx.read_sample(stream, *cursor).ok()?;
            *cursor += 1;
            Some(pkt)
        }
    }
}
