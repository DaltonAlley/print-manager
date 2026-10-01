use crate::ui::jobs::{
    card_state_class, file_extension, readable_event_time, readable_file_size,
    workflow_outcome_notice,
};

#[test]
fn card_state_classes_are_explicit_and_safe() {
    assert_eq!(card_state_class("design"), "state-design");
    assert_eq!(card_state_class("waiting"), "state-waiting");
    assert_eq!(card_state_class("print"), "state-print");
    assert_eq!(card_state_class("finish"), "state-finish");
    assert_eq!(card_state_class("complete"), "state-complete");
    assert_eq!(card_state_class("<script>"), "state-unknown");
}

#[test]
fn production_file_metadata_is_compact_and_safe_for_display() {
    assert_eq!(readable_file_size(0), "0 B");
    assert_eq!(readable_file_size(1536), "1.5 KB");
    assert_eq!(readable_file_size(2 * 1024 * 1024), "2.0 MB");
    assert_eq!(file_extension("press-ready.PDF"), "PDF");
    assert_eq!(file_extension("README"), "FILE");
    assert_eq!(file_extension("artwork.abcdef"), "ABCD");
}
#[test]
fn workflow_outcome_copy_only_accepts_controlled_values() {
    assert_eq!(
        workflow_outcome_notice(Some("print"), false, false, false),
        Some("Job updated.")
    );
    assert_eq!(
        workflow_outcome_notice(Some("PRINT"), false, false, false),
        None
    );
    assert_eq!(
        workflow_outcome_notice(Some("<script>"), false, false, false),
        None
    );
    assert_eq!(
        workflow_outcome_notice(Some("unknown"), true, false, false),
        Some("Pickup confirmed.")
    );
    assert_eq!(
        workflow_outcome_notice(None, false, true, false),
        Some("Card order saved.")
    );
    assert_eq!(
        workflow_outcome_notice(None, false, false, true),
        Some("Quote started as a scheduled job.")
    );
}

#[test]
fn event_times_are_readable_and_deterministic_across_ssr_and_hydration() {
    assert_eq!(
        readable_event_time("2026-08-21T00:05:06Z"),
        "Aug 21, 2026 at 12:05 AM UTC"
    );
    assert_eq!(
        readable_event_time("2026-08-21T14:35:06.123-04:00"),
        "Aug 21, 2026 at 2:35 PM UTC-04:00"
    );
    assert_eq!(readable_event_time("legacy timestamp"), "legacy timestamp");
}
