//! Numbers for the interface: sizes and durations.

pub fn human(bytes: i64) -> String {
    human_with(bytes, ["B", "KiB", "MiB", "GiB", "TiB"])
}

/// The same as `human`, but with the units of the interface language (M21).
/// The number format is one, only the unit labels change, so the function lives here,
/// next to `human`, and not in `i18n`.
pub fn human_in(lang: crate::i18n::Lang, bytes: i64) -> String {
    human_with(bytes, crate::i18n::units(lang))
}

fn human_with(bytes: i64, units: [&str; 5]) -> String {
    let b = bytes as f64;
    let mut v = b;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v:.0} {}", units[i])
    } else {
        format!("{v:.1} {}", units[i])
    }
}

/// The video duration: `M:SS`, from an hour `H:MM:SS`. Rounded to the second.
pub fn duration(secs: f64) -> String {
    let total = secs.round().max(0.0) as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
