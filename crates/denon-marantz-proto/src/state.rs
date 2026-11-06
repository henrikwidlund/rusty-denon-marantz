//! State of a receiver, updated from telnet events and `AppCommand` answers.
//!
//! A value is `None` until the receiver reports it, or if the receiver reports
//! something that is not a known value.
//!
//! Telnet events change one value at a time. An event with text that cannot be
//! read is ignored, so the last known value stays. An `AppCommand` answer is a
//! full snapshot. A value in it that cannot be read makes the value unknown.

use std::borrow::Cow;

use crate::appcommand::Field;
use crate::signal::SignalState;
use crate::sound_mode_list::SoundModeList;
use crate::source::telnet_to_display_source;
use crate::source_list::SourceList;
use crate::telnet::{Event, Message};
use crate::{
    AutoStandby, ContainmentAmount, DimmerMode, DiracFilter, DynamicVolume, EcoMode, MultiEq,
    Power, ReferenceLevelOffset, Sleep, ToneLevel, Volume, Zone,
};

/// A value of the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Setting {
    /// Power of a zone.
    Power,
    /// Volume of a zone.
    Volume,
    /// Volume limit of a zone.
    MaxVolume,
    /// Mute of a zone.
    Muted,
    /// Selected source of a zone.
    Source,
    /// Raw sound mode of a zone.
    SoundMode,
    /// Auto standby time of a zone.
    AutoStandby,
    /// Sleep timer of a zone.
    Sleep,
    /// Display dimmer of the receiver.
    Dimmer,
    /// Eco mode of the receiver.
    EcoMode,
    /// Setup menu of the receiver is open.
    SettingsMenu,
    /// Tone control of the receiver is on.
    ToneControl,
    /// Bass level of the receiver.
    Bass,
    /// Treble level of the receiver.
    Treble,
    /// Audyssey MultEQ mode of the receiver.
    MultiEq,
    /// Audyssey Dynamic EQ of the receiver is on.
    DynamicEq,
    /// Audyssey reference level offset of the receiver.
    ReferenceLevelOffset,
    /// Audyssey Dynamic Volume of the receiver.
    DynamicVolume,
    /// Audyssey LFC of the receiver is on.
    Lfc,
    /// Audyssey LFC containment amount of the receiver.
    ContainmentAmount,
    /// Dirac Live filter of the receiver.
    DiracFilter,
    /// A Dirac Live slot of the receiver, from 1 to 3.
    DiracSlot(u8),
    /// The video or audio signal of the receiver. Any value of the signal
    /// information changed.
    Signal,
    /// The names or the use of the input sources of the receiver. Any value
    /// of the source list changed.
    Sources,
    /// The list of sound modes that the receiver offers.
    SoundModes,
}

/// A value that changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Change {
    /// Zone of the value. `None` for values of the whole receiver.
    pub zone: Option<Zone>,
    /// Value that changed.
    pub setting: Setting,
}

/// Values of one zone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZoneState {
    /// Power.
    pub power: Option<Power>,
    /// Volume.
    pub volume: Option<Volume>,
    /// Volume limit. `None` if there is no limit or it is not known.
    pub max_volume: Option<Volume>,
    /// Mute.
    pub muted: Option<bool>,
    /// Selected source, as the receiver reports it.
    pub source: Option<String>,
    /// Sound mode in upper case, with single spaces. Only the main zone has one.
    pub sound_mode: Option<String>,
    /// Auto standby time.
    pub auto_standby: Option<AutoStandby>,
    /// Sleep timer.
    pub sleep: Option<Sleep>,
}

/// A Dirac Live filter slot. Telnet reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiracSlot {
    /// Name that the filter was saved with. `None` if the slot has none.
    pub name: Option<String>,
    /// A filter is in the slot.
    pub imported: Option<bool>,
    /// Kind of filter, as the receiver writes it, for example `ART`.
    pub kind: Option<String>,
}

/// Values of the whole receiver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    /// Display dimmer.
    pub dimmer: Option<DimmerMode>,
    /// Eco mode.
    pub eco_mode: Option<EcoMode>,
    /// The setup menu is open.
    pub settings_menu: Option<bool>,
    /// Tone control is on. The bass and treble levels only work then.
    pub tone_control: Option<bool>,
    /// Bass level.
    pub bass: Option<ToneLevel>,
    /// Treble level.
    pub treble: Option<ToneLevel>,
    /// Audyssey MultEQ mode.
    pub multi_eq: Option<MultiEq>,
    /// Audyssey Dynamic EQ is on.
    pub dynamic_eq: Option<bool>,
    /// Audyssey reference level offset. It works only with Dynamic EQ on.
    pub reference_level_offset: Option<ReferenceLevelOffset>,
    /// Audyssey Dynamic Volume.
    pub dynamic_volume: Option<DynamicVolume>,
    /// Audyssey LFC is on. Telnet reports it, `AppCommand` does not.
    pub lfc: Option<bool>,
    /// Audyssey LFC containment amount. Telnet reports it, `AppCommand` does
    /// not.
    pub containment_amount: Option<ContainmentAmount>,
    /// Dirac Live filter. Telnet reports it, `AppCommand` does not.
    pub dirac_filter: Option<DiracFilter>,
    /// The three Dirac Live slots. Slot 1 is the first. Telnet reports them.
    pub dirac_slots: [DiracSlot; 3],
    /// The video and audio signal. Telnet reports it.
    pub signal: SignalState,
    /// The input sources, with their names. Telnet reports them.
    pub sources: SourceList,
    /// The sound modes that the receiver offers. Telnet reports them.
    pub sound_modes: SoundModeList,
}

