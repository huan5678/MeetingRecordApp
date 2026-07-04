//! Speaker diarization via sherpa-onnx (segmentation + speaker embedding),
//! behind cargo feature `diarize`.
//!
//! sherpa-onnx produces a set of **speaker turns** — time ranges each labelled
//! with a speaker id. We then map those labels onto whisper's text segments by
//! maximal temporal overlap (PRD §4.5: "align with whisper segments, write into
//! `transcript_segments.speaker`").
//!
//! Design split:
//! - [`Diarizer::diarize`] runs the native sherpa-onnx model (feature-gated;
//!   stubbed to an empty turn list otherwise). **Unverifiable** here.
//! - [`assign_speakers`] is the **pure** overlap-alignment function and is
//!   heavily unit-tested with fake turns/segments. This is where the real logic
//!   lives, so it is exercised regardless of the `diarize` feature.

use std::path::PathBuf;

use super::whisper::RawSegment;
use super::Result;

/// A contiguous span of audio attributed to one speaker by diarization. Times
/// are milliseconds from the start of the audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerTurn {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Cluster index assigned by sherpa-onnx (0-based).
    pub speaker_id: u32,
}

impl SpeakerTurn {
    /// Overlap (in ms, never negative) between this turn and `[start_ms, end_ms)`.
    pub fn overlap_ms(&self, start_ms: i64, end_ms: i64) -> i64 {
        let lo = self.start_ms.max(start_ms);
        let hi = self.end_ms.min(end_ms);
        (hi - lo).max(0)
    }
}

/// Format a 0-based cluster index as a stable, human-friendly label
/// (`"Speaker 1"`, `"Speaker 2"`, …) for storage in
/// `transcript_segments.speaker`.
pub fn speaker_label(speaker_id: u32) -> String {
    format!("Speaker {}", speaker_id + 1)
}

/// Assign a speaker label to each whisper segment by maximal temporal overlap
/// with the diarization turns.
///
/// Rules:
/// - For each segment, the speaker whose turns overlap it the most (by total
///   overlapping milliseconds) wins.
/// - Ties break toward the **lower** speaker id for determinism.
/// - A segment that overlaps **no** turn (e.g. silence-padded edges, or no
///   diarization at all) gets `None` — callers leave `transcript_segments.speaker`
///   NULL, which is valid per the schema.
///
/// Returns one `Option<String>` per input segment, in order. This function is
/// pure: same inputs → same outputs, no I/O.
pub fn assign_speakers(segments: &[RawSegment], turns: &[SpeakerTurn]) -> Vec<Option<String>> {
    segments
        .iter()
        .map(|seg| best_speaker_for(seg.start_ms, seg.end_ms, turns).map(speaker_label))
        .collect()
}

/// Overlay diarization turns onto transcript segments in place: where a turn
/// covers a segment, override its `speaker` with `"Speaker N"`. Segments with
/// no overlapping turn keep their existing `speaker` — so a Gemini-provided
/// label survives when diarization is unavailable (i.e. the `diarize` feature
/// is off → empty `turns` → no-op, no regression). Used by the Gemini path,
/// where diarization owns segmentation but Gemini text is the fallback labeller.
pub fn apply_turns_to_segments(
    segments: &mut [crate::models::TranscriptSegment],
    turns: &[SpeakerTurn],
) {
    for seg in segments.iter_mut() {
        if let Some(id) = best_speaker_for(seg.start_time_ms, seg.end_time_ms, turns) {
            seg.speaker = Some(speaker_label(id));
        }
    }
}

/// Find the speaker id with the greatest total overlap against
/// `[start_ms, end_ms)`. Returns `None` if nothing overlaps.
fn best_speaker_for(start_ms: i64, end_ms: i64, turns: &[SpeakerTurn]) -> Option<u32> {
    // Accumulate overlap per speaker id. A meeting has few speakers, so a small
    // Vec keyed by id is cheaper and more cache-friendly than a hash map and
    // keeps tie-breaking on the lower id trivial.
    let mut totals: Vec<(u32, i64)> = Vec::new();
    for turn in turns {
        let ov = turn.overlap_ms(start_ms, end_ms);
        if ov == 0 {
            continue;
        }
        match totals.iter_mut().find(|(id, _)| *id == turn.speaker_id) {
            Some((_, acc)) => *acc += ov,
            None => totals.push((turn.speaker_id, ov)),
        }
    }

    totals
        .into_iter()
        // Max by overlap; on equal overlap prefer the lower speaker id. Because
        // max_by returns the *last* maximal element, we invert the id in the
        // comparison key so the smallest id wins ties.
        .max_by(|(id_a, ov_a), (id_b, ov_b)| {
            ov_a.cmp(ov_b).then(id_b.cmp(id_a))
        })
        .map(|(id, _)| id)
}

