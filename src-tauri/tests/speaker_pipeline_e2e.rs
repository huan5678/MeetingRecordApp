//! End-to-end test of the speaker-identity pipeline (spec 2026-07-03), through
//! the public crate API and the on-disk sidecar format.
//!
//! This is the deterministic seam that runs anywhere: a stream of active-speaker
//! samples → `build_spans` → `speakers.srt` text → `parse_speaker_srt` →
//! `assign_speakers` onto a transcript. It does NOT exercise the Windows UI
//! Automation poller that *produces* the samples (that is device-level, gated on
//! the Phase 0 spike, and verified manually on Windows — see the spec).

use meeting_record_app_lib::detection::speaker::{
    assign_speakers, build_spans, parse_speaker_srt, to_speaker_srt, SpanConfig,
    DEFAULT_MIN_OVERLAP_FRAC,
};
use meeting_record_app_lib::models::TranscriptSegment;

/// Append `Some(name)` samples for `[from, to)` at `step` ms — simulating the
/// poller reading the same active speaker each tick.
fn fill(out: &mut Vec<(i64, Option<String>)>, from: i64, to: i64, name: &str, step: i64) {
    let mut t = from;
    while t < to {
        out.push((t, Some(name.to_string())));
        t += step;
    }
}

fn seg(start: i64, end: i64, speaker: Option<&str>) -> TranscriptSegment {
    TranscriptSegment {
        id: format!("s{start}"),
        meeting_id: "m1".into(),
        segment_index: start,
        start_time_ms: start,
        end_time_ms: end,
        text: "…".into(),
        speaker: speaker.map(|s| s.into()),
        confidence: None,
        language: None,
        created_at: "2026-07-03 00:00:00".into(),
    }
}

#[test]
fn identity_stream_labels_the_transcript_end_to_end() {
    let cfg = SpanConfig::default(); // poll 250, min_span 600, max_gap 750
    let step = cfg.poll_ms;

    // A meeting: Alice 0–7.75s, one Carol flicker at 5.0s, Bob 8.0–15.0s.
    let mut samples = Vec::new();
    fill(&mut samples, 0, 5_000, "Alice", step);
    samples.push((5_000, Some("Carol".into()))); // lone flicker sample
    fill(&mut samples, 5_250, 8_000, "Alice", step);
    fill(&mut samples, 8_000, 15_000, "Bob", step);

    // 1) Debounce into spans: the Carol flicker (< min_span) is dropped, Alice
    //    splits around it, Bob is one span.
    let spans = build_spans(&samples, &cfg);
    assert!(
        spans.iter().all(|s| s.name != "Carol"),
        "flicker must be dropped, got {spans:?}"
    );
    assert!(spans.iter().any(|s| s.name == "Alice"));
    assert!(spans.iter().any(|s| s.name == "Bob"));

    // 2) Persist to the sidecar and read it back — the artifact is faithful.
    let srt = to_speaker_srt(&spans);
    assert_eq!(parse_speaker_srt(&srt), spans, "speakers.srt must round-trip");

    // 3) A transcript (as if from Gemini/whisper) with only generic labels.
    let mut transcript = vec![
        seg(500, 4_500, None),                    // Alice
        seg(5_250, 7_800, None),                  // Alice (after the flicker)
        seg(8_200, 12_000, Some("Speaker 1")),    // Bob (overrides diarization)
        seg(12_500, 14_800, None),                // Bob
        seg(14_900, 16_000, Some("Speaker 2")),   // trails past Bob's span -> keep Speaker 2
    ];

    // 4) Overlap-join names onto the transcript using the parsed sidecar spans.
    let from_disk = parse_speaker_srt(&srt);
    assign_speakers(&mut transcript, &from_disk, DEFAULT_MIN_OVERLAP_FRAC);

    let labels: Vec<Option<&str>> = transcript.iter().map(|s| s.speaker.as_deref()).collect();
    assert_eq!(
        labels,
        vec![
            Some("Alice"),
            Some("Alice"),
            Some("Bob"),
            Some("Bob"),
            Some("Speaker 2"), // below overlap threshold -> diarization label preserved
        ]
    );
}
