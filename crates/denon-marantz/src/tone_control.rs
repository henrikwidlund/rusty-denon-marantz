//! Typed control of bass, treble and tone control on telnet.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{DiracFilter, ToneLevel, Zone};

use crate::avr::Avr;
use crate::error::{Error, ToneBlocker};

/// Controls bass, treble and tone control of a receiver. Get it from
/// [`Avr::tone`].
///
/// Bass and treble work only while tone control is on. A method that changes
/// them turns tone control on first if the state says that it is off. The
/// receiver ignores that with any Dirac Live filter selected. It sends no
/// answer and no error. So these methods return [`Error::ToneControlBlocked`]
/// and send nothing when the state shows a Dirac filter. Audyssey Dynamic EQ
/// might also block it, but that is not confirmed, so it is not checked. To
/// send the command anyway, use [`Avr::send`] with [`Command::ToneControl`].
///
/// Each method returns when the commands are written, as [`Avr::send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::send`].
#[derive(Debug, Clone, Copy)]
pub struct ToneControl<'a> {
    avr: &'a Avr,
}

impl<'a> ToneControl<'a> {
    pub(crate) const fn new(avr: &'a Avr) -> Self {
        Self { avr }
    }

    /// Turns tone control on (`true`) or off (`false`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::ToneControlBlocked`], and sends nothing, if `on` is
    /// `true` and the state says that a Dirac filter is selected. See also
    /// [`ToneControl`].
    pub async fn set_enabled(&self, on: bool) -> Result<(), Error> {
        if on {
            self.check_not_blocked()?;
        }
        self.send(Command::ToneControl(on)).await
    }

    /// Sets the bass level. The main zone takes levels from -6 to +6 dB.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ToneOutOfRange`], and sends nothing, for a level
    /// outside that range. See also [`ToneControl`].
    pub async fn set_bass(&self, level: ToneLevel) -> Result<(), Error> {
        check_main_range(level)?;
        self.change(Command::SetBass(level)).await
    }

    /// Sets the bass level in dB, from -6 to +6.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ToneOutOfRange`], and sends nothing, for a value that
    /// is not from -6 to +6 dB. See also [`ToneControl`].
    pub async fn set_bass_db(&self, db: i8) -> Result<(), Error> {
        self.set_bass(level_from_db(db)?).await
    }

    /// Raises the bass by one step. Does nothing at the highest level.
    ///
    /// # Errors
    ///
    /// See [`ToneControl`].
    pub async fn bass_up(&self) -> Result<(), Error> {
        if self.bass() == Some(ToneLevel::MAIN_MAX) {
            return Ok(());
        }
        self.change(Command::BassUp).await
    }

    /// Lowers the bass by one step. Does nothing at the lowest level.
    ///
    /// # Errors
    ///
    /// See [`ToneControl`].
    pub async fn bass_down(&self) -> Result<(), Error> {
        if self.bass() == Some(ToneLevel::MAIN_MIN) {
            return Ok(());
        }
        self.change(Command::BassDown).await
    }

    /// Sets the treble level. The main zone takes levels from -6 to +6 dB.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ToneOutOfRange`], and sends nothing, for a level
    /// outside that range. See also [`ToneControl`].
    pub async fn set_treble(&self, level: ToneLevel) -> Result<(), Error> {
        check_main_range(level)?;
        self.change(Command::SetTreble(level)).await
    }

    /// Sets the treble level in dB, from -6 to +6.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ToneOutOfRange`], and sends nothing, for a value that
    /// is not from -6 to +6 dB. See also [`ToneControl`].
    pub async fn set_treble_db(&self, db: i8) -> Result<(), Error> {
        self.set_treble(level_from_db(db)?).await
    }

    /// Raises the treble by one step. Does nothing at the highest level.
    ///
    /// # Errors
    ///
    /// See [`ToneControl`].
    pub async fn treble_up(&self) -> Result<(), Error> {
        if self.treble() == Some(ToneLevel::MAIN_MAX) {
            return Ok(());
        }
        self.change(Command::TrebleUp).await
    }

    /// Lowers the treble by one step. Does nothing at the lowest level.
    ///
    /// # Errors
    ///
    /// See [`ToneControl`].
    pub async fn treble_down(&self) -> Result<(), Error> {
        if self.treble() == Some(ToneLevel::MAIN_MIN) {
            return Ok(());
        }
        self.change(Command::TrebleDown).await
    }

    fn bass(self) -> Option<ToneLevel> {
        self.avr.state().borrow().zone(Zone::Main).bass
    }

    fn treble(self) -> Option<ToneLevel> {
        self.avr.state().borrow().zone(Zone::Main).treble
    }

    /// Sends a level command, after tone control is turned on if needed.
    async fn change(&self, command: Command<'_>) -> Result<(), Error> {
        let on = self.avr.state().borrow().device().tone_control;
        if on != Some(true) {
            self.set_enabled(true).await?;
        }
        self.send(command).await
    }

    /// Fails if the state shows what stops tone control from turning on.
    fn check_not_blocked(self) -> Result<(), Error> {
        let state = self.avr.state();
        let state = state.borrow();
        if state
            .device()
            .dirac_filter
            .is_some_and(|filter| filter != DiracFilter::Off)
        {
            Err(Error::ToneControlBlocked(ToneBlocker::DiracFilter))
        } else {
            Ok(())
        }
    }

    async fn send(&self, command: Command<'_>) -> Result<(), Error> {
        self.avr.send(Zone::Main, command).await
    }
}

fn level_from_db(db: i8) -> Result<ToneLevel, Error> {
    ToneLevel::from_db(db)
        .filter(|level| level.is_main_range())
        .ok_or(Error::ToneOutOfRange(db))
}

/// The main zone only takes levels from -6 to +6 dB.
fn check_main_range(level: ToneLevel) -> Result<(), Error> {
    if level.is_main_range() {
        Ok(())
    } else {
        Err(Error::ToneOutOfRange(level.db()))
    }
}
