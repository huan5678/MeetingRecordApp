//! Window detection module.
//!
//! Monitors the active foreground window and matches its title against a
//! configurable list of known meeting-app patterns (Microsoft Teams / Google
//! Meet / Zoom) so the app can prompt the user to start recording (PRD §4.7,
//! user story #7).
//!
//! Split:
//! - This file (`mod.rs`) holds the **pure** matching logic: a window title
//!   (plus optional process name) in, a [`MeetingApp`] out. It has no platform
//!   dependencies and is fully unit-tested.
//! - [`monitor`] holds the **platform** layer: on Windows it polls
//!   `GetForegroundWindow` + `GetWindowTextW` (`#[cfg(target_os = "windows")]`);
//!   on every other OS it is a no-op stub returning `None` /
//!   `crate::unsupported_platform(...)` for v1.0 (Windows-only).

pub mod monitor;

pub use monitor::{ActiveWindow, WindowMonitor};

use serde::{Deserialize, Serialize};

/// A meeting application we know how to recognise from its window title.
///
/// Serializes to a stable lowercase string so it can cross the Tauri IPC
/// boundary and be persisted if needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingApp {
    /// Microsoft Teams desktop or web client.
    Teams,
    /// Google Meet (typically inside a Chrome/Edge tab).
    GoogleMeet,
    /// Zoom meeting window (the in-call window, not the idle home screen).
    Zoom,
}

impl MeetingApp {
    /// A short, human-readable label for UI ("Microsoft Teams").
    pub fn display_name(self) -> &'static str {
        match self {
            MeetingApp::Teams => "Microsoft Teams",
            MeetingApp::GoogleMeet => "Google Meet",
            MeetingApp::Zoom => "Zoom",
        }
    }

    /// The stable identifier used on the IPC boundary / in storage.
    pub fn as_str(self) -> &'static str {
        match self {
            MeetingApp::Teams => "teams",
            MeetingApp::GoogleMeet => "google_meet",
            MeetingApp::Zoom => "zoom",
        }
    }
}

/// A single rule mapping window text to a [`MeetingApp`].
///
/// Matching is case-insensitive substring matching on the window title. A rule
/// may also require a substring of the owning process name (e.g. only treat a
/// "Google Meet" title as Meet when it lives in a browser process) — this keeps
/// false positives down without hard-coding a single browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingPattern {
    /// Which app this rule identifies.
    pub app: MeetingApp,
    /// Case-insensitive substring that must appear in the window **title**.
    pub title_contains: String,
    /// Optional case-insensitive substring that must appear in the **process
    /// name** for the rule to fire. `None` means "any process".
    #[serde(default)]
    pub process_contains: Option<String>,
}

impl MeetingPattern {
    /// Convenience constructor for a title-only rule.
    pub fn title(app: MeetingApp, title_contains: impl Into<String>) -> Self {
        MeetingPattern {
            app,
            title_contains: title_contains.into(),
            process_contains: None,
        }
    }

    /// Convenience constructor for a rule that also constrains the process name.
    pub fn title_and_process(
        app: MeetingApp,
        title_contains: impl Into<String>,
        process_contains: impl Into<String>,
    ) -> Self {
        MeetingPattern {
            app,
            title_contains: title_contains.into(),
            process_contains: Some(process_contains.into()),
        }
    }

    /// Does this rule match the given window title / process name?
    ///
    /// Empty `title_contains` never matches (guards against an empty pattern
    /// accidentally matching every window).
    pub fn matches(&self, title: &str, process: Option<&str>) -> bool {
        if self.title_contains.is_empty() {
            return false;
        }
        if !contains_ci(title, &self.title_contains) {
            return false;
        }
        match &self.process_contains {
            None => true,
            Some(needle) => match process {
                Some(p) => contains_ci(p, needle),
                // Rule demands a process constraint but we don't know the
                // process → can't confirm, so don't match.
                None => false,
            },
        }
    }
}

/// The default, built-in set of meeting-app patterns (PRD §4.7).
///
/// Order matters: the first matching rule wins, so more specific browser-bound
/// rules (Google Meet) come before broad ones. Callers may extend or replace
/// this list (it is user-configurable per the PRD's "extensible pattern list").
pub fn default_patterns() -> Vec<MeetingPattern> {
    vec![
        // Microsoft Teams: the desktop client window title contains
        // "Microsoft Teams"; the newer client is "Microsoft Teams (work or
        // school)" — both contain the substring.
        MeetingPattern::title(MeetingApp::Teams, "Microsoft Teams"),
        // Zoom in-call window. The idle home screen is titled just "Zoom"; the
        // active call window is "Zoom Meeting", so we require the longer form
        // to avoid prompting before a call actually starts.
        MeetingPattern::title(MeetingApp::Zoom, "Zoom Meeting"),
        // Google Meet runs in a browser tab; Chrome/Edge put the tab title in
        // the window title ("Meet - <name>" / "Google Meet"). Constrain to a
        // browser-ish process so an email *about* a Meet link doesn't trigger.
        // Two rules cover Chrome and Edge process names.
        MeetingPattern::title_and_process(MeetingApp::GoogleMeet, "Meet", "chrome"),
        MeetingPattern::title_and_process(MeetingApp::GoogleMeet, "Meet", "msedge"),
    ]
}

