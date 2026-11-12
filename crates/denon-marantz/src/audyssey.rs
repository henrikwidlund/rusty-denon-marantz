//! Typed control of the Audyssey settings.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{ContainmentAmount, DynamicVolume, MultiEq, ReferenceLevelOffset, Zone};

use crate::avr::Avr;
use crate::error::Error;
use crate::status::{Blocker, Feature, SettingStatus};

/// Controls the Audyssey settings of a receiver. Get it from
/// [`Avr::audyssey`].
///
/// Dirac Live and Audyssey are two ways to calibrate the speakers and do not
/// work together. A receiver with a Dirac Live filter selected ignores the
/// Audyssey settings and sends no answer and no error. So each method but
/// [`Audyssey::set_multi_eq`] returns [`Error::AudysseyBlockedByDirac`] and
/// sends nothing while the state shows a Dirac filter. Selecting a MultEQ mode
/// switches back to Audyssey, so it is sent. To send an Audyssey
/// command anyway, use [`Avr::send`].
///
/// Each method returns when the command is sent, as [`Avr::send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::send`].
#[derive(Debug, Clone, Copy)]
pub struct Audyssey<'a> {
    avr: &'a Avr,
}

impl<'a> Audyssey<'a> {
    pub(crate) const fn new(avr: &'a Avr) -> Self {
        Self { avr }
    }

    /// Turns Dynamic EQ on (`true`) or off (`false`).
    ///
    /// # Errors
    ///
    /// See [`Audyssey`].
    pub async fn set_dynamic_eq(&self, on: bool) -> Result<(), Error> {
        self.check_no_dirac()?;
        self.send(Command::DynamicEq(on)).await
    }

    /// Sets the MultEQ mode.
    ///
    /// # Errors
    ///
    /// See [`Audyssey`].
    pub async fn set_multi_eq(&self, mode: MultiEq) -> Result<(), Error> {
        self.send(Command::SetMultiEq(mode)).await
    }

    /// Sets the reference level offset.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DynamicEqOff`], and sends nothing, if the state says
    /// that Dynamic EQ is off: the receiver accepts the offset only with
    /// Dynamic EQ on. See also [`Audyssey`].
    pub async fn set_reference_level_offset(
        &self,
        offset: ReferenceLevelOffset,
    ) -> Result<(), Error> {
        match self.avr.setting_status(Feature::ReferenceLevelOffset) {
            SettingStatus::Blocked(Blocker::DiracFilter) => {
                return Err(Error::AudysseyBlockedByDirac);
            }
            SettingStatus::Blocked(Blocker::DynamicEqOff) => return Err(Error::DynamicEqOff),
            _ => {}
        }
        self.send(Command::SetReferenceLevelOffset(offset)).await
    }

    /// Sets Dynamic Volume.
    ///
    /// # Errors
    ///
    /// See [`Audyssey`].
    pub async fn set_dynamic_volume(&self, volume: DynamicVolume) -> Result<(), Error> {
        self.check_no_dirac()?;
        self.send(Command::SetDynamicVolume(volume)).await
    }

    /// Turns LFC on (`true`) or off (`false`).
    ///
    /// # Errors
    ///
    /// See [`Audyssey`].
    pub async fn set_lfc(&self, on: bool) -> Result<(), Error> {
        self.check_no_dirac()?;
        self.send(Command::Lfc(on)).await
    }

    /// Sets the LFC containment amount.
    ///
    /// # Errors
    ///
    /// See [`Audyssey`].
    pub async fn set_containment_amount(&self, amount: ContainmentAmount) -> Result<(), Error> {
        self.check_no_dirac()?;
        self.send(Command::SetContainmentAmount(amount)).await
    }

    /// Sets the LFC containment amount from 1 to 7.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ContainmentOutOfRange`], and sends nothing, for a
    /// value that is not from 1 to 7. See also [`Audyssey`].
    pub async fn set_containment_level(&self, level: u8) -> Result<(), Error> {
        let amount = ContainmentAmount::new(level).ok_or(Error::ContainmentOutOfRange(level))?;
        self.set_containment_amount(amount).await
    }

    /// Raises the containment amount by one step. Does nothing at the highest
    /// amount.
    ///
    /// # Errors
    ///
    /// See [`Audyssey`].
    pub async fn containment_amount_up(&self) -> Result<(), Error> {
        self.check_no_dirac()?;
        if self.containment_amount() == Some(ContainmentAmount::MAX) {
            return Ok(());
        }
        self.send(Command::ContainmentAmountUp).await
    }

    /// Lowers the containment amount by one step. Does nothing at the lowest
    /// amount.
    ///
    /// # Errors
    ///
    /// See [`Audyssey`].
    pub async fn containment_amount_down(&self) -> Result<(), Error> {
        self.check_no_dirac()?;
        if self.containment_amount() == Some(ContainmentAmount::MIN) {
            return Ok(());
        }
        self.send(Command::ContainmentAmountDown).await
    }

    /// Fails if the state shows that a Dirac filter is selected.
    fn check_no_dirac(self) -> Result<(), Error> {
        match self.avr.setting_status(Feature::Audyssey) {
            SettingStatus::Blocked(Blocker::DiracFilter) => Err(Error::AudysseyBlockedByDirac),
            _ => Ok(()),
        }
    }

    fn containment_amount(self) -> Option<ContainmentAmount> {
        self.avr.state().borrow().device().containment_amount
    }

    async fn send(&self, command: Command<'_>) -> Result<(), Error> {
        self.avr.send(Zone::Main, command).await
    }
}