impl DeviceState {
    /// The Dirac Live filter of the slot with this name, if the state knows
    /// one. Upper and lower case are the same. A slot that has no filter is
    /// not used. If two slots have the name, the first one is used.
    #[must_use]
    pub fn dirac_filter_named(&self, name: &str) -> Option<DiracFilter> {
        let name = name.trim();
        let index = self.dirac_slots.iter().position(|slot| {
            slot.imported != Some(false)
                && slot
                    .name
                    .as_deref()
                    .is_some_and(|slot_name| slot_name.eq_ignore_ascii_case(name))
        })?;
        DiracFilter::from_slot_number(u8::try_from(index + 1).ok()?)
    }
}

/// State of a receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    single_zone: bool,
    device: DeviceState,
    zones: [ZoneState; 3],
}

impl State {
    /// Creates an empty state.
    ///
    /// Set `single_zone` for a receiver with only the main zone. It reports
    /// the power of the main zone with `PW` events, not `ZM` events.
    #[must_use]
    pub fn new(single_zone: bool) -> Self {
        Self {
            single_zone,
            device: DeviceState::default(),
            zones: Default::default(),
        }
    }

    /// Values of a zone.
    #[must_use]
    pub const fn zone(&self, zone: Zone) -> &ZoneState {
        &self.zones[zone.index()]
    }

    /// Values of the whole receiver.
    #[must_use]
    pub const fn device(&self) -> &DeviceState {
        &self.device
    }

    /// Applies a telnet event. Returns what changed.
    pub fn apply_message(&mut self, message: &Message<'_>) -> Option<Change> {
        let parameter = message.parameter;
        let zone = message.zone.zone();
        match message.event {
            Event::Zm if !self.single_zone => self.set_power(zone?, parameter),
            Event::Pw if self.single_zone => self.set_power(Zone::Main, parameter),
            Event::Z2 | Event::Z3 => self.set_power(zone?, parameter),
            Event::Mv => {
                let value = Volume::from_telnet(parameter)?;
                self.zone_change(zone?, Setting::Volume, |z| set(&mut z.volume, Some(value)))
            }
            Event::Mu => {
                let value = match parameter {
                    "ON" => true,
                    "OFF" => false,
                    _ => return None,
                };
                self.zone_change(zone?, Setting::Muted, |z| set(&mut z.muted, Some(value)))
            }
            Event::Si => {
                let source = telnet_to_display_source(parameter);
                self.zone_change(zone?, Setting::Source, |z| set_text(&mut z.source, source))
            }
            Event::Ms if zone == Some(Zone::Main) => {
                // Quick and smart select are not sound modes.
                if parameter.starts_with("QUICK") || parameter.starts_with("SMART") {
                    return None;
                }
                let mode = normalize_sound_mode(parameter);
                self.zone_change(zone?, Setting::SoundMode, |z| {
                    set_text(&mut z.sound_mode, &mode)
                })
            }
            Event::Stby => {
                let value = AutoStandby::from_any(parameter)?;
                self.zone_change(zone?, Setting::AutoStandby, |z| {
                    set(&mut z.auto_standby, Some(value))
                })
            }
            Event::Slp => {
                let value = Sleep::from_telnet(parameter)?;
                self.zone_change(zone?, Setting::Sleep, |z| set(&mut z.sleep, Some(value)))
            }
            Event::Ss if zone == Some(Zone::Main) => {
                let Some(value) = parameter.strip_prefix("VCTZMALIM") else {
                    if let Some(changed) = self.device.sources.apply_ss(parameter) {
                        return changed.then_some(Change {
                            zone: None,
                            setting: Setting::Sources,
                        });
                    }
                    return self.signal_change(|signal| signal.apply_ss(parameter));
                };
                let value = value.trim();
                let limit = if matches!(value, "OFF" | "") {
                    None
                } else {
                    Some(Volume::from_telnet(value)?)
                };
                self.zone_change(Zone::Main, Setting::MaxVolume, |z| {
                    set(&mut z.max_volume, limit)
                })
            }
            Event::Sy => self.signal_change(|signal| signal.apply_sy(parameter)),
            Event::Dim => {
                let value = DimmerMode::from_telnet(parameter.get(1..)?)?;
                self.device_change(Setting::Dimmer, |d| set(&mut d.dimmer, Some(value)))
            }
            Event::Eco if zone == Some(Zone::Main) => {
                let value = EcoMode::from_telnet(parameter)?;
                self.device_change(Setting::EcoMode, |d| set(&mut d.eco_mode, Some(value)))
            }
            Event::Mn => {
                let value = match parameter.strip_prefix("MEN")?.get(1..)? {
                    "ON" => true,
                    "OFF" => false,
                    _ => return None,
                };
                self.device_change(Setting::SettingsMenu, |d| {
                    set(&mut d.settings_menu, Some(value))
                })
            }
            Event::Ps => self.apply_sound_setting(parameter),
            Event::Op => {
                if let Some(changed) = self.device.sound_modes.apply_op(parameter) {
                    return changed.then_some(Change {
                        zone: None,
                        setting: Setting::SoundModes,
                    });
                }
                self.apply_dirac_slot(parameter)
            }
            _ => None,
        }
    }

