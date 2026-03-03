//! Multi-modal media attachments.
//!
//! Events can carry media (images, audio, video) either inline
//! (for small files) or as external references (for large files).

use serde::{Deserialize, Serialize};

/// Threshold for inline vs external storage (10 MB).
pub const INLINE_THRESHOLD_BYTES: u64 = 10 * 1024 * 1024;

/// A media attachment on an event.
///
/// Small media (< 10MB) is stored inline as bytes. Large media
/// is uploaded to object storage and referenced by URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaAttachment {
    /// MIME type (e.g., "image/jpeg", "audio/ogg", "video/mp4").
    pub media_type: String,

    /// Inline bytes for small media. `None` if stored externally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_blob: Option<Vec<u8>>,

    /// URI for externally stored media (e.g., S3 path). `None` if inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ref: Option<String>,

    /// Total size in bytes (always present regardless of storage mode).
    pub size_bytes: u64,
}

impl MediaAttachment {
    /// Create an inline media attachment. If the data exceeds
    /// [`INLINE_THRESHOLD_BYTES`], consider using [`MediaAttachment::external`].
    pub fn inline(media_type: impl Into<String>, data: Vec<u8>) -> Self {
        let size = data.len() as u64;
        Self {
            media_type: media_type.into(),
            inline_blob: Some(data),
            external_ref: None,
            size_bytes: size,
        }
    }

    /// Create an externally referenced media attachment.
    pub fn external(media_type: impl Into<String>, uri: impl Into<String>, size_bytes: u64) -> Self {
        Self {
            media_type: media_type.into(),
            inline_blob: None,
            external_ref: Some(uri.into()),
            size_bytes,
        }
    }

    /// Convenience: JPEG image from raw bytes.
    pub fn jpeg(data: Vec<u8>) -> Self {
        Self::inline("image/jpeg", data)
    }

    /// Convenience: OGG audio from raw bytes.
    pub fn audio_ogg(data: Vec<u8>) -> Self {
        Self::inline("audio/ogg", data)
    }

    /// Whether the media is stored inline.
    pub fn is_inline(&self) -> bool {
        self.inline_blob.is_some()
    }

    /// Whether the data exceeds the recommended inline threshold.
    pub fn exceeds_inline_threshold(&self) -> bool {
        self.size_bytes > INLINE_THRESHOLD_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_media() {
        let media = MediaAttachment::jpeg(vec![0xFF, 0xD8, 0xFF]);
        assert!(media.is_inline());
        assert_eq!(media.media_type, "image/jpeg");
        assert_eq!(media.size_bytes, 3);
        assert!(media.external_ref.is_none());
    }

    #[test]
    fn external_media() {
        let media = MediaAttachment::external("video/mp4", "s3://bucket/video.mp4", 500_000_000);
        assert!(!media.is_inline());
        assert!(media.exceeds_inline_threshold());
        assert_eq!(media.external_ref.as_deref(), Some("s3://bucket/video.mp4"));
    }

    #[test]
    fn threshold_check() {
        let small = MediaAttachment::inline("audio/ogg", vec![0; 1000]);
        assert!(!small.exceeds_inline_threshold());

        let large = MediaAttachment::external("video/mp4", "s3://x", 20 * 1024 * 1024);
        assert!(large.exceeds_inline_threshold());
    }
}
