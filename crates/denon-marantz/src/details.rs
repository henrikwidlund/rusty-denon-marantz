//! What happened to each page of a read from the web interface.

use denon_marantz_proto::{GeneralPage, SettingsGroup, VideoMenuPage};

/// Why a page was not asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SkipReason {
    /// A Dirac ART filter is active. The receiver does not answer the page
    /// while it is, so it is not asked.
    DiracArt,
    /// The source of the main zone does not report what plays.
    NoPlayingSource,
    /// The caller turned the page off.
    NotAsked,
    /// Another request already gave the values of the page, so it is not
    /// asked.
    AlreadyRead,
}

/// The result for one page of the web interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PageStatus {
    /// The page was read and its values are in the state.
    Read,
    /// The page was not asked for.
    Skipped(SkipReason),
    /// The receiver does not have the page (HTTP status 404).
    NotFound,
    /// The receiver refused the page with this HTTP status. A receiver with a
    /// Dirac ART filter answers the channel level page with 500.
    Refused(u16),
    /// The receiver did not answer in time. This is only used when it answered
    /// another page before, so the receiver is there but busy.
    NoAnswer,
    /// The receiver answered, but the answer is not a page that the library
    /// can read.
    Unreadable,
}

impl PageStatus {
    /// Returns true if the page was read.
    #[must_use]
    pub const fn is_read(self) -> bool {
        matches!(self, Self::Read)
    }

    /// Returns true if the state has the values of the page: it was read, or
    /// another request gave the values.
    #[must_use]
    pub const fn has_values(self) -> bool {
        matches!(self, Self::Read | Self::Skipped(SkipReason::AlreadyRead))
    }
}

/// The pages of [`Avr::update_speaker_details`](crate::Avr::update_speaker_details).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerDetails {
    /// Speaker distances and the number of subwoofers.
    pub distances: PageStatus,
    /// Speaker crossovers.
    pub crossovers: PageStatus,
    /// The channel levels, the dials of the web interface.
    pub channel_levels: PageStatus,
    /// Subwoofer output, low pass filter and LFE distribution.
    pub bass: PageStatus,
    /// How two-channel sources are played.
    pub playback_2ch: PageStatus,
    /// What the XLR outputs of the subwoofers carry.
    pub xlr_assign: PageStatus,
}

impl SpeakerDetails {
    /// Returns true if at least one page was read.
    #[must_use]
    pub const fn any_read(&self) -> bool {
        self.distances.is_read()
            || self.crossovers.is_read()
            || self.channel_levels.is_read()
            || self.bass.is_read()
            || self.playback_2ch.is_read()
            || self.xlr_assign.is_read()
    }
}

/// The pages of [`Avr::update_source_details`](crate::Avr::update_source_details).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceDetails {
    /// The physical input of each source.
    pub assignments: PageStatus,
    /// The default and automatic names of the sources.
    pub names: PageStatus,
    /// The levels of the analog and digital inputs.
    pub levels: PageStatus,
}

impl SourceDetails {
    /// Returns true if at least one page was read.
    #[must_use]
    pub const fn any_read(&self) -> bool {
        self.assignments.is_read() || self.names.is_read() || self.levels.is_read()
    }
}

/// What happened to each page of a read of several pages. `P` names a page,
/// for example a [`SettingsGroup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageResults<P> {
    /// The result for each page, in the order that they were read.
    pub pages: Vec<(P, PageStatus)>,
}

impl<P: PartialEq + Copy> PageResults<P> {
    /// The result for a page.
    #[must_use]
    pub fn status(&self, page: P) -> Option<PageStatus> {
        self.pages
            .iter()
            .find(|(known, _)| *known == page)
            .map(|(_, status)| *status)
    }

    /// Returns true if at least one page was read.
    #[must_use]
    pub fn any_read(&self) -> bool {
        self.pages.iter().any(|(_, status)| status.is_read())
    }
}

/// The pages of [`Avr::update_settings_availability`](crate::Avr::update_settings_availability).
pub type SettingsAvailabilityDetails = PageResults<SettingsGroup>;

/// The pages of [`Avr::update_video_details`](crate::Avr::update_video_details).
pub type VideoMenuDetails = PageResults<VideoMenuPage>;

/// The pages of [`Avr::update_general_settings`](crate::Avr::update_general_settings).
pub type GeneralDetails = PageResults<GeneralPage>;