    /// Applies a value from an `AppCommand` answer. Returns what changed.
    ///
    /// `value` is `None` for an empty element or a missing attribute. Values
    /// of fields that the state does not hold are ignored.
    pub fn apply_field(&mut self, zone: Zone, field: Field, value: Option<&str>) -> Option<Change> {
        match field {
            Field::Muted => {
                let muted = value.map(|v| v.eq_ignore_ascii_case("on"));
                self.zone_change(zone, Setting::Muted, |z| set(&mut z.muted, muted))
            }
            Field::Power => {
                let power = value.and_then(power_from_answer);
                self.zone_change(zone, Setting::Power, |z| set(&mut z.power, power))
            }
            Field::InputFunc => self.zone_change(zone, Setting::Source, |z| {
                set_optional_text(&mut z.source, value)
            }),
            Field::Volume => {
                let volume = value.and_then(volume_from_answer);
                self.zone_change(zone, Setting::Volume, |z| set(&mut z.volume, volume))
            }
            Field::MaxVolume => {
                let limit = value.and_then(limit_from_answer);
                self.zone_change(zone, Setting::MaxVolume, |z| set(&mut z.max_volume, limit))
            }
            Field::AutoStandby => {
                let standby = value.and_then(AutoStandby::from_any);
                self.zone_change(zone, Setting::AutoStandby, |z| {
                    set(&mut z.auto_standby, standby)
                })
            }
            // Only the main zone has a sound mode.
            Field::SoundModeRaw if zone != Zone::Main => None,
            Field::SoundModeRaw => {
                let mode = value.map(normalize_sound_mode);
                self.zone_change(zone, Setting::SoundMode, |z| {
                    set_optional_text(&mut z.sound_mode, mode.as_deref())
                })
            }
            Field::Dimmer => {
                let dimmer = value.and_then(DimmerMode::from_any);
                self.device_change(Setting::Dimmer, |d| set(&mut d.dimmer, dimmer))
            }
            Field::EcoMode => {
                let eco = value.and_then(EcoMode::from_any);
                self.device_change(Setting::EcoMode, |d| set(&mut d.eco_mode, eco))
            }
            Field::ToneControlAdjust => {
                let on = value.and_then(on_off_digit);
                self.device_change(Setting::ToneControl, |d| set(&mut d.tone_control, on))
            }
            Field::Bass => {
                let level = value.and_then(ToneLevel::from_answer);
                self.device_change(Setting::Bass, |d| set(&mut d.bass, level))
            }
            Field::Treble => {
                let level = value.and_then(ToneLevel::from_answer);
                self.device_change(Setting::Treble, |d| set(&mut d.treble, level))
            }
            Field::MultEq => {
                let mode = value.and_then(MultiEq::from_any);
                self.device_change(Setting::MultiEq, |d| set(&mut d.multi_eq, mode))
            }
            Field::DynamicEq => {
                let on = value.and_then(on_off_digit);
                self.device_change(Setting::DynamicEq, |d| set(&mut d.dynamic_eq, on))
            }
            Field::RefLevOffset => {
                let offset = value.and_then(ReferenceLevelOffset::from_any);
                self.device_change(Setting::ReferenceLevelOffset, |d| {
                    set(&mut d.reference_level_offset, offset)
                })
            }
            Field::DynamicVol => {
                let volume = value.and_then(DynamicVolume::from_any);
                self.device_change(Setting::DynamicVolume, |d| {
                    set(&mut d.dynamic_volume, volume)
                })
            }
            _ => None,
        }
    }

    /// Applies a `PS` event. The state holds the tone control, the Audyssey
    /// and the Dirac settings. The other `PS` events are ignored.
    fn apply_sound_setting(&mut self, parameter: &str) -> Option<Change> {
        let (name, value) = match parameter.strip_prefix("MULTEQ:") {
            Some(value) => ("MULTEQ", value),
            None => parameter.split_once(' ')?,
        };
        match name {
            "TONE" => {
                let on = on_off(value.strip_prefix("CTRL ")?)?;
                self.device_change(Setting::ToneControl, |d| set(&mut d.tone_control, Some(on)))
            }
            "BAS" => {
                let level = ToneLevel::from_telnet(value)?;
                self.device_change(Setting::Bass, |d| set(&mut d.bass, Some(level)))
            }
            "TRE" => {
                let level = ToneLevel::from_telnet(value)?;
                self.device_change(Setting::Treble, |d| set(&mut d.treble, Some(level)))
            }
            "MULTEQ" => {
                let mode = MultiEq::from_telnet(value)?;
                self.device_change(Setting::MultiEq, |d| set(&mut d.multi_eq, Some(mode)))
            }
            "DYNEQ" => {
                let on = on_off(value)?;
                self.device_change(Setting::DynamicEq, |d| set(&mut d.dynamic_eq, Some(on)))
            }
            "REFLEV" => {
                let offset = ReferenceLevelOffset::from_telnet(value)?;
                self.device_change(Setting::ReferenceLevelOffset, |d| {
                    set(&mut d.reference_level_offset, Some(offset))
                })
            }
            "DYNVOL" => {
                let volume = DynamicVolume::from_telnet(value)?;
                self.device_change(Setting::DynamicVolume, |d| {
                    set(&mut d.dynamic_volume, Some(volume))
                })
            }
            "LFC" => {
                let on = on_off(value)?;
                self.device_change(Setting::Lfc, |d| set(&mut d.lfc, Some(on)))
            }
            "CNTAMT" => {
                let amount = ContainmentAmount::from_telnet(value)?;
                self.device_change(Setting::ContainmentAmount, |d| {
                    set(&mut d.containment_amount, Some(amount))
                })
            }
            "DIRAC" => {
                let filter = DiracFilter::from_code(value)?;
                self.device_change(Setting::DiracFilter, |d| {
                    set(&mut d.dirac_filter, Some(filter))
                })
            }
            _ => None,
        }
    }

    /// Applies an `OP` event about a Dirac Live slot, for example
    /// `DLSSL1NAME Comte`. The other `OP` events are ignored.
    fn apply_dirac_slot(&mut self, parameter: &str) -> Option<Change> {
        let rest = parameter.strip_prefix("DLSSL")?;
        let number = rest.bytes().next()?.checked_sub(b'1').filter(|n| *n < 3)?;
        let (field, value) = rest.get(1..)?.split_once(' ')?;
        let value = value.trim();
        let change = |state: &mut Self, update: &dyn Fn(&mut DiracSlot) -> bool| {
            let slot = &mut state.device.dirac_slots[usize::from(number)];
            update(slot).then_some(Change {
                zone: None,
                setting: Setting::DiracSlot(number + 1),
            })
        };
        match field {
            "NAME" => change(self, &|slot| {
                set_optional_text(&mut slot.name, Some(value).filter(|name| !name.is_empty()))
            }),
            "IMP" => {
                let imported = on_off(value)?;
                change(self, &|slot| set(&mut slot.imported, Some(imported)))
            }
            "TYP" => change(self, &|slot| {
                set_optional_text(&mut slot.kind, Some(value).filter(|kind| !kind.is_empty()))
            }),
            _ => None,
        }
    }

