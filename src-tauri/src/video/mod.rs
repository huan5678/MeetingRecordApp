//! Screen recording module.
//!
//! v1.1 — NOT in v1.0. v1.0 is audio-only (docs/PRD.md §6). This module is a
//! deliberate stub: FFmpeg sidecar wrapper + window/screen selection land in
//! v1.1. Do not implement here.
//!
// TODO(v1.1): FFmpeg-driven screen/window capture, A-V sync, quality config.

pub mod ffmpeg;
pub mod selector;
