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
    // Diarization off / produced nothing → leave every Gemini label untouched.
    if turns.is_empty() {
        return;
    }
    // Diarization ran → it OWNS attribution: label every segment "Speaker N" so
    // the transcript never mixes "Speaker N" with Gemini's "講者 N". Prefer the
    // maximal-overlap speaker; for a segment sitting in a diarization gap, fall
    // back to the temporally nearest turn.
    // ponytail: nearest-turn is a heuristic for uncovered gaps; good enough — a
    // wrong guess is one click to rename. Upgrade to re-segmentation if needed.
    for seg in segments.iter_mut() {
        let id = best_speaker_for(seg.start_time_ms, seg.end_time_ms, turns)
            .or_else(|| nearest_speaker_for(seg.start_time_ms, seg.end_time_ms, turns));
        if let Some(id) = id {
            seg.speaker = Some(speaker_label(id));
        }
    }
}

/// Speaker of the turn with the smallest time-gap to `[start_ms, end_ms)`.
/// `None` only when `turns` is empty. Ties break toward the lower speaker id.
fn nearest_speaker_for(start_ms: i64, end_ms: i64, turns: &[SpeakerTurn]) -> Option<u32> {
    turns
        .iter()
        .min_by_key(|t| {
            // Gap distance: 0 if they overlap, else the space between them.
            let gap = (start_ms - t.end_ms).max(t.start_ms - end_ms).max(0);
            (gap, t.speaker_id)
        })
        .map(|t| t.speaker_id)
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

// -- voiceprint memory (Phase 3) ------------------------------------------
//
// Pure helpers for cross-meeting speaker memory. Embedding *extraction* needs
// sherpa (feature-gated, in the worker); these — slicing a cluster's audio and
// matching a fresh embedding against the enrolled library — are pure and tested
// here regardless of feature.

/// Cosine-similarity floor for treating a fresh cluster as a known enrolled
/// speaker. sherpa's own default; tune on Windows for far-field meeting-room
/// audio.
// ponytail: 0.5 is sherpa's default — raise if false matches, lower if misses.
pub const VOICEPRINT_MATCH_THRESHOLD: f32 = 0.5;

/// Concatenate the PCM samples belonging to one speaker's diarization turns,
/// so the result can be fed to the embedding extractor as that speaker's signal.
/// `sample_rate` maps turn milliseconds to sample indices; turns running past
/// the buffer are clamped (never panics).
pub fn cluster_pcm_for(
    turns: &[SpeakerTurn],
    pcm: &[f32],
    speaker_id: u32,
    sample_rate: u32,
) -> Vec<f32> {
    let to_idx = |ms: i64| ((ms.max(0) * sample_rate as i64) / 1000) as usize;
    let mut out = Vec::new();
    for t in turns.iter().filter(|t| t.speaker_id == speaker_id) {
        let lo = to_idx(t.start_ms).min(pcm.len());
        let hi = to_idx(t.end_ms).min(pcm.len()).max(lo);
        out.extend_from_slice(&pcm[lo..hi]);
    }
    out
}

/// Cosine similarity of two vectors; 0.0 if lengths differ or either is zero.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Name of the enrolled voiceprint most similar to `emb`, if any scores at or
/// above `threshold`. `library` is `(name, embedding)`.
// ponytail: single best (argmax); add a best-vs-second margin if false matches
// show up in the field.
pub fn best_voiceprint_match(
    emb: &[f32],
    library: &[(String, Vec<f32>)],
    threshold: f32,
) -> Option<String> {
    library
        .iter()
        .map(|(name, v)| (name, cosine(emb, v)))
        .filter(|(_, s)| *s >= threshold)
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(name, _)| name.clone())
}

/// Default cosine-distance clustering threshold for auto speaker-count mode.
/// Higher merges more (fewer speakers). Tuned on real meeting audio: sherpa's
/// own default (0.5) over-clusters badly (a 6-person meeting → 13+ clusters);
/// ~0.75 tracks the real count for a mixed/unknown headcount.
// ponytail: 0.75 from a one-meeting sweep — revisit per-corpus if counts drift;
// a known headcount via `num_speakers` sidesteps it entirely.
pub const DEFAULT_CLUSTER_THRESHOLD: f32 = 0.75;

/// sherpa FastClustering cluster count from an optional known headcount: a known
/// count pins it; unknown → `-1` (auto — the threshold decides). This avoids
/// sherpa-rs's blind default of a fixed 4 speakers when we pass `None`.
fn sherpa_num_clusters(num_speakers: Option<u32>) -> i32 {
    num_speakers.map(|n| n as i32).unwrap_or(-1)
}

/// Configuration for the diarizer: paths to the sherpa-onnx models and
/// clustering controls.
#[derive(Debug, Clone)]
pub struct DiarizeConfig {
    /// Segmentation model (e.g. pyannote segmentation ONNX).
    pub segmentation_model: PathBuf,
    /// Speaker-embedding model (e.g. 3D-Speaker / wespeaker ONNX).
    pub embedding_model: PathBuf,
    /// If `Some(n)`, cluster into exactly `n` speakers; if `None`, auto-estimate
    /// the count using `cluster_threshold`.
    pub num_speakers: Option<u32>,
    /// Cosine-distance merge threshold used only in auto mode (`num_speakers`
    /// = None). See [`DEFAULT_CLUSTER_THRESHOLD`].
    pub cluster_threshold: f32,
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