    fn set_power(&mut self, zone: Zone, parameter: &str) -> Option<Change> {
        let power = Power::from_code(parameter)?;
        self.zone_change(zone, Setting::Power, |z| set(&mut z.power, Some(power)))
    }

    fn zone_change(
        &mut self,
        zone: Zone,
        setting: Setting,
        update: impl FnOnce(&mut ZoneState) -> bool,
    ) -> Option<Change> {
        update(&mut self.zones[zone.index()]).then_some(Change {
            zone: Some(zone),
            setting,
        })
    }

    fn signal_change(&mut self, update: impl FnOnce(&mut SignalState) -> bool) -> Option<Change> {
        update(&mut self.device.signal).then_some(Change {
            zone: None,
            setting: Setting::Signal,
        })
    }

    fn device_change(
        &mut self,
        setting: Setting,
        update: impl FnOnce(&mut DeviceState) -> bool,
    ) -> Option<Change> {
        update(&mut self.device).then_some(Change {
            zone: None,
            setting,
        })
    }
}

/// `ON` and `OFF`, as telnet events have them.
fn on_off(text: &str) -> Option<bool> {
    match text {
        "ON" => Some(true),
        "OFF" => Some(false),
        _ => None,
    }
}

/// `1` is on and `0` is off, as in an `AppCommand` answer.
fn on_off_digit(text: &str) -> Option<bool> {
    match text.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Replaces the value. Returns true if it was different.
fn set<T: PartialEq>(slot: &mut Option<T>, value: Option<T>) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

/// Like [`set`] for text. The buffer is reused if there is one.
fn set_text(slot: &mut Option<String>, value: &str) -> bool {
    match slot {
        Some(current) if current == value => false,
        Some(current) => {
            current.clear();
            current.push_str(value);
            true
        }
        None => {
            *slot = Some(value.to_owned());
            true
        }
    }
}

fn set_optional_text(slot: &mut Option<String>, value: Option<&str>) -> bool {
    match value {
        Some(value) => set_text(slot, value),
        None => slot.take().is_some(),
    }
}

/// A sound mode in upper case, with single spaces and no spaces at the ends.
///
/// Receivers write the same mode in different ways: `Stereo` in `AppCommand`
/// answers and `STEREO` in telnet events. One spelling keeps a mode that did
/// not change from looking like a change.
fn normalize_sound_mode(text: &str) -> Cow<'_, str> {
    let clean = text.split(' ').all(|word| {
        !word.is_empty()
            && !word.contains(char::is_whitespace)
            && !word.bytes().any(|b| b.is_ascii_lowercase())
    });
    if clean {
        return Cow::Borrowed(text);
    }
    let mut mode = String::with_capacity(text.len());
    for (index, word) in text.split_whitespace().enumerate() {
        if index > 0 {
            mode.push(' ');
        }
        mode.extend(word.chars().map(|c| c.to_ascii_uppercase()));
    }
    Cow::Owned(mode)
}

/// Power from an `AppCommand` answer. Some receivers answer in lower case.
fn power_from_answer(text: &str) -> Option<Power> {
    Power::ALL
        .iter()
        .copied()
        .find(|power| power.code().eq_ignore_ascii_case(text))
}

/// Volume in dB from an `AppCommand` answer. `--` is the lowest volume.
fn volume_from_answer(text: &str) -> Option<Volume> {
    let text = text.trim();
    if text == "--" {
        return Some(Volume::MIN);
    }
    Volume::from_db_rounded(text.parse().ok()?)
}