/// Configuration for the diarizer: paths to the sherpa-onnx models and an
/// optional fixed speaker count.
#[derive(Debug, Clone)]
pub struct DiarizeConfig {
    /// Segmentation model (e.g. pyannote segmentation ONNX).
    pub segmentation_model: PathBuf,
    /// Speaker-embedding model (e.g. 3D-Speaker / wespeaker ONNX).
    pub embedding_model: PathBuf,
    /// If `Some(n)`, cluster into exactly `n` speakers; if `None`, let the
    /// backend estimate the count.
    pub num_speakers: Option<u32>,
}

/// Runs sherpa-onnx diarization over 16 kHz mono PCM.
pub struct Diarizer {
    #[allow(dead_code)]
    config: DiarizeConfig,
}

impl Diarizer {
    pub fn new(config: DiarizeConfig) -> Self {
        Diarizer { config }
    }

    /// Produce speaker turns for a 16 kHz mono `f32` PCM buffer.
    ///
    /// With the `diarize` feature **off**, this returns an empty turn list (a
    /// no-op): every segment then aligns to `None`, leaving speakers unlabeled
    /// — the documented degraded behaviour, not an error, so the transcription
    /// pipeline still completes.
    #[cfg(feature = "diarize")]
    pub fn diarize(&self, pcm_16k_mono: &[f32]) -> Result<Vec<SpeakerTurn>> {
        use sherpa_rs::diarize::{Diarize, DiarizeConfig as SherpaConfig};

        let config = SherpaConfig {
            num_clusters: self.config.num_speakers.map(|n| n as i32).unwrap_or(-1),
            ..Default::default()
        };
        let mut engine = Diarize::new(
            self.config
                .segmentation_model
                .to_string_lossy()
                .into_owned(),
            self.config.embedding_model.to_string_lossy().into_owned(),
            config,
        )
        .map_err(|e| super::TranscriptionError::Engine(format!("sherpa init: {e}")))?;

        let segments = engine
            .compute(pcm_16k_mono.to_vec(), |_processed, _total| {})
            .map_err(|e| super::TranscriptionError::Engine(format!("sherpa compute: {e}")))?;

        Ok(segments
            .into_iter()
            .map(|s| SpeakerTurn {
                // sherpa reports seconds (f32); convert to ms.
                start_ms: (s.start * 1000.0).round() as i64,
                end_ms: (s.end * 1000.0).round() as i64,
                speaker_id: s.speaker.max(0) as u32,
            })
            .collect())
    }

