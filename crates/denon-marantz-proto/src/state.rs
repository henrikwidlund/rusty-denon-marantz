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
use crate::source::telnet_to_display_source;
use crate::telnet::{Event, Message};
use crate::{AutoStandby, DimmerMode, EcoMode, Power, Sleep, Volume, Zone};

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
    /// Sound mode, as the receiver reports it, with single spaces.
    pub sound_mode: Option<String>,
    /// Auto standby time.
    pub auto_standby: Option<AutoStandby>,
    /// Sleep timer.
    pub sleep: Option<Sleep>,
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
            Event::Ms => {
                // Quick and smart select are not sound modes.
                if parameter.starts_with("QUICK") || parameter.starts_with("SMART") {
                    return None;
                }
                let mode = collapse_spaces(parameter);
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
                let value = parameter.strip_prefix("VCTZMALIM")?.trim();
                let limit = if matches!(value, "OFF" | "") {
                    None
                } else {
                    Some(Volume::from_telnet(value)?)
                };
                self.zone_change(Zone::Main, Setting::MaxVolume, |z| {
                    set(&mut z.max_volume, limit)
                })
            }
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
            Field::SoundModeRaw => {
                let mode = value.map(collapse_spaces);
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

/// Text with white space replaced by single spaces, without spaces at the ends.
fn collapse_spaces(text: &str) -> Cow<'_, str> {
    let clean = text
        .split(' ')
        .all(|word| !word.is_empty() && !word.contains(char::is_whitespace));
    if clean {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(text.split_whitespace().collect::<Vec<_>>().join(" "))
    }
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
            "PSBAS 50",
            "VSMONI1",
            "CVFL 50",
            "TR1 ON",
            "SYSDA ANALOG",
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
        assert_eq!(state.apply_field(zone, Field::Bass, Some("50")), None);
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
    fn spaces_are_collapsed_without_allocating_when_clean() {
        assert!(matches!(collapse_spaces("DTS SURROUND"), Cow::Borrowed(_)));
        assert_eq!(collapse_spaces("  A \t B  "), "A B");
        assert_eq!(collapse_spaces(""), "");
    }
}