/// Identify which meeting app (if any) a window belongs to.
///
/// Pure function: tries each pattern in order and returns the first match.
/// `process` is optional because not every platform layer can cheaply resolve
/// the owning process name; rules that require a process simply won't fire when
/// it's `None`.
pub fn match_meeting_app(
    patterns: &[MeetingPattern],
    title: &str,
    process: Option<&str>,
) -> Option<MeetingApp> {
    patterns
        .iter()
        .find(|p| p.matches(title, process))
        .map(|p| p.app)
}

/// Case-insensitive substring test. ASCII-fast-path friendly and Unicode-safe:
/// uses `to_lowercase()` so non-ASCII titles (e.g. localized "會議") still work.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meeting_app_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&MeetingApp::GoogleMeet).unwrap(),
            "\"google_meet\""
        );
        assert_eq!(MeetingApp::Teams.as_str(), "teams");
        assert_eq!(MeetingApp::Zoom.display_name(), "Zoom");
    }

    #[test]
    fn teams_desktop_title_matches() {
        let pats = default_patterns();
        assert_eq!(
            match_meeting_app(&pats, "Microsoft Teams", None),
            Some(MeetingApp::Teams)
        );
        // Newer client variant.
        assert_eq!(
            match_meeting_app(&pats, "Chat | Microsoft Teams (work or school)", None),
            Some(MeetingApp::Teams)
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        let pats = default_patterns();
        assert_eq!(
            match_meeting_app(&pats, "MICROSOFT TEAMS", None),
            Some(MeetingApp::Teams)
        );
        assert_eq!(
            match_meeting_app(&pats, "zoom meeting", None),
            Some(MeetingApp::Zoom)
        );
    }

    #[test]
    fn zoom_idle_home_screen_does_not_match() {
        let pats = default_patterns();
        // Bare "Zoom" is the idle window — must NOT trigger a recording prompt.
        assert_eq!(match_meeting_app(&pats, "Zoom", None), None);
    }

    #[test]
    fn zoom_meeting_window_matches() {
        let pats = default_patterns();
        assert_eq!(
            match_meeting_app(&pats, "Zoom Meeting", None),
            Some(MeetingApp::Zoom)
        );
    }

    #[test]
    fn google_meet_requires_browser_process() {
        let pats = default_patterns();
        // Title alone (no process) won't fire the process-constrained rule.
        assert_eq!(match_meeting_app(&pats, "Meet - Daily Standup", None), None);
        // In Chrome it matches.
        assert_eq!(
            match_meeting_app(&pats, "Meet - Daily Standup", Some("chrome.exe")),
            Some(MeetingApp::GoogleMeet)
        );
        // In Edge it matches.
        assert_eq!(
            match_meeting_app(&pats, "Google Meet", Some("msedge.exe")),
            Some(MeetingApp::GoogleMeet)
        );
        // In some random app it does not.
        assert_eq!(
            match_meeting_app(&pats, "Notes about Meet", Some("notepad.exe")),
            None
        );
    }

    #[test]
    fn unknown_window_returns_none() {
        let pats = default_patterns();
        assert_eq!(match_meeting_app(&pats, "Untitled - Notepad", None), None);
        assert_eq!(match_meeting_app(&pats, "", None), None);
    }

    #[test]
    fn empty_pattern_never_matches_everything() {
        let pats = vec![MeetingPattern::title(MeetingApp::Teams, "")];
        assert_eq!(match_meeting_app(&pats, "anything at all", None), None);
    }

    #[test]
    fn first_matching_pattern_wins() {
        // Two rules that both match; the first in the list should be chosen.
        let pats = vec![
            MeetingPattern::title(MeetingApp::Zoom, "Meeting"),
            MeetingPattern::title(MeetingApp::Teams, "Meeting"),
        ];
        assert_eq!(
            match_meeting_app(&pats, "Weekly Meeting", None),
            Some(MeetingApp::Zoom)
        );
    }

    #[test]
    fn process_constraint_ignored_when_rule_has_none() {
        // A title-only rule matches regardless of the process value.
        let pats = vec![MeetingPattern::title(MeetingApp::Teams, "Microsoft Teams")];
        assert_eq!(
            match_meeting_app(&pats, "Microsoft Teams", Some("anything.exe")),
            Some(MeetingApp::Teams)
        );
    }

    #[test]
    fn pattern_roundtrips_through_serde() {
        let p = MeetingPattern::title_and_process(MeetingApp::GoogleMeet, "Meet", "chrome");
        let json = serde_json::to_string(&p).unwrap();
        let back: MeetingPattern = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn pattern_deserializes_without_process_field() {
        // `process_contains` is `#[serde(default)]` → optional in JSON.
        let p: MeetingPattern =
            serde_json::from_str(r#"{"app":"teams","title_contains":"Microsoft Teams"}"#).unwrap();
        assert_eq!(p.app, MeetingApp::Teams);
        assert_eq!(p.process_contains, None);
    }

    #[test]
    fn contains_ci_handles_unicode() {
        assert!(contains_ci("線上會議室", "會議"));
        assert!(!contains_ci("線上會議室", "zoom"));
    }
}
