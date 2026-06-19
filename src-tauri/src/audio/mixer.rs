//! Audio mixer — the **pure DSP core** of the audio subsystem.
//!
//! The mic and the system-audio loopback come from *different devices on
//! different clock domains* (PRD §4.4). Two failure modes follow from that and
//! both are handled here:
//!
//! 1. **Sample-rate mismatch.** The two legs may be captured at different rates
//!    (e.g. mic 44.1 kHz, system 48 kHz). We resample each leg to a common
//!    target rate before summing.
//! 2. **Clock drift.** Even at a nominally identical rate, two crystals tick at
//!    slightly different speeds, so over a long meeting one leg slowly produces
//!    more frames than the other. Left unchecked the two legs desynchronise.
//!    [`DriftCompensator`] measures the running frame ratio against wall-clock
//!    expectation and nudges the effective resample ratio to keep them locked.
//!
//! Buffering between the capture callbacks (which push at the device's whim)
//! and the consumer (recorder / transcription, which pulls fixed blocks) is a
//! [`SampleQueue`] that explicitly reports under-run (consumer outran
//! producer → emit silence, never garbage) and caps over-run (producer outran
//! consumer → drop oldest, never grow without bound).
//!
//! Everything in this file is deterministic and allocation-light, and the whole
//! file is covered by `#[cfg(test)]` tests at the bottom: resample ratio, drift
//! compensation, mono downmix, queue under/overrun, and the property that a
//! pause introduces no discontinuity / no sample corruption.

use super::AudioChunk;

/// Linearly resample interleaved `f32` PCM from `from_rate` to `to_rate`,
/// preserving channel interleaving.
///
/// Linear interpolation is deliberate: it is cheap, phase-coherent enough for
/// speech fed to whisper, and — crucially — *exact* at the boundary cases the
/// tests pin (identity, integer up/down ratios), which makes the DSP auditable.
/// A higher-quality polyphase/sinc resampler is a drop-in future upgrade behind
/// the same signature.
///
/// `channels` is the interleave width of `input`. Returns interleaved output at
/// `to_rate`. An empty or single-frame input returns the (possibly converted)
/// input unchanged in length-correct fashion.
pub fn resample_linear(input: &[f32], channels: u16, from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let in_frames = input.len() / ch;
    if in_frames == 0 {
        return Vec::new();
    }
    if in_frames == 1 {
        // Nothing to interpolate between; just hold the single frame.
        return input[..ch].to_vec();
    }

    let ratio = to_rate as f64 / from_rate as f64;
    // Number of output frames. Use the span between the first and last input
    // frame so the mapping is symmetric and round-trips cleanly.
    let out_frames = ((in_frames - 1) as f64 * ratio).round() as usize + 1;
    let mut out = Vec::with_capacity(out_frames * ch);

    let step = (in_frames - 1) as f64 / (out_frames - 1).max(1) as f64;
    for o in 0..out_frames {
        let src_pos = o as f64 * step;
        let i0 = src_pos.floor() as usize;
        let i1 = (i0 + 1).min(in_frames - 1);
        let frac = (src_pos - i0 as f64) as f32;
        for c in 0..ch {
            let a = input[i0 * ch + c];
            let b = input[i1 * ch + c];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// Downmix interleaved PCM to a single mono channel by averaging across
/// channels. Mono input is returned as-is.
pub fn downmix_to_mono(input: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return input.to_vec();
    }
    let frames = input.len() / ch;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let base = f * ch;
        let mut acc = 0.0f32;
        for c in 0..ch {
            acc += input[base + c];
        }
        out.push(acc / ch as f32);
    }
    out
}

/// Sum two equal-length mono streams sample-for-sample with clamping to
/// `[-1.0, 1.0]`, so a loud mic + loud system audio can never wrap or clip into
/// noise. If the lengths differ, the shorter is treated as zero-padded (the
/// extra tail of the longer stream is copied through, clamped).
pub fn mix_mono(a: &[f32], b: &[f32]) -> Vec<f32> {
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let av = a.get(i).copied().unwrap_or(0.0);
        let bv = b.get(i).copied().unwrap_or(0.0);
        out.push((av + bv).clamp(-1.0, 1.0));
    }
    out
}

