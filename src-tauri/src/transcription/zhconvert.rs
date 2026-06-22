//! Simplified → Traditional Chinese conversion (OpenCC `s2twp`).
//!
//! whisper.cpp and the Belle Chinese fine-tunes emit **Simplified** Chinese, but
//! the target users are Taiwanese and want **Traditional** Chinese. OpenCC's
//! `s2twp` profile converts Simplified → Traditional (Taiwan standard) *and*
//! localizes vocabulary (e.g. 数据库 → 資料庫), which is the standard ASR
//! post-processing step.
//!
//! Pure-Rust (`opencc-fmmseg`, bundled dictionaries) so there's no C++/native
//! dependency. Gated behind the `opencc` cargo feature; without it, the
//! functions are no-ops and the transcript stays as whisper produced it.

use crate::models::TranscriptSegment;

/// Convert each segment's text from Simplified to Traditional Chinese (Taiwan,
/// with localized phrases). Builds the converter once for the whole batch.
#[cfg(feature = "opencc")]
pub fn segments_to_traditional(segments: &mut [TranscriptSegment]) {
    let cc = opencc_fmmseg::OpenCC::new();
    for s in segments.iter_mut() {
        s.text = cc.convert(&s.text, "s2twp", false);
    }
}

/// No-op without the `opencc` feature.
#[cfg(not(feature = "opencc"))]
pub fn segments_to_traditional(_segments: &mut [TranscriptSegment]) {}

#[cfg(all(test, feature = "opencc"))]
mod tests {
    use super::*;
    use crate::models::TranscriptSegment;

    fn seg(text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: "x".into(),
            meeting_id: "m".into(),
            segment_index: 0,
            start_time_ms: 0,
            end_time_ms: 0,
            text: text.into(),
            speaker: None,
            confidence: None,
            language: Some("zh".into()),
            created_at: String::new(),
        }
    }

    #[test]
    fn converts_simplified_to_traditional_taiwan() {
        // 简体 "数据库" → 台灣正體 "資料庫" (vocab localization via s2twp).
        let mut segs = vec![seg("我们在测试数据库")];
        segments_to_traditional(&mut segs);
        assert_eq!(segs[0].text, "我們在測試資料庫");
    }
}
