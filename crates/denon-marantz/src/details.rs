//! What happened to each page of a read from the web control pages.

/// Why a page was not asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SkipReason {
    /// A Dirac ART filter is active. The receiver does not answer the page
    /// while it is, so it is not asked.
    DiracArt,
}

/// The result for one page of the web control.
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
}

/// The pages of [`Avr::update_speaker_details`](crate::Avr::update_speaker_details).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerDetails {
    /// Speaker distances and the number of subwoofers.
    pub distances: PageStatus,
    /// Speaker crossovers.
    pub crossovers: PageStatus,
    /// The channel levels, the dials of the web control.
    pub channel_levels: PageStatus,
    /// Subwoofer output, low pass filter and LFE distribution.
    pub bass: PageStatus,
}

impl SpeakerDetails {
    /// Returns true if at least one page was read.
    #[must_use]
    pub const fn any_read(&self) -> bool {
        self.distances.is_read()
            || self.crossovers.is_read()
            || self.channel_levels.is_read()
            || self.bass.is_read()
    }
}

/// The pages of [`Avr::update_source_details`](crate::Avr::update_source_details).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceDetails {
    /// The physical input of each source.
    pub assignments: PageStatus,
    /// The default and automatic names of the sources.
    pub names: PageStatus,
}

impl SourceDetails {
    /// Returns true if at least one page was read.
    #[must_use]
    pub const fn any_read(&self) -> bool {
        self.assignments.is_read() || self.names.is_read()
    }
}