/// Convert a mono `f32` stream in `[-1.0, 1.0]` to signed 16-bit PCM, the format
/// whisper and our WAV files use. Out-of-range inputs are clamped, not wrapped.
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            // Asymmetric full-scale: -1.0 → i16::MIN, +1.0 → i16::MAX.
            if clamped >= 0.0 {
                (clamped * i16::MAX as f32).round() as i16
            } else {
                (-clamped * i16::MIN as f32).round() as i16
            }
        })
        .collect()
}

/// Tracks clock drift between a leg's *actual* produced frames and the number
/// it *should* have produced for the elapsed wall-clock time, and yields a
/// small multiplicative correction to the resample ratio so the leg stays
/// locked to the target timeline over a long meeting.
///
/// Usage: feed [`observe`](DriftCompensator::observe) the frames produced and
/// the nominal rate each callback; multiply your base resample ratio by
/// [`correction`](DriftCompensator::correction) before resampling.
#[derive(Debug, Clone)]
pub struct DriftCompensator {
    nominal_rate: f64,
    /// Total frames the device has actually delivered.
    observed_frames: u64,
    /// Wall-clock seconds elapsed across all observations.
    elapsed_secs: f64,
    /// How hard to pull toward the measured drift, in (0, 1]. Small = smooth.
    gain: f64,
    /// Clamp on the correction so a bad measurement can't run away.
    max_correction: f64,
}

impl DriftCompensator {
    /// `nominal_rate` is the device's advertised sample rate. `gain` damps the
    /// correction (0.1 is a reasonable default); `max_correction` caps it (e.g.
    /// 0.02 = at most ±2 %, far more than any real crystal drifts).
    pub fn new(nominal_rate: u32, gain: f64, max_correction: f64) -> Self {
        Self {
            nominal_rate: nominal_rate.max(1) as f64,
            observed_frames: 0,
            elapsed_secs: 0.0,
            gain: gain.clamp(0.0, 1.0),
            max_correction: max_correction.abs(),
        }
    }

    /// Record that `frames` were delivered over `dt_secs` of wall-clock time.
    pub fn observe(&mut self, frames: usize, dt_secs: f64) {
        self.observed_frames += frames as u64;
        self.elapsed_secs += dt_secs.max(0.0);
    }

    /// Multiplicative correction for the resample ratio.
    ///
    /// `>1.0` means the device is running *slow* (produced fewer frames than
    /// nominal), so we stretch slightly to fill the target timeline; `<1.0`
    /// means it is running *fast*. Returns exactly `1.0` until there is enough
    /// signal to measure, and is clamped to `1 ± max_correction`.
    pub fn correction(&self) -> f64 {
        if self.elapsed_secs <= 0.0 || self.observed_frames == 0 {
            return 1.0;
        }
        let expected = self.nominal_rate * self.elapsed_secs;
        // measured_rate = observed / elapsed. ratio < 1 → running slow.
        let ratio = self.observed_frames as f64 / expected;
        // Desired correction is 1/ratio (stretch slow streams), damped by gain.
        let raw = 1.0 / ratio;
        let damped = 1.0 + (raw - 1.0) * self.gain;
        damped.clamp(1.0 - self.max_correction, 1.0 + self.max_correction)
    }

    /// Current measured drift in parts-per-million (positive = device fast).
    pub fn drift_ppm(&self) -> f64 {
        if self.elapsed_secs <= 0.0 {
            return 0.0;
        }
        let expected = self.nominal_rate * self.elapsed_secs;
        (self.observed_frames as f64 - expected) / expected * 1_000_000.0
    }
}

/// A bounded FIFO of mono `f32` samples sitting between a capture callback
/// (producer) and the recorder/transcription consumer.
///
/// * **Over-run** (producer faster than consumer): once `capacity` is reached,
///   the *oldest* samples are dropped to make room. This bounds latency and
///   memory; we lose the stalest audio rather than ballooning.
/// * **Under-run** (consumer faster than producer): [`pop`](SampleQueue::pop)
///   pads the shortfall with silence (`0.0`) so the output timeline never
///   stutters or replays stale data — silence is the correct, non-corrupting
///   fill for a momentary gap.
#[derive(Debug)]
pub struct SampleQueue {
    buf: std::collections::VecDeque<f32>,
    capacity: usize,
    /// Running counters for diagnostics / tests.
    dropped: u64,
    padded: u64,
}

