//! Tells if a setting can be changed now, without sending anything.

use denon_marantz_proto::DiracFilter;

use crate::avr::Avr;

/// A group of settings that the receiver can refuse in some states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Feature {
    /// Turning tone control on, and bass and treble.
    ToneControl,
    /// The Audyssey settings, except the MultEQ mode.
    Audyssey,
    /// The reference level offset.
    ReferenceLevelOffset,
    /// Dirac Live.
    Dirac,
}

/// What stops a setting from being changed now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Blocker {
    /// A Dirac Live filter is selected.
    DiracFilter,
    /// Dynamic EQ is off.
    DynamicEqOff,
}

impl std::fmt::Display for Blocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DiracFilter => "a Dirac filter is selected",
            Self::DynamicEqOff => "Dynamic EQ is off",
        })
    }
}

/// If a setting can be changed now, as far as the state shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SettingStatus {
    /// The setting can be changed.
    Available,
    /// The receiver has the setting, but ignores it in its state now.
    Blocked(Blocker),
    /// The receiver does not have the setting.
    Unsupported,
    /// The state does not have the values that decide it yet. The setters
    /// send the command then.
    Unknown,
}

impl SettingStatus {
    /// Returns true if the setting is not blocked or unsupported.
    #[must_use]
    pub const fn can_send(self) -> bool {
        matches!(self, Self::Available | Self::Unknown)
    }
}

impl Avr {
    /// Tells if a group of settings can be changed now, with the same rules
    /// that the typed controls use before they send. Nothing is sent. The
    /// answer comes from the state, so it is [`SettingStatus::Unknown`] until
    /// the values that decide it are read.
    ///
    /// A setting that is not covered, for example Dynamic Volume without
    /// Audyssey, is not checked by the controls either.
    #[must_use]
    pub fn setting_status(&self, feature: Feature) -> SettingStatus {
        if feature == Feature::Dirac {
            return if self.dirac().is_ok() {
                SettingStatus::Available
            } else {
                SettingStatus::Unsupported
            };
        }
        let state = self.state();
        let state = state.borrow();
        let device = state.device();
        if device
            .dirac_filter
            .is_some_and(|filter| filter != DiracFilter::Off)
        {
            return SettingStatus::Blocked(Blocker::DiracFilter);
        }
        // A receiver without Dirac Live never reports a filter.
        let known = device.dirac_filter.is_some() || self.dirac().is_err();
        match feature {
            Feature::ReferenceLevelOffset if device.dynamic_eq == Some(false) => {
                SettingStatus::Blocked(Blocker::DynamicEqOff)
            }
            Feature::ReferenceLevelOffset if device.dynamic_eq.is_none() => SettingStatus::Unknown,
            _ if known => SettingStatus::Available,
            _ => SettingStatus::Unknown,
        }
    }
}
