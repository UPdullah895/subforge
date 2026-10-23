//! Text-encoding detection and decoding for subtitle files.

use chardetng::EncodingDetector;
use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};
use serde::Serialize;

/// The encoding a file was decoded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DetectedEncoding {
    /// WHATWG encoding name, e.g. `UTF-8`, `UTF-16LE`, `windows-1256`.
    pub name: &'static str,
    /// Whether the file started with a byte-order mark (which is stripped from the text).
    pub had_bom: bool,
}

/// Decode `bytes` into text, detecting the encoding.
///
/// Order: BOM (UTF-8 / UTF-16LE / UTF-16BE), then valid UTF-8, then statistical detection.
pub fn decode(bytes: &[u8]) -> (String, DetectedEncoding) {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (text, _) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return (
            text.into_owned(),
            DetectedEncoding {
                name: bom_name(enc),
                had_bom: true,
            },
        );
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return (
            text.to_owned(),
            DetectedEncoding {
                name: UTF_8.name(),
                had_bom: false,
            },
        );
    }

    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    let enc = detector.guess(None, true);
    let (text, _) = enc.decode_without_bom_handling(bytes);
    (
        text.into_owned(),
        DetectedEncoding {
            name: enc.name(),
            had_bom: false,
        },
    )
}

fn bom_name(enc: &'static Encoding) -> &'static str {
    if enc == UTF_16LE {
        UTF_16LE.name()
    } else if enc == UTF_16BE {
        UTF_16BE.name()
    } else {
        UTF_8.name()
    }
}