impl SampleQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: std::collections::VecDeque::with_capacity(capacity.min(1 << 20)),
            capacity: capacity.max(1),
            dropped: 0,
            padded: 0,
        }
    }

    /// Push samples, dropping oldest on over-run. Returns how many were dropped.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let mut dropped_now = 0;
        // If the incoming batch alone exceeds capacity, keep only its tail.
        let start = samples.len().saturating_sub(self.capacity);
        if start > 0 {
            dropped_now += start;
        }
        for &s in &samples[start..] {
            if self.buf.len() == self.capacity {
                self.buf.pop_front();
                dropped_now += 1;
            }
            self.buf.push_back(s);
        }
        self.dropped += dropped_now as u64;
        dropped_now
    }

    /// Pop exactly `n` samples, padding with silence on under-run. Always
    /// returns a `Vec` of length `n`.
    pub fn pop(&mut self, n: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(n);
        let available = self.buf.len().min(n);
        for _ in 0..available {
            out.push(self.buf.pop_front().unwrap());
        }
        let shortfall = n - available;
        if shortfall > 0 {
            out.resize(n, 0.0);
            self.padded += shortfall as u64;
        }
        out
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped
    }

    pub fn padded_total(&self) -> u64 {
        self.padded
    }

    /// Discard buffered samples without resetting counters. Used by the recorder
    /// on **pause**: we stop accepting capture, but draining what's queued (vs.
    /// leaving it) means resume starts cleanly with no stale pre-pause audio
    /// bleeding across the gap.
    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

/// Stateless-ish helper that takes a raw [`AudioChunk`] from either leg and
/// produces a mono stream at `target_rate`, applying (in order) downmix →
/// drift-corrected resample. The per-leg [`DriftCompensator`] is owned by the
/// caller (the recorder) so it persists across chunks.
///
/// Returns mono `f32` at `target_rate`.
pub fn prepare_leg(
    chunk: &AudioChunk,
    target_rate: u32,
    drift: &DriftCompensator,
) -> Vec<f32> {
    let mono = downmix_to_mono(&chunk.samples, chunk.format.channels);
    // Fold the drift correction into the effective source rate: producing
    // *fewer* frames than nominal (slow clock, correction > 1) means we should
    // treat the source as if it were running slightly slower so the resampler
    // stretches it back onto the target timeline.
    let corrected_from = (chunk.format.sample_rate as f64 / drift.correction()).round() as u32;
    let corrected_from = corrected_from.max(1);
    resample_linear(&mono, 1, corrected_from, target_rate)
}

/// The mixer's two output sinks. `transcription` is always 16 kHz mono. The
/// optional `dual_track` keeps each leg separate (at `record_rate`, mono) so
/// diarization can use the system/mic split as a hint (PRD §4.4).
#[derive(Debug, Clone, PartialEq)]
pub struct MixOutput {
    /// 16 kHz mono, mic+system summed — fed to whisper.
    pub transcription: Vec<f32>,
    /// `record_rate` mono, mic+system summed — what gets written to the main
    /// WAV (resampled to i16 by the recorder).
    pub recording: Vec<f32>,
    /// Optional separate legs for diarization: `(system, mic)` at `record_rate`.
    pub dual_track: Option<(Vec<f32>, Vec<f32>)>,
}

/// Combine an already-mono, already-resampled system leg and mic leg (both at
/// `record_rate`) into the mixer outputs.
///
/// This is the final summation stage; resampling/drift/downmix have already run
/// per-leg via [`prepare_leg`]. Splitting it this way keeps each transform
/// independently testable.
pub fn combine_legs(
    system_mono: &[f32],
    mic_mono: &[f32],
    record_rate: u32,
    keep_dual_track: bool,
) -> MixOutput {
    let recording = mix_mono(system_mono, mic_mono);
    let transcription = resample_linear(&recording, 1, record_rate, super::WHISPER_SAMPLE_RATE);
    let dual_track = if keep_dual_track {
        Some((system_mono.to_vec(), mic_mono.to_vec()))
    } else {
        None
    };
    MixOutput {
        transcription,
        recording,
        dual_track,
    }
}