    /// Stub: without the `diarize` feature there are no speaker turns.
    #[cfg(not(feature = "diarize"))]
    pub fn diarize(&self, _pcm_16k_mono: &[f32]) -> Result<Vec<SpeakerTurn>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: i64, end_ms: i64) -> RawSegment {
        RawSegment {
            start_ms,
            end_ms,
            text: "hi".into(),
            confidence: None,
        }
    }

    fn turn(start_ms: i64, end_ms: i64, speaker_id: u32) -> SpeakerTurn {
        SpeakerTurn {
            start_ms,
            end_ms,
            speaker_id,
        }
    }

    #[test]
    fn label_is_one_based() {
        assert_eq!(speaker_label(0), "Speaker 1");
        assert_eq!(speaker_label(3), "Speaker 4");
    }

    fn tseg(start_ms: i64, end_ms: i64, speaker: Option<&str>) -> crate::models::TranscriptSegment {
        crate::models::TranscriptSegment {
            id: String::new(),
            meeting_id: String::new(),
            segment_index: 0,
            start_time_ms: start_ms,
            end_time_ms: end_ms,
            text: "hi".into(),
            speaker: speaker.map(str::to_string),
            confidence: None,
            language: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn apply_turns_overrides_where_covered_and_keeps_the_rest() {
        // seg0 fully inside turn0; seg1 mostly turn1; seg2 has no turn coverage.
        let mut segs = vec![
            tseg(0, 1000, Some("講者 1")),
            tseg(1000, 2000, Some("講者 1")),
            tseg(5000, 6000, Some("講者 2")),
        ];
        let turns = vec![turn(0, 1000, 0), turn(1000, 2000, 1)];

        apply_turns_to_segments(&mut segs, &turns);

        assert_eq!(segs[0].speaker.as_deref(), Some("Speaker 1")); // overridden
        assert_eq!(segs[1].speaker.as_deref(), Some("Speaker 2")); // overridden
        assert_eq!(segs[2].speaker.as_deref(), Some("講者 2")); // no turn → kept (no regression)
    }

    #[test]
    fn apply_turns_empty_is_a_noop() {
        let mut segs = vec![tseg(0, 1000, Some("講者 1")), tseg(1000, 2000, None)];
        apply_turns_to_segments(&mut segs, &[]); // diarize feature off → no turns
        assert_eq!(segs[0].speaker.as_deref(), Some("講者 1")); // Gemini label survives
        assert_eq!(segs[1].speaker, None);
    }

    #[test]
    fn overlap_basic() {
        let t = turn(1000, 3000, 0);
        assert_eq!(t.overlap_ms(2000, 4000), 1000); // [2000,3000)
        assert_eq!(t.overlap_ms(0, 1000), 0); // touches but no overlap
        assert_eq!(t.overlap_ms(5000, 6000), 0); // disjoint
        assert_eq!(t.overlap_ms(1500, 2500), 1000); // fully inside
    }

    #[test]
    fn single_turn_covers_segment() {
        let segs = vec![seg(0, 1000)];
        let turns = vec![turn(0, 2000, 0)];
        assert_eq!(assign_speakers(&segs, &turns), vec![Some("Speaker 1".into())]);
    }

    #[test]
    fn segment_with_no_overlap_is_none() {
        let segs = vec![seg(5000, 6000)];
        let turns = vec![turn(0, 1000, 0)];
        assert_eq!(assign_speakers(&segs, &turns), vec![None]);
    }

    #[test]
    fn empty_turns_yields_all_none() {
        let segs = vec![seg(0, 1000), seg(1000, 2000)];
        assert_eq!(assign_speakers(&segs, &[]), vec![None, None]);
    }

    #[test]
    fn majority_overlap_wins() {
        // Segment [0,1000): speaker 0 owns [0,300) (300ms), speaker 1 owns
        // [300,1000) (700ms) → speaker 1.
        let segs = vec![seg(0, 1000)];
        let turns = vec![turn(0, 300, 0), turn(300, 1000, 1)];
        assert_eq!(assign_speakers(&segs, &turns), vec![Some("Speaker 2".into())]);
    }

    #[test]
    fn accumulates_overlap_across_multiple_turns_of_same_speaker() {
        // Speaker 0 has two short turns totalling 600ms; speaker 1 has one
        // 500ms turn. Speaker 0 should win on accumulated overlap.
        let segs = vec![seg(0, 2000)];
        let turns = vec![
            turn(0, 300, 0),
            turn(1000, 1300, 0),
            turn(300, 800, 1),
        ];
        assert_eq!(assign_speakers(&segs, &turns), vec![Some("Speaker 1".into())]);
    }

    #[test]
    fn ties_break_to_lower_speaker_id() {
        // Equal 500ms overlap each → lower id (speaker 0) wins deterministically.
        let segs = vec![seg(0, 1000)];
        let turns = vec![turn(0, 500, 1), turn(500, 1000, 0)];
        assert_eq!(assign_speakers(&segs, &turns), vec![Some("Speaker 1".into())]);
    }

    #[test]
    fn multiple_segments_each_aligned_independently() {
        let segs = vec![seg(0, 1000), seg(1000, 2000), seg(2000, 3000)];
        let turns = vec![turn(0, 1000, 0), turn(1000, 2000, 1), turn(2000, 3000, 0)];
        assert_eq!(
            assign_speakers(&segs, &turns),
            vec![
                Some("Speaker 1".into()),
                Some("Speaker 2".into()),
                Some("Speaker 1".into()),
            ]
        );
    }

    #[test]
    fn zero_length_segment_overlaps_nothing() {
        // A degenerate [t,t) segment has no positive overlap with anything.
        let segs = vec![seg(1000, 1000)];
        let turns = vec![turn(0, 2000, 0)];
        assert_eq!(assign_speakers(&segs, &turns), vec![None]);
    }

    #[cfg(not(feature = "diarize"))]
    #[test]
    fn diarize_stub_returns_no_turns() {
        let d = Diarizer::new(DiarizeConfig {
            segmentation_model: "/seg.onnx".into(),
            embedding_model: "/emb.onnx".into(),
            num_speakers: None,
        });
        assert!(d.diarize(&[0.0f32; 16]).unwrap().is_empty());
    }
}