        // sherpa-rs 0.6.8 DiarizeConfig. All fields set explicitly — its Default
        // blindly forces `num_clusters: Some(4)` + `threshold: 0.5`, which pins
        // every meeting to 4 speakers (fixed count ignores the threshold). We
        // instead pass -1 for auto mode so `threshold` actually drives the count,
        // and only pin a count when the headcount is known. min_duration on/off
        // = 0.3/0.5 (pyannote-sane; sherpa's 0.0/0.0 over-segments).
        let config = SherpaConfig {
            num_clusters: Some(sherpa_num_clusters(self.config.num_speakers)),
            threshold: Some(self.config.cluster_threshold),
            min_duration_on: Some(0.3),
            min_duration_off: Some(0.5),
            provider: None,
            debug: false,
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

        // Second arg is an optional progress callback (Option<ProgressCallback>);
        // we don't report per-chunk diarization progress.
        let segments = engine
            .compute(pcm_16k_mono.to_vec(), None)
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
    fn apply_turns_labels_every_segment_when_diarization_ran() {
        // seg0/seg1 overlap turns; seg2 has NO overlapping turn. When diarization
        // ran (turns non-empty) every segment must get a "Speaker N" so the
        // transcript never mixes diarization labels with Gemini's "講者 N".
        let mut segs = vec![
            tseg(0, 1000, Some("講者 1")),
            tseg(1000, 2000, Some("講者 1")),
            tseg(5000, 6000, Some("講者 2")),
        ];
        let turns = vec![turn(0, 1000, 0), turn(1000, 2000, 1)];

        apply_turns_to_segments(&mut segs, &turns);

        assert_eq!(segs[0].speaker.as_deref(), Some("Speaker 1")); // overlap
        assert_eq!(segs[1].speaker.as_deref(), Some("Speaker 2")); // overlap
        assert_eq!(segs[2].speaker.as_deref(), Some("Speaker 2")); // nearest turn, no mix
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

    #[test]
    fn sherpa_num_clusters_maps_unknown_to_auto() {
        // The bug this fixes: sherpa-rs defaults a None cluster count to a fixed
        // 4 speakers. We must instead pass -1 (auto: the threshold decides), and
        // only pin a fixed count when the headcount is actually known.
        assert_eq!(sherpa_num_clusters(None), -1);
        assert_eq!(sherpa_num_clusters(Some(6)), 6);
    }

    #[test]
    fn cluster_pcm_gathers_only_that_speakers_turns() {
        // 1 sample per ms (sample_rate 1000) so sample index == ms. pcm[i] == i.
        let pcm: Vec<f32> = (0..2000).map(|i| i as f32).collect();
        let turns = vec![turn(0, 500, 0), turn(1000, 1500, 1), turn(1500, 2000, 0)];

        // Speaker 0 owns [0,500) and [1500,2000) → 1000 samples, concatenated.
        let got = cluster_pcm_for(&turns, &pcm, 0, 1000);
        assert_eq!(got.len(), 1000);
        assert_eq!(got[0], 0.0); // first sample of first turn
        assert_eq!(got[500], 1500.0); // first sample of second turn
    }

    #[test]
    fn cluster_pcm_clamps_out_of_range_turns() {
        let pcm: Vec<f32> = (0..100).map(|i| i as f32).collect();
        // Turn runs past the buffer end; must clamp, not panic.
        let turns = vec![turn(0, 999_000, 0)];
        let got = cluster_pcm_for(&turns, &pcm, 0, 1000);
        assert_eq!(got.len(), 100);
    }

    #[test]
    fn voiceprint_match_returns_name_on_exact_hit() {
        let lib = vec![
            ("Alice".to_string(), vec![1.0, 0.0, 0.0]),
            ("Bob".to_string(), vec![0.0, 1.0, 0.0]),
        ];
        // Identical to Alice's vector → cosine 1.0.
        assert_eq!(
            best_voiceprint_match(&[1.0, 0.0, 0.0], &lib, VOICEPRINT_MATCH_THRESHOLD),
            Some("Alice".to_string())
        );
    }

    #[test]
    fn voiceprint_match_picks_highest_cosine() {
        let lib = vec![
            ("Alice".to_string(), vec![1.0, 0.0]),
            ("Bob".to_string(), vec![0.7, 0.7]),
        ];
        // [0.9,0.1] is closer in angle to Alice than Bob.
        assert_eq!(
            best_voiceprint_match(&[0.9, 0.1], &lib, 0.5),
            Some("Alice".to_string())
        );
    }

    #[test]
    fn voiceprint_match_below_threshold_is_none() {
        let lib = vec![("Alice".to_string(), vec![1.0, 0.0])];
        // Orthogonal → cosine 0.0 < 0.5.
        assert_eq!(best_voiceprint_match(&[0.0, 1.0], &lib, 0.5), None);
    }

    #[test]
    fn voiceprint_match_empty_library_is_none() {
        assert_eq!(best_voiceprint_match(&[1.0, 0.0], &[], 0.5), None);
    }

    #[cfg(not(feature = "diarize"))]
    #[test]
    fn diarize_stub_returns_no_turns() {
        let d = Diarizer::new(DiarizeConfig {
            segmentation_model: "/seg.onnx".into(),
            embedding_model: "/emb.onnx".into(),
            num_speakers: None,
            cluster_threshold: DEFAULT_CLUSTER_THRESHOLD,
        });
        assert!(d.diarize(&[0.0f32; 16]).unwrap().is_empty());
    }
}
