//! Checks that more than one fuzz target uses.

use denon_marantz_proto::{State, Zone};

/// Asserts that the source of each zone is never empty and has no spaces at
/// the end.
pub fn check_sources(state: &State) {
    for zone in [Zone::Main, Zone::Zone2, Zone::Zone3] {
        let source = state.zone(zone).source.as_deref();
        assert!(
            source.is_none_or(|s| !s.is_empty() && s.trim_end() == s),
            "{source:?}"
        );
    }
}