/// Volume limit in dB from an `AppCommand` answer. `OFF` is no limit.
fn limit_from_answer(text: &str) -> Option<Volume> {
    let text = text.trim();
    if matches!(text, "OFF" | "--" | "") {
        return None;
    }
    volume_from_answer(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(state: &mut State, line: &str) -> Option<Change> {
        state.apply_message(&Message::parse(line).unwrap())
    }

    #[allow(clippy::unnecessary_wraps)] // The tests compare with `Option<Change>`.
    fn change(zone: Option<Zone>, setting: Setting) -> Option<Change> {
        Some(Change { zone, setting })
    }

    #[test]
    fn power_follows_zone_events() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "ZMON"),
            change(Some(Zone::Main), Setting::Power)
        );
        assert_eq!(apply(&mut state, "ZMON"), None);
        assert_eq!(
            apply(&mut state, "Z2OFF"),
            change(Some(Zone::Zone2), Setting::Power)
        );
        assert_eq!(
            apply(&mut state, "Z3STANDBY"),
            change(Some(Zone::Zone3), Setting::Power)
        );
        assert_eq!(state.zone(Zone::Main).power, Some(Power::On));
        assert_eq!(state.zone(Zone::Zone2).power, Some(Power::Off));
        assert_eq!(state.zone(Zone::Zone3).power, Some(Power::Standby));
        assert_eq!(
            apply(&mut state, "ZMOFF"),
            change(Some(Zone::Main), Setting::Power)
        );
        assert_eq!(state.zone(Zone::Main).power, Some(Power::Off));
    }

    #[test]
    fn device_power_event_is_ignored_with_several_zones() {
        let mut state = State::new(false);
        assert_eq!(apply(&mut state, "PWON"), None);
        assert_eq!(state.zone(Zone::Main).power, None);
    }

    #[test]
    fn single_zone_receiver_uses_device_power() {
        let mut state = State::new(true);
        assert_eq!(apply(&mut state, "ZMON"), None);
        assert_eq!(
            apply(&mut state, "PWSTANDBY"),
            change(Some(Zone::Main), Setting::Power)
        );
        assert_eq!(state.zone(Zone::Main).power, Some(Power::Standby));
    }

    #[test]
    fn unknown_power_text_is_ignored() {
        let mut state = State::new(false);
        apply(&mut state, "ZMON");
        assert_eq!(apply(&mut state, "ZMBROKEN"), None);
        assert_eq!(state.zone(Zone::Main).power, Some(Power::On));
    }

    #[test]
    fn volume_and_mute_per_zone() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "MV435"),
            change(Some(Zone::Main), Setting::Volume)
        );
        assert_eq!(
            apply(&mut state, "Z250"),
            change(Some(Zone::Zone2), Setting::Volume)
        );
        assert_eq!(apply(&mut state, "MV435"), None);
        assert_eq!(state.zone(Zone::Main).volume.map(Volume::db), Some(-36.5));
        assert_eq!(state.zone(Zone::Zone2).volume.map(Volume::db), Some(-30.0));
        assert_eq!(state.zone(Zone::Zone3).volume, None);
        assert_eq!(
            apply(&mut state, "MUON"),
            change(Some(Zone::Main), Setting::Muted)
        );
        assert_eq!(
            apply(&mut state, "Z2MUON"),
            change(Some(Zone::Zone2), Setting::Muted)
        );
        assert_eq!(
            apply(&mut state, "MUOFF"),
            change(Some(Zone::Main), Setting::Muted)
        );
        assert_eq!(state.zone(Zone::Main).muted, Some(false));
        assert_eq!(state.zone(Zone::Zone2).muted, Some(true));
    }

    #[test]
    fn garbage_in_events_keeps_the_last_value() {
        let mut state = State::new(false);
        apply(&mut state, "MV435");
        apply(&mut state, "MUON");
        assert_eq!(apply(&mut state, "MV4350"), None);
        assert_eq!(apply(&mut state, "MVxyz"), None);
        assert_eq!(apply(&mut state, "MUMAYBE"), None);
        assert_eq!(state.zone(Zone::Main).volume.map(Volume::db), Some(-36.5));
        assert_eq!(state.zone(Zone::Main).muted, Some(true));
    }

    #[test]
    fn max_volume_from_the_limit_event() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "SSVCTZMALIM 70"),
            change(Some(Zone::Main), Setting::MaxVolume)
        );
        assert_eq!(
            state.zone(Zone::Main).max_volume.map(Volume::db),
            Some(-10.0)
        );
        assert_eq!(
            apply(&mut state, "SSVCTZMALIM OFF"),
            change(Some(Zone::Main), Setting::MaxVolume)
        );
        assert_eq!(state.zone(Zone::Main).max_volume, None);
        assert_eq!(apply(&mut state, "SSVCTZMALIM OFF"), None);
        assert_eq!(apply(&mut state, "SSOTHER 1"), None);
        assert_eq!(apply(&mut state, "SSVCTZMALIM x"), None);
    }

    #[test]
    fn source_uses_display_names() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "SIGAME"),
            change(Some(Zone::Main), Setting::Source)
        );
        assert_eq!(apply(&mut state, "SIGAME"), None);
        assert_eq!(state.zone(Zone::Main).source.as_deref(), Some("GAME"));
        apply(&mut state, "SIIRADIO");
        assert_eq!(
            state.zone(Zone::Main).source.as_deref(),
            Some("Internet Radio")
        );
        apply(&mut state, "Z2SAT/CBL");
        assert_eq!(state.zone(Zone::Zone2).source.as_deref(), Some("SAT/CBL"));
    }

    #[test]
    fn sound_mode_is_normalized_and_filters_select_events() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "MSDOLBY AUDIO - DD+   + DSUR"),
            change(Some(Zone::Main), Setting::SoundMode)
        );
        assert_eq!(
            state.zone(Zone::Main).sound_mode.as_deref(),
            Some("DOLBY AUDIO - DD+ + DSUR")
        );
        assert_eq!(apply(&mut state, "MSQUICK1"), None);
        assert_eq!(apply(&mut state, "MSSMART2"), None);
        assert_eq!(apply(&mut state, "MSDOLBY AUDIO - DD+ + DSUR"), None);
        assert_eq!(
            state.zone(Zone::Main).sound_mode.as_deref(),
            Some("DOLBY AUDIO - DD+ + DSUR")
        );
    }

    #[test]
    fn only_the_main_zone_has_a_sound_mode() {
        let mut state = State::new(false);
        assert_eq!(apply(&mut state, "Z2MSSTEREO"), None);
        for zone in [Zone::Zone2, Zone::Zone3] {
            assert_eq!(
                state.apply_field(zone, Field::SoundModeRaw, Some("Stereo")),
                None
            );
            assert_eq!(state.zone(zone).sound_mode, None);
        }
        assert_eq!(
            state.apply_field(Zone::Main, Field::SoundModeRaw, Some("Stereo")),
            change(Some(Zone::Main), Setting::SoundMode)
        );
    }

    #[test]
    fn standby_sleep_and_device_values() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "STBY2H"),
            change(Some(Zone::Main), Setting::AutoStandby)
        );
        assert_eq!(
            apply(&mut state, "Z2STBYOFF"),
            change(Some(Zone::Zone2), Setting::AutoStandby)
        );
        assert_eq!(apply(&mut state, "STBYxx"), None);
        assert_eq!(
            state.zone(Zone::Main).auto_standby,
            Some(AutoStandby::Hours2)
        );
        assert_eq!(
            apply(&mut state, "SLP010"),
            change(Some(Zone::Main), Setting::Sleep)
        );
        assert_eq!(
            apply(&mut state, "Z3SLPOFF"),
            change(Some(Zone::Zone3), Setting::Sleep)
        );
        assert_eq!(apply(&mut state, "SLP999"), None);
        assert_eq!(state.zone(Zone::Main).sleep, Some(Sleep::Minutes(10)));
        assert_eq!(state.zone(Zone::Zone3).sleep, Some(Sleep::Off));

        assert_eq!(apply(&mut state, "DIM BRI"), change(None, Setting::Dimmer));
        assert_eq!(apply(&mut state, "DIM BRI"), None);
        assert_eq!(apply(&mut state, "DIM XXX"), None);
        assert_eq!(state.device().dimmer, Some(DimmerMode::Bright));
        assert_eq!(apply(&mut state, "ECOAUTO"), change(None, Setting::EcoMode));
        assert_eq!(state.device().eco_mode, Some(EcoMode::Auto));
        assert_eq!(
            apply(&mut state, "MNMEN ON"),
            change(None, Setting::SettingsMenu)
        );
        assert_eq!(state.device().settings_menu, Some(true));
        assert_eq!(
            apply(&mut state, "MNMEN OFF"),
            change(None, Setting::SettingsMenu)
        );
        assert_eq!(apply(&mut state, "MNCUP"), None);
        assert_eq!(state.device().settings_menu, Some(false));
    }

    #[test]
    fn other_events_do_not_change_the_state() {
        let mut state = State::new(false);
        let before = state.clone();
        for line in [
            "NSE1Hello",
            "PSDRC OFF",
            "VSMONI1",
            "CVFL 50",
            "TR1 ON",
            "SYHDMIDIAGERR NO",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(state, before);
    }

    #[test]
    fn answer_values() {
        let mut state = State::new(false);
        let main = Zone::Main;
        assert_eq!(
            state.apply_field(main, Field::Power, Some("ON")),
            change(Some(main), Setting::Power)
        );
        assert_eq!(state.apply_field(main, Field::Power, Some("ON")), None);
        state.apply_field(main, Field::Volume, Some("-36.5"));
        state.apply_field(main, Field::MaxVolume, Some("-10"));
        state.apply_field(main, Field::Muted, Some("on"));
        state.apply_field(main, Field::InputFunc, Some("MPLAY"));
        state.apply_field(main, Field::SoundModeRaw, Some("DTS  SURROUND"));
        state.apply_field(main, Field::AutoStandby, Some("120"));
        state.apply_field(main, Field::Dimmer, Some("3"));
        state.apply_field(main, Field::EcoMode, Some("2"));
        let zone = state.zone(main);
        assert_eq!(zone.power, Some(Power::On));
        assert_eq!(zone.volume.map(Volume::db), Some(-36.5));
        assert_eq!(zone.max_volume.map(Volume::db), Some(-10.0));
        assert_eq!(zone.muted, Some(true));
        assert_eq!(zone.source.as_deref(), Some("MPLAY"));
        assert_eq!(zone.sound_mode.as_deref(), Some("DTS SURROUND"));
        assert_eq!(zone.auto_standby, Some(AutoStandby::Hours2));
        assert_eq!(state.device().dimmer, Some(DimmerMode::Bright));
        assert_eq!(state.device().eco_mode, Some(EcoMode::Auto));
    }

    #[test]
    fn answer_special_values() {
        let mut state = State::new(false);
        let zone = Zone::Zone2;
        state.apply_field(zone, Field::Volume, Some("--"));
        assert_eq!(state.zone(zone).volume, Some(Volume::MIN));
        state.apply_field(zone, Field::MaxVolume, Some("OFF"));
        assert_eq!(state.zone(zone).max_volume, None);
        // Text that is not a value makes the value unknown.
        state.apply_field(zone, Field::Volume, Some("loud"));
        assert_eq!(state.zone(zone).volume, None);
        state.apply_field(zone, Field::Power, Some("ON"));
        assert_eq!(
            state.apply_field(zone, Field::Power, Some("MAYBE")),
            change(Some(zone), Setting::Power)
        );
        assert_eq!(state.zone(zone).power, None);
        // An empty value is unknown too.
        state.apply_field(zone, Field::InputFunc, Some("CD"));
        assert_eq!(
            state.apply_field(zone, Field::InputFunc, None),
            change(Some(zone), Setting::Source)
        );
        assert_eq!(state.zone(zone).source, None);
        assert_eq!(state.apply_field(zone, Field::InputFunc, None), None);
        // Values that the state does not hold are ignored.
        assert_eq!(
            state.apply_field(zone, Field::BassLevel, Some("+1dB")),
            None
        );
    }

    #[test]
    fn tone_control_follows_events_and_answers() {
        let mut state = State::new(false);
        let message = |parameter: &str| format!("PS{parameter}");
        let device = |state: &State| state.device().clone();
        let setting = |setting| {
            Some(Change {
                zone: None,
                setting,
            })
        };
        assert_eq!(
            apply(&mut state, &message("TONE CTRL ON")),
            setting(Setting::ToneControl)
        );
        assert_eq!(apply(&mut state, &message("TONE CTRL ON")), None);
        assert_eq!(
            apply(&mut state, &message("BAS 53")),
            setting(Setting::Bass)
        );
        assert_eq!(
            apply(&mut state, &message("TRE 44")),
            setting(Setting::Treble)
        );
        let tone = device(&state);
        assert_eq!(tone.tone_control, Some(true));
        assert_eq!(tone.bass, ToneLevel::from_db(3));
        assert_eq!(tone.treble, Some(ToneLevel::MIN));
        // Text that is not a value keeps the last value.
        for text in ["BAS 99", "BAS", "TRE x", "TONE CTRL MAYBE", "DRC OFF"] {
            assert_eq!(apply(&mut state, &message(text)), None, "{text}");
        }
        assert_eq!(device(&state), tone);
        // An answer has the steps above the lowest level.
        assert_eq!(
            state.apply_field(Zone::Main, Field::Bass, Some("12")),
            setting(Setting::Bass)
        );
        assert_eq!(state.device().bass, Some(ToneLevel::MAX));
        assert_eq!(
            state.apply_field(Zone::Main, Field::ToneControlAdjust, Some("0")),
            setting(Setting::ToneControl)
        );
        assert_eq!(state.device().tone_control, Some(false));
        assert_eq!(
            state.apply_field(Zone::Main, Field::Treble, Some("13")),
            setting(Setting::Treble)
        );
        assert_eq!(state.device().treble, None);
    }

    #[test]
    fn audyssey_and_dirac_follow_events_and_answers() {
        let mut state = State::new(false);
        let setting = |setting| {
            Some(Change {
                zone: None,
                setting,
            })
        };
        let cases = [
            ("PSMULTEQ:AUDYSSEY", Setting::MultiEq),
            ("PSDYNEQ ON", Setting::DynamicEq),
            ("PSREFLEV 10", Setting::ReferenceLevelOffset),
            ("PSDYNVOL HEV", Setting::DynamicVolume),
            ("PSLFC ON", Setting::Lfc),
            ("PSCNTAMT 04", Setting::ContainmentAmount),
            ("PSDIRAC 2", Setting::DiracFilter),
        ];
        for (line, expected) in cases {
            assert_eq!(apply(&mut state, line), setting(expected), "{line}");
            assert_eq!(apply(&mut state, line), None, "{line} again");
        }
        let device = state.device().clone();
        assert_eq!(device.multi_eq, Some(MultiEq::Reference));
        assert_eq!(device.dynamic_eq, Some(true));
        assert_eq!(
            device.reference_level_offset,
            Some(ReferenceLevelOffset::Db10)
        );
        assert_eq!(device.dynamic_volume, Some(DynamicVolume::Heavy));
        assert_eq!(device.lfc, Some(true));
        assert_eq!(
            device.containment_amount.map(ContainmentAmount::get),
            Some(4)
        );
        assert_eq!(device.dirac_filter, Some(DiracFilter::Slot2));
        // Text that is not a value keeps the last value.
        for line in [
            "PSMULTEQ:X",
            "PSDYNEQ MAYBE",
            "PSREFLEV 7",
            "PSDYNVOL X",
            "PSLFC",
            "PSCNTAMT 09",
            "PSDIRAC 9",
            "PSDIRAC",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(state.device(), &device);
        // The values of an answer use the other codes.
        let answer =
            |state: &mut State, field, text| state.apply_field(Zone::Main, field, Some(text));
        assert_eq!(
            answer(&mut state, Field::MultEq, "2"),
            setting(Setting::MultiEq)
        );
        assert_eq!(state.device().multi_eq, Some(MultiEq::LrBypass));
        assert_eq!(
            answer(&mut state, Field::DynamicEq, "0"),
            setting(Setting::DynamicEq)
        );
        assert_eq!(state.device().dynamic_eq, Some(false));
        assert_eq!(
            answer(&mut state, Field::RefLevOffset, "3"),
            setting(Setting::ReferenceLevelOffset)
        );
        assert_eq!(
            state.device().reference_level_offset,
            Some(ReferenceLevelOffset::Db15)
        );
        assert_eq!(
            answer(&mut state, Field::DynamicVol, "1"),
            setting(Setting::DynamicVolume)
        );
        assert_eq!(state.device().dynamic_volume, Some(DynamicVolume::Light));
        // An answer with something else makes the value unknown.
        assert_eq!(
            answer(&mut state, Field::MultEq, "9"),
            setting(Setting::MultiEq)
        );
        assert_eq!(state.device().multi_eq, None);
        // Values that the answer does not have stay.
        assert_eq!(state.device().dirac_filter, Some(DiracFilter::Slot2));
    }

    #[test]
    fn sources_follow_events() {
        let mut state = State::new(false);
        let sources = || change(None, Setting::Sources);
        assert_eq!(apply(&mut state, "SSFUNMPLAY Apple TV   "), sources());
        assert_eq!(apply(&mut state, "SSFUNMPLAY Apple TV   "), None);
        assert_eq!(apply(&mut state, "SSSODMPLAY USE"), sources());
        assert_eq!(apply(&mut state, "SSSODTV DEL"), sources());
        // The ends of the lists and the other name lists are not changes.
        assert_eq!(apply(&mut state, "SSFUN END"), None);
        assert_eq!(apply(&mut state, "SSSOD END"), None);
        assert_eq!(apply(&mut state, "SSFUNGDNBD Blu-ray"), None);
        let list = &state.device().sources;
        assert_eq!(list.name("MPLAY"), Some("Apple TV"));
        assert_eq!(list.get("MPLAY").unwrap().in_use, Some(true));
        assert_eq!(list.get("TV").unwrap().in_use, Some(false));
        // Signal events still reach the signal state.
        assert_eq!(
            apply(&mut state, "SSINFSIGRES I4K50"),
            change(None, Setting::Signal)
        );
    }

    #[test]
    fn sound_modes_follow_events() {
        let mut state = State::new(false);
        assert_eq!(apply(&mut state, "OPSMLALL MOV011Stereo"), None);
        assert_eq!(apply(&mut state, "OPSMLALL MUS080Stereo"), None);
        assert_eq!(state.device().sound_modes.as_slice(), []);
        assert_eq!(
            apply(&mut state, "OPSMLALL END"),
            change(None, Setting::SoundModes)
        );
        assert_eq!(state.device().sound_modes.as_slice().len(), 2);
        // The Dirac slots still work.
        assert_eq!(
            apply(&mut state, "OPDLSSL1IMP ON"),
            change(None, Setting::DiracSlot(1))
        );
    }

    #[test]
    fn dirac_slots_follow_events() {
        let mut state = State::new(false);
        let slot = |n| {
            Some(Change {
                zone: None,
                setting: Setting::DiracSlot(n),
            })
        };
        // The receiver pads the name with spaces.
        let name = format!("OPDLSSL1NAME {:<55}", "Comte");
        assert_eq!(apply(&mut state, &name), slot(1));
        assert_eq!(apply(&mut state, &name), None);
        assert_eq!(apply(&mut state, "OPDLSSL1IMP ON"), slot(1));
        assert_eq!(apply(&mut state, "OPDLSSL1TYP BC"), slot(1));
        assert_eq!(apply(&mut state, "OPDLSSL2NAME Dirac ART"), slot(2));
        assert_eq!(apply(&mut state, "OPDLSSL2TYP ART"), slot(2));
        assert_eq!(apply(&mut state, "OPDLSSL3IMP OFF"), slot(3));
        let slots = &state.device().dirac_slots;
        assert_eq!(slots[0].name.as_deref(), Some("Comte"));
        assert_eq!(slots[0].imported, Some(true));
        assert_eq!(slots[0].kind.as_deref(), Some("BC"));
        assert_eq!(slots[1].name.as_deref(), Some("Dirac ART"));
        assert_eq!(slots[1].imported, None);
        assert_eq!(slots[2].imported, Some(false));
        // A name of only spaces means the slot has none.
        assert_eq!(apply(&mut state, "OPDLSSL2NAME      "), slot(2));
        assert_eq!(state.device().dirac_slots[1].name, None);
        // A name finds its slot. A slot without a filter is not used.
        assert_eq!(
            state.device().dirac_filter_named("comte"),
            Some(DiracFilter::Slot1)
        );
        assert_eq!(
            state.device().dirac_filter_named(" Comte "),
            Some(DiracFilter::Slot1)
        );
        assert_eq!(state.device().dirac_filter_named("Dirac ART"), None);
        assert_eq!(state.device().dirac_filter_named(""), None);
        apply(&mut state, "OPDLSSL2NAME Dirac ART");
        assert_eq!(
            state.device().dirac_filter_named("DIRAC art"),
            Some(DiracFilter::Slot2)
        );
        apply(&mut state, "OPDLSSL2IMP OFF");
        assert_eq!(state.device().dirac_filter_named("Dirac ART"), None);
        apply(&mut state, "OPDLSSL2IMP ON");
        apply(&mut state, "OPDLSSL2NAME      ");
        // Other events and text that is not a value change nothing.
        let before = state.clone();
        for line in [
            "OPDLS END",
            "OPDLSSL4NAME X",
            "OPDLSSL0NAME X",
            "OPDLSSL1IMP MAYBE",
            "OPDLSSL1",
            "OPDLSSL1COLOR RED",
            "OPSLP010",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(state, before);
    }

    #[test]
    fn signal_events_change_the_signal() {
        let mut state = State::new(false);
        let signal = Some(Change {
            zone: None,
            setting: Setting::Signal,
        });
        // The values of a real receiver.
        for line in [
            "SSINFSIGRES I4K50",
            "SSINFSIGHDR I---",
            "SSINFSIGCOS 22",
            "SSINFSIGECI 00",
            "SSINFSIGPIX 11",
            "SSINFAISFSV 48K",
            "SYSDA PCM                  ",
            "SYSMI Stereo",
            "SYSDVOUT 4K50",
        ] {
            let changed = apply(&mut state, line);
            // The line before it can have set the same value.
            assert!(
                changed == signal || changed.is_none(),
                "{line}: {changed:?}"
            );
        }
        let signal_state = &state.device().signal;
        assert_eq!(signal_state.video.input.as_deref(), Some("4K50"));
        assert_eq!(signal_state.video.output.as_deref(), Some("4K50"));
        assert_eq!(signal_state.audio_sound.as_deref(), Some("Stereo"));
        assert_eq!(signal_state.sampling_rate.as_deref(), Some("48"));
        assert_eq!(apply(&mut state, "SSINFSIGRES I4K50"), None);
        assert_eq!(apply(&mut state, "SSINFSIGRES I4K60"), signal);
        // The volume limit shares the event and is not mixed up with it.
        assert_eq!(
            apply(&mut state, "SSVCTZMALIM 60"),
            change(Some(Zone::Main), Setting::MaxVolume)
        );
        assert_eq!(apply(&mut state, "SSTTR OFF"), None);
    }

    #[test]
    fn zones_are_independent() {
        let mut state = State::new(false);
        state.apply_field(Zone::Zone3, Field::Muted, Some("ON"));
        assert_eq!(state.zone(Zone::Zone3).muted, Some(true));
        assert_eq!(state.zone(Zone::Main).muted, None);
        assert_eq!(state.zone(Zone::Zone2).muted, None);
    }

    #[test]
    fn sound_modes_are_normalized() {
        assert!(matches!(
            normalize_sound_mode("DTS SURROUND"),
            Cow::Borrowed(_)
        ));
        assert_eq!(normalize_sound_mode("  A \t B  "), "A B");
        assert_eq!(normalize_sound_mode("Stereo"), "STEREO");
        assert_eq!(normalize_sound_mode("Dolby  Atmos + x"), "DOLBY ATMOS + X");
        assert_eq!(normalize_sound_mode(""), "");
    }

    #[test]
    fn a_sound_mode_that_only_differs_in_case_is_not_a_change() {
        let mut state = State::new(false);
        assert!(
            state
                .apply_field(Zone::Main, Field::SoundModeRaw, Some("Stereo"))
                .is_some()
        );
        assert_eq!(apply(&mut state, "MSSTEREO"), None);
        assert_eq!(state.zone(Zone::Main).sound_mode.as_deref(), Some("STEREO"));
    }
}
