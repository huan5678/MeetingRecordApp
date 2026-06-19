//! Summary templates (PRD §4.6): 1:1, team sync, client call, interview,
//! general.
//!
//! The [`SummaryTemplate`] enum and its prompt instructions live in
//! [`crate::ai::provider`] (they're coupled to `build_prompt`). This module is
//! the "template engine" surface from the architecture diagram: it re-exports
//! the type and provides the catalog + meeting-type mapping used by the
//! settings UI and the summarization command.

pub use crate::ai::provider::SummaryTemplate;

use crate::models::MeetingType;

/// Every template, in display order. Drives the template picker in the UI.
pub const ALL_TEMPLATES: [SummaryTemplate; 5] = [
    SummaryTemplate::General,
    SummaryTemplate::OneOnOne,
    SummaryTemplate::TeamSync,
    SummaryTemplate::ClientCall,
    SummaryTemplate::Interview,
];

/// Choose the default template for a meeting, given its (optional) type. Thin
/// wrapper over [`SummaryTemplate::for_meeting_type`] so callers can depend on
/// the `templates` module rather than reaching into `provider`.
pub fn default_template_for(meeting_type: Option<MeetingType>) -> SummaryTemplate {
    SummaryTemplate::for_meeting_type(meeting_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_all_five_templates() {
        assert_eq!(ALL_TEMPLATES.len(), 5);
        assert!(ALL_TEMPLATES.contains(&SummaryTemplate::OneOnOne));
        assert!(ALL_TEMPLATES.contains(&SummaryTemplate::TeamSync));
        assert!(ALL_TEMPLATES.contains(&SummaryTemplate::ClientCall));
        assert!(ALL_TEMPLATES.contains(&SummaryTemplate::Interview));
        assert!(ALL_TEMPLATES.contains(&SummaryTemplate::General));
    }

    #[test]
    fn every_template_has_distinct_label_and_instructions() {
        for (i, a) in ALL_TEMPLATES.iter().enumerate() {
            for b in &ALL_TEMPLATES[i + 1..] {
                assert_ne!(a.label(), b.label());
                assert_ne!(a.instructions(), b.instructions());
            }
        }
    }

    #[test]
    fn default_template_mapping() {
        assert_eq!(
            default_template_for(Some(MeetingType::Interview)),
            SummaryTemplate::Interview
        );
        assert_eq!(default_template_for(None), SummaryTemplate::General);
    }
}
