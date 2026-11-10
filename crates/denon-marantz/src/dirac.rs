//! Typed control of Dirac Live on telnet.

use denon_marantz_proto::DiracFilter;
use denon_marantz_proto::Zone;
use denon_marantz_proto::telnet::Command;

use crate::avr::Avr;
use crate::error::Error;

/// Controls Dirac Live of a receiver. Get it from [`Avr::dirac`].
///
/// Each method returns when the command is written, as [`Avr::telnet_send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::telnet_send`].
#[derive(Debug, Clone, Copy)]
pub struct Dirac<'a> {
    avr: &'a Avr,
}

impl<'a> Dirac<'a> {
    pub(crate) const fn new(avr: &'a Avr) -> Self {
        Self { avr }
    }

    /// Selects the filter of slot 1, 2 or 3.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DiracSlotOutOfRange`], and sends nothing, for another
    /// number. See also [`Dirac`].
    pub async fn select_slot(&self, number: u8) -> Result<(), Error> {
        let filter =
            DiracFilter::from_slot_number(number).ok_or(Error::DiracSlotOutOfRange(number))?;
        self.set_filter(filter).await
    }

    /// Selects the filter of the slot with this name. Upper and lower case are
    /// the same. The names come from the state, so they are known after a
    /// telnet connection has reported them. A slot that has no filter is not
    /// used.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownDiracSlot`], and sends nothing, if no slot has
    /// the name. See also [`Dirac`].
    pub async fn select_slot_by_name(&self, name: &str) -> Result<(), Error> {
        let filter = self.avr.state().borrow().device().dirac_filter_named(name);
        let filter = filter.ok_or_else(|| Error::UnknownDiracSlot(name.to_owned()))?;
        self.set_filter(filter).await
    }

    /// Selects a Dirac Live filter, or turns Dirac Live off.
    ///
    /// # Errors
    ///
    /// See [`Dirac`].
    pub async fn set_filter(&self, filter: DiracFilter) -> Result<(), Error> {
        self.avr
            .telnet_send(Zone::Main, Command::SetDiracFilter(filter))
            .await
    }
}