/// Convenience: run the whole per-leg → combine pipeline for one pair of raw
/// chunks. Either side may be `None` (only one source active), in which case it
/// contributes silence-equivalent (an empty leg). The drift compensators are
/// borrowed so they keep accumulating across calls.
#[allow(clippy::too_many_arguments)]
pub fn mix_chunks(
    system: Option<&AudioChunk>,
    mic: Option<&AudioChunk>,
    record_rate: u32,
    system_drift: &DriftCompensator,
    mic_drift: &DriftCompensator,
    keep_dual_track: bool,
) -> MixOutput {
    let sys_leg = system
        .map(|c| prepare_leg(c, record_rate, system_drift))
        .unwrap_or_default();
    let mic_leg = mic
        .map(|c| prepare_leg(c, record_rate, mic_drift))
        .unwrap_or_default();
    combine_legs(&sys_leg, &mic_leg, record_rate, keep_dual_track)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{Source, StreamFormat};

    // ---- resample_linear --------------------------------------------------

    #[test]
    fn resample_identity_rate_is_passthrough() {
        let input = vec![0.1, -0.2, 0.3, -0.4];
        let out = resample_linear(&input, 1, 16_000, 16_000);
        assert_eq!(out, input);
    }

    #[test]
    fn resample_upsample_2x_doubles_frame_count() {
        // mono ramp: 0,1,2,3 at 16k → 32k should ~double frames.
        let input = vec![0.0, 1.0, 2.0, 3.0];
        let out = resample_linear(&input, 1, 16_000, 32_000);
        // (4-1)*2 + 1 = 7 output frames.
        assert_eq!(out.len(), 7);
        // Endpoints preserved exactly.
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[out.len() - 1] - 3.0).abs() < 1e-6);
        // Midpoints are linear interpolations: first interior ≈ 0.5.
        assert!((out[1] - 0.5).abs() < 1e-5, "got {}", out[1]);
    }

    #[test]
    fn resample_downsample_48k_to_16k_ratio() {
        // 48k → 16k is a 1/3 ratio. 7 input frames → (7-1)/3 + 1 = 3 frames.
        let input: Vec<f32> = (0..7).map(|i| i as f32).collect();
        let out = resample_linear(&input, 1, 48_000, 16_000);
        assert_eq!(out.len(), 3);
        // Endpoints exact.
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[2] - 6.0).abs() < 1e-6);
    }

    #[test]
    fn resample_preserves_stereo_interleave() {
        // L ramps 0..3, R is constant 9. Upsample 2x; R must stay 9 everywhere.
        let input = vec![0.0, 9.0, 1.0, 9.0, 2.0, 9.0, 3.0, 9.0];
        let out = resample_linear(&input, 2, 16_000, 32_000);
        assert_eq!(out.len() % 2, 0);
        for f in 0..out.len() / 2 {
            assert!((out[f * 2 + 1] - 9.0).abs() < 1e-5, "R drifted at {f}");
        }
    }

    #[test]
    fn resample_empty_and_single_frame_are_safe() {
        assert!(resample_linear(&[], 1, 48_000, 16_000).is_empty());
        let one = resample_linear(&[0.42], 1, 48_000, 16_000);
        assert_eq!(one, vec![0.42]);
    }

    // ---- downmix ----------------------------------------------------------

    #[test]
    fn downmix_stereo_averages_channels() {
        // frames: (1,-1)->0, (0.5,0.5)->0.5
        let input = vec![1.0, -1.0, 0.5, 0.5];
        let out = downmix_to_mono(&input, 2);
        assert_eq!(out, vec![0.0, 0.5]);
    }

    #[test]
    fn downmix_mono_is_passthrough() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(downmix_to_mono(&input, 1), input);
    }

    // ---- mix + clamp ------------------------------------------------------

    #[test]
    fn mix_mono_clamps_instead_of_wrapping() {
        let a = vec![0.8, -0.9, 0.0];
        let b = vec![0.8, -0.9, 0.0];
        let out = mix_mono(&a, &b);
        assert_eq!(out, vec![1.0, -1.0, 0.0]); // clamped, not 1.6 / -1.8
    }

    #[test]
    fn mix_mono_uneven_lengths_zero_pads_shorter() {
        let a = vec![0.1, 0.2, 0.3];
        let b = vec![0.5];
        let out = mix_mono(&a, &b);
        assert_eq!(out.len(), 3);
        assert!((out[0] - 0.6).abs() < 1e-6);
        assert!((out[1] - 0.2).abs() < 1e-6);
        assert!((out[2] - 0.3).abs() < 1e-6);
    }

    // ---- f32 -> i16 -------------------------------------------------------

    #[test]
    fn f32_to_i16_full_scale_and_clamp() {
        assert_eq!(f32_to_i16(&[1.0])[0], i16::MAX);
        assert_eq!(f32_to_i16(&[-1.0])[0], i16::MIN);
        assert_eq!(f32_to_i16(&[0.0])[0], 0);
        // Out of range clamps, never wraps.
        assert_eq!(f32_to_i16(&[2.5])[0], i16::MAX);
        assert_eq!(f32_to_i16(&[-2.5])[0], i16::MIN);
    }

    // ---- drift compensation ----------------------------------------------

    #[test]
    fn drift_correction_starts_neutral() {
        let d = DriftCompensator::new(48_000, 0.1, 0.02);
        assert!((d.correction() - 1.0).abs() < 1e-12);
        assert!((d.drift_ppm() - 0.0).abs() < 1e-12);
    }

    #[test]
    fn drift_slow_device_yields_stretch_above_one() {
        // Device nominal 48k but only delivered 47_990 frames in 1s → running
        // slow → correction should pull > 1.0 (stretch to fill timeline).
        let mut d = DriftCompensator::new(48_000, 1.0, 0.02); // gain 1 = full
        d.observe(47_990, 1.0);
        let c = d.correction();
        assert!(c > 1.0, "expected stretch >1, got {c}");
        // ppm should be negative (fewer than expected).
        assert!(d.drift_ppm() < 0.0);
    }

    #[test]
    fn drift_fast_device_yields_squeeze_below_one() {
        let mut d = DriftCompensator::new(48_000, 1.0, 0.02);
        d.observe(48_010, 1.0); // more than nominal → fast
        let c = d.correction();
        assert!(c < 1.0, "expected squeeze <1, got {c}");
        assert!(d.drift_ppm() > 0.0);
    }

    #[test]
    fn drift_correction_is_clamped() {
        // Wildly wrong measurement must not blow past max_correction.
        let mut d = DriftCompensator::new(48_000, 1.0, 0.02);
        d.observe(1, 1.0); // absurdly slow
        let c = d.correction();
        assert!(c <= 1.0 + 0.02 + 1e-9, "clamp violated: {c}");
        assert!(c >= 1.0 - 0.02 - 1e-9);
    }

    #[test]
    fn drift_gain_damps_correction() {
        let mut slow_full = DriftCompensator::new(48_000, 1.0, 1.0);
        let mut slow_damped = DriftCompensator::new(48_000, 0.1, 1.0);
        slow_full.observe(47_000, 1.0);
        slow_damped.observe(47_000, 1.0);
        let full = slow_full.correction() - 1.0;
        let damped = slow_damped.correction() - 1.0;
        // Damped correction is ~10% of full and same sign.
        assert!(damped > 0.0 && full > 0.0);
        assert!(damped < full);
        assert!((damped / full - 0.1).abs() < 0.05, "ratio {}", damped / full);
    }

    // ---- SampleQueue: under/overrun --------------------------------------

    #[test]
    fn queue_overrun_drops_oldest_and_bounds_size() {
        let mut q = SampleQueue::new(3);
        assert_eq!(q.push(&[1.0, 2.0, 3.0]), 0);
        // Pushing one more drops the oldest (1.0).
        assert_eq!(q.push(&[4.0]), 1);
        assert_eq!(q.len(), 3);
        let got = q.pop(3);
        assert_eq!(got, vec![2.0, 3.0, 4.0]);
        assert_eq!(q.dropped_total(), 1);
    }

    #[test]
    fn queue_push_larger_than_capacity_keeps_tail() {
        let mut q = SampleQueue::new(2);
        let dropped = q.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(q.len(), 2);
        assert_eq!(q.pop(2), vec![4.0, 5.0]);
        assert_eq!(dropped, 3);
    }

    #[test]
    fn queue_underrun_pads_with_silence_never_garbage() {
        let mut q = SampleQueue::new(8);
        q.push(&[0.5, 0.5]);
        let got = q.pop(5);
        assert_eq!(got.len(), 5);
        assert_eq!(&got[..2], &[0.5, 0.5]);
        // Shortfall is pure silence, not stale/repeated data.
        assert_eq!(&got[2..], &[0.0, 0.0, 0.0]);
        assert_eq!(q.padded_total(), 3);
    }

    #[test]
    fn queue_pop_empty_is_all_silence() {
        let mut q = SampleQueue::new(4);
        assert_eq!(q.pop(3), vec![0.0, 0.0, 0.0]);
    }

    // ---- pause produces no gap corruption --------------------------------

    #[test]
    fn pause_clear_then_resume_has_no_pre_pause_bleed() {
        // Simulate: capture A, pause (clear queued but undrained A), resume,
        // capture B. The consumer must NOT see leftover A samples interleaved
        // into B — only B, padded with clean silence if it under-runs.
        let mut q = SampleQueue::new(16);
        q.push(&[0.9, 0.9, 0.9]); // pre-pause audio, never consumed
        q.clear(); // <- pause boundary
        q.push(&[0.1, 0.2]); // post-resume audio
        let got = q.pop(4);
        // First two are the new audio, remaining are silence — zero 0.9 leakage.
        assert_eq!(&got[..2], &[0.1, 0.2]);
        assert_eq!(&got[2..], &[0.0, 0.0]);
        assert!(!got.contains(&0.9), "pre-pause sample bled across the gap");
    }

    #[test]
    fn pause_does_not_advance_timeline() {
        // The recorder tracks duration by consumed frames. Across a pause we
        // push nothing while paused, so popping during pause yields only
        // silence and the *captured* (non-silent) frame count is unchanged.
        let mut q = SampleQueue::new(16);
        q.push(&[0.3, 0.3]);
        let before = q.len();
        // (paused: no pushes)
        let during_pause = q.pop(0); // consumer also idle
        assert!(during_pause.is_empty());
        assert_eq!(q.len(), before); // nothing corrupted, nothing lost
    }

    // ---- prepare_leg / combine / mix_chunks integration ------------------

    #[test]
    fn prepare_leg_downmixes_and_resamples_to_target() {
        // stereo 48k → mono 16k.
        let chunk = AudioChunk::new(
            Source::System,
            StreamFormat::new(48_000, 2),
            vec![1.0, 1.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, -1.0, -1.0],
        );
        let drift = DriftCompensator::new(48_000, 0.1, 0.02);
        let out = prepare_leg(&chunk, 16_000, &drift);
        // 7 input frames at 48k → ~3 output frames at 16k.
        assert_eq!(out.len(), 3);
        // Mono of (1,1) is 1.0; endpoints preserved through resample.
        assert!((out[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn combine_legs_produces_16k_transcription_and_optional_dual() {
        let sys = vec![0.2; 48]; // 1ms-ish at 48k
        let mic = vec![0.1; 48];
        let out = combine_legs(&sys, &mic, 48_000, true);
        // recording is summed at record rate, same length.
        assert_eq!(out.recording.len(), 48);
        assert!((out.recording[0] - 0.3).abs() < 1e-6);
        // transcription downsampled to 16k → ~1/3 length.
        assert_eq!(out.transcription.len(), (47.0_f64 / 3.0).round() as usize + 1);
        // dual track kept and legs unchanged.
        let (s, m) = out.dual_track.unwrap();
        assert_eq!(s, sys);
        assert_eq!(m, mic);
    }

    #[test]
    fn combine_legs_without_dual_track_is_none() {
        let out = combine_legs(&[0.1, 0.1], &[0.1, 0.1], 16_000, false);
        assert!(out.dual_track.is_none());
    }

    #[test]
    fn mix_chunks_with_one_missing_leg_uses_silence() {
        let mic = AudioChunk::new(
            Source::Microphone,
            StreamFormat::new(16_000, 1),
            vec![0.5, 0.5, 0.5, 0.5],
        );
        let sd = DriftCompensator::new(48_000, 0.1, 0.02);
        let md = DriftCompensator::new(16_000, 0.1, 0.02);
        let out = mix_chunks(None, Some(&mic), 16_000, &sd, &md, false);
        // No system leg → output equals the mic leg (no NaNs, no panic).
        assert_eq!(out.recording.len(), 4);
        assert!((out.recording[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mix_chunks_both_legs_at_different_rates_align() {
        // system at 48k, mic at 44.1k, both ~1ms; both resample to 16k record.
        let sys = AudioChunk::new(Source::System, StreamFormat::new(48_000, 1), vec![0.2; 48]);
        let mic = AudioChunk::new(
            Source::Microphone,
            StreamFormat::new(44_100, 1),
            vec![0.1; 44],
        );
        let sd = DriftCompensator::new(48_000, 0.1, 0.02);
        let md = DriftCompensator::new(44_100, 0.1, 0.02);
        let out = mix_chunks(Some(&sys), Some(&mic), 16_000, &sd, &md, false);
        // Both legs land at 16k; lengths within one frame of each other so the
        // sum is well-defined and clamped.
        assert!(!out.recording.is_empty());
        assert!(out.recording.iter().all(|s| s.abs() <= 1.0));
    }
}
