//! State of a receiver, updated from telnet events, `AppCommand` answers and
//! pages of the web interface.
//!
//! A value is `None` until the receiver reports it, or if the receiver reports
//! something that is not a known value.
//!
//! Telnet events change one value at a time. An event with text that cannot be
//! read is ignored, so the last known value stays. An `AppCommand` answer is a
//! full snapshot. A value in it that cannot be read makes the value unknown.
//!
//! The web interface has values that telnet does not report. A page fills its
//! values when it is read, and the receiver does not send changes of it.

use std::borrow::Cow;

use crate::appcommand::Field;
use crate::device_settings::BLUETOOTH_LEVELS;
use crate::general_menu::{GeneralPart, GeneralSettings};
use crate::input_pages::SourceLevels;
use crate::input_pages::{InputAssignment, SourceNames};
use crate::network_audio::MAX_NETWORK_PRESET;
use crate::now_playing::{NowPlaying, PlayingKind};
use crate::picture::PictureSettings;
use crate::settings_availability::{GroupAvailability, SettingsAvailability};
use crate::signal::ChannelLayout;
use crate::signal::SignalState;
use crate::signal_pages::{AudioChannelsPage, VideoSignalPage};
use crate::sound_mode_list::{SoundModeCategory, SoundModeList};
use crate::sound_settings::SoundSettings;
use crate::source::{playing_kind, telnet_to_display_source, xml_to_telnet_source};
use crate::source_list::SourceList;
use crate::speaker_extras::{Playback2Ch, XlrAssign};
use crate::speaker_setup::{SpeakerGroup, SpeakerSetup};
use crate::speaker_tables::{BassSettings, SpeakerTable};
use crate::system_settings::SystemSettings;
use crate::telnet::{Event, Message};
use crate::tuner::{TunerPage, TunerState};
use crate::video::VideoSettings;
use crate::video_menu::{VideoMenu, VideoMenuPart};
use crate::zone_settings::MAX_QUICK_SELECT;
use crate::{
    AutoStandby, BluetoothOutputMode, Channel, ChannelLevel, ContainmentAmount, DigitalCodec,
    DimmerMode, DiracFilter, DynamicVolume, EcoMode, HdmiOutput, Illumination, InputModeState,
    MultiEq, MuteLevel, PanelLock, Power, PowerOnVolume, ReferenceLevelOffset, Sleep,
    SpeakerPreset, ToneLevel, Trigger, Volume, VolumeDisplay, VolumeMode, Zone, ZoneChannelMode,
    ZoneHdmiAudio,
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
    /// The high pass filter of zone 2 or 3.
    HighPassFilter,
    /// Stereo or mono in zone 2 or 3.
    ChannelMode,
    /// The HDMI audio of zone 2 or 3.
    ZoneHdmiAudio,
    /// The level of the left or right channel of zone 2 or 3.
    ChannelLevels,
    /// The level of a channel of the main zone, the `CV` events.
    ChannelVolumes,
    /// The audio input mode of the main zone.
    InputMode,
    /// Speaker crossovers, configuration or graphic EQ.
    SpeakerSetup,
    /// Which settings the receiver offers and lets the user change, from the
    /// web interface.
    SettingsAvailability,
    /// The settings that only the web interface gives, such as HDMI control.
    SystemSettings,
    /// What the receiver plays: title, artist, album and station.
    NowPlaying,
    /// The video menu of the web interface: on-screen display, TV format,
    /// screen saver, HDMI upscaler, HDCP and the signal format of the inputs.
    /// Any value of the menu changed.
    VideoMenu,
    /// The general menu of the web interface: language, firmware update
    /// settings, usage data and the trigger assignment. Any value changed.
    GeneralSettings,
    /// How the receiver plays two-channel sources, from the web interface.
    Playback2Ch,
    /// What the XLR outputs of the subwoofers carry, from the web interface.
    XlrAssign,
    /// The levels of the analog and digital inputs, from the web interface.
    SourceLevels,
    /// Variable or fixed volume of a zone.
    VolumeMode,
    /// The volume of a zone when it is switched on.
    PowerOnVolume,
    /// How far the volume goes down when a zone is muted.
    MuteLevel,
    /// How the receiver shows the volume.
    VolumeDisplay,
    /// Mute of a zone.
    Muted,
    /// Selected source of a zone.
    Source,
    /// Raw sound mode of a zone.
    SoundMode,
    /// Category of the sound mode of a zone.
    SoundModeCategory,
    /// Auto standby time of a zone.
    AutoStandby,
    /// Sleep timer of a zone.
    Sleep,
    /// Display dimmer of the receiver.
    Dimmer,
    /// Eco mode of the receiver.
    EcoMode,
    /// All zone stereo.
    AllZoneStereo,
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
    /// Name of a zone.
    ZoneName,
    /// Name that the user gave the receiver.
    FriendlyName,
    /// Auto lip sync of the receiver is on.
    AutoLipSync,
    /// The remote control lock of the receiver.
    RemoteLock,
    /// The panel lock of the receiver.
    PanelLock,
    /// The name of a quick select.
    QuickSelectNames,
    /// The level of a speaker channel.
    SpeakerLevels,
    /// A sound setting such as the audio restorer or the room size.
    SoundSettings,
    /// A trigger output of the receiver is on or off.
    Triggers,
    /// The Bluetooth transmitter or its output mode.
    Bluetooth,
    /// The digital input mode of the receiver.
    DigitalCodec,
    /// A picture setting such as the picture mode or the contrast.
    PictureSettings,
    /// The illumination of a Marantz receiver.
    Illumination,
    /// The quick select (smart select on Marantz) of a zone.
    QuickSelect,
    /// The speaker preset of the receiver.
    SpeakerPreset,
    /// Video select of the receiver: on or off, or its source.
    VideoSelect,
    /// `InstaPrevue` of the receiver is shown.
    InstaPrevue,
    /// The source of the recording output (REC SELECT).
    RecordSource,
    /// The name of a network audio preset.
    NetworkPresetNames,
    /// The band, mode, frequency, preset or station of the tuner, the HD
    /// Radio state or the tuner presets.
    Tuner,
    /// A video setting such as the video processing mode or the HDMI output.
    VideoSettings,
}

/// A value that changed. It names the setting and the zone, not the new
/// value: read the state to get it.
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
    /// Volume when the zone is switched on. The main zone calls it power on
    /// level. Telnet reports it, and a change in the web interface or the app
    /// also comes as an event. A change in the menu of the receiver does not.
    pub power_on_volume: Option<PowerOnVolume>,
    /// Variable or fixed volume. Telnet reports it for zone 2 and 3.
    pub volume_mode: Option<VolumeMode>,
    /// Mute.
    pub muted: Option<bool>,
    /// How far the volume goes down when the zone is muted. Telnet reports
    /// it, and a change in the web interface or the app also comes as an
    /// event. A change in the menu of the receiver does not.
    pub mute_level: Option<MuteLevel>,
    /// How the receiver shows the volume. Telnet reports it for the main
    /// zone.
    pub volume_display: Option<VolumeDisplay>,
    /// High pass filter is on. Telnet reports it for zone 2 and 3 when it
    /// changes.
    pub high_pass_filter: Option<bool>,
    /// Stereo or mono. Telnet reports it for zone 2 and 3 when it changes.
    pub channel_mode: Option<ZoneChannelMode>,
    /// HDMI audio of zone 2 and 3. Telnet reports it when it changes.
    pub hdmi_audio: Option<ZoneHdmiAudio>,
    /// The quick select of the zone, from 1 to [`MAX_QUICK_SELECT`], or 0 for
    /// none. A Marantz receiver calls it smart select. Telnet reports it.
    pub quick_select: Option<u8>,
    /// Bass level. Telnet reports it for zone 2 and 3 when it changes, and
    /// `AppCommand0300.xml` gives it.
    pub bass: Option<ToneLevel>,
    /// Treble level. Telnet reports it for zone 2 and 3 when it changes, and
    /// `AppCommand0300.xml` gives it.
    pub treble: Option<ToneLevel>,
    /// Level of the left channel of zone 2 or 3. Telnet reports it when it
    /// changes, and `AppCommand0300.xml` gives it.
    pub left_level: Option<ChannelLevel>,
    /// Level of the right channel of zone 2 or 3. Telnet reports it when it
    /// changes, and `AppCommand0300.xml` gives it.
    pub right_level: Option<ChannelLevel>,
    /// Selected source, as the receiver reports it.
    pub source: Option<String>,
    /// Sound mode in upper case, with single spaces. Only the main zone has one.
    pub sound_mode: Option<String>,
    /// Category of the sound mode, as the movie, music, game and pure buttons
    /// select it. Only the main zone has one. Telnet reports it when it
    /// changes, and `AppCommand0300.xml` when it is asked. An answer of
    /// `AppCommand0300.xml` with no category set keeps the value.
    pub sound_mode_category: Option<SoundModeCategory>,
    /// Auto standby time.
    pub auto_standby: Option<AutoStandby>,
    /// Sleep timer.
    pub sleep: Option<Sleep>,
    /// Name of the zone, for example `ZONE2`. Only `AppCommand` reports it,
    /// and the receiver does not send a rename on telnet: it changes at the
    /// next update.
    pub name: Option<String>,
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
    /// All zone stereo is on. The receiver reports it with `MNZST` events.
    pub all_zone_stereo: Option<bool>,
    /// The setup menu is open.
    pub settings_menu: Option<bool>,
    /// Tone control is on. The bass and treble levels only work then.
    pub tone_control: Option<bool>,
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
    /// Name that the user gave the receiver. A receiver that has no name
    /// reports an empty one on telnet, which does not change this value.
    pub friendly_name: Option<String>,
    /// Auto lip sync is on. Telnet reports it, and `AppCommand0300.xml` gives
    /// it.
    pub auto_lip_sync: Option<bool>,
    /// The remote control is locked. Telnet reports it, also when it changes.
    pub remote_lock: Option<bool>,
    /// What the panel lock locks. Telnet reports it, also when it changes.
    pub panel_lock: Option<PanelLock>,
    /// The digital input mode. Telnet reports it.
    pub digital_codec: Option<DigitalCodec>,
    /// The picture settings, for example the picture mode. Telnet reports
    /// them on older receivers.
    pub picture_settings: PictureSettings,
    /// The Bluetooth transmitter is on. Telnet reports it.
    pub bluetooth_transmitter: Option<bool>,
    /// Where the sound goes when the Bluetooth transmitter is on. Telnet
    /// reports it.
    pub bluetooth_output: Option<BluetoothOutputMode>,
    /// Bluetooth output level, from 30 (-50 dB) to 90 (+10 dB). 80 is 0 dB.
    /// Telnet reports it.
    pub bluetooth_level: Option<u8>,
    /// Brightness of the illumination. Marantz receivers only. Telnet reports
    /// it.
    pub illumination: Option<Illumination>,
    /// Names of the quick selects. The first is quick select 1, the last at
    /// most [`MAX_QUICK_SELECT`]. A quick select with no name, or that the
    /// receiver has not reported, is `None`.
    /// Telnet reports them.
    pub quick_select_names: Vec<Option<String>>,
    /// Level of each speaker channel that the receiver has reported, in the
    /// order of the report. Telnet reports them on a Denon receiver.
    pub speaker_levels: Vec<(Channel, ChannelLevel)>,
    /// Level of each channel of the main zone that the receiver has reported
    /// as `CV` events, in the order of the report.
    pub channel_volumes: Vec<(Channel, ChannelLevel)>,
    /// The channels that the receiver uses now, in the order of the latest
    /// list: the `CV` list of telnet up to its `CVEND` line, or the channels
    /// that the web interface page shows. The receiver sends a new list when
    /// the input or the signal changes. [`DeviceState::channel_volumes`] keeps
    /// the levels of the channels of earlier lists too.
    pub active_channels: Vec<Channel>,
    /// The audio input mode of the main zone, the `SD` events: the mode that
    /// was set, or an input that the receiver reports only, such as ARC.
    pub input_mode: Option<InputModeState>,
    /// Speaker crossovers, configuration and graphic EQ. Telnet reports them.
    pub speaker_setup: SpeakerSetup,
    /// Which settings the receiver offers and lets the user change now. Only
    /// the web interface has it, and it changes with the sound mode and the
    /// signal, so it is read on request.
    pub settings_availability: SettingsAvailability,
    /// Settings that telnet does not report, such as HDMI control, the menu
    /// lock and when the network answers. Only the web interface has them,
    /// and it does not send changes, so it is read on request.
    pub system_settings: SystemSettings,
    /// The video menu of the web interface. Only the web interface has it, so
    /// it is read on request.
    pub video_menu: VideoMenu,
    /// The general menu of the web interface: language, firmware update
    /// settings, usage data and the trigger assignment.
    pub general_settings: GeneralSettings,
    /// How the receiver plays two-channel sources. `None` until the page was
    /// read.
    pub playback_2ch: Option<Playback2Ch>,
    /// What the XLR outputs carry. `None` until the page was read.
    pub xlr_assign: Option<XlrAssign>,
    /// The levels of the inputs.
    pub source_levels: SourceLevels,
    /// The sound settings that the state has no value for above, such as
    /// the audio restorer. Telnet reports them.
    pub sound_settings: SoundSettings,
    /// What the tuner reports. Telnet reports it for the tuner source.
    pub tuner: TunerState,
    /// What network audio or HD radio plays. Telnet reports it in `NSE` and
    /// `HD` events and the status pages have it. A receiver that uses HEOS for
    /// network audio gives neither.
    pub now_playing: NowPlaying,
    /// The speaker preset. Telnet reports it.
    pub speaker_preset: Option<SpeakerPreset>,
    /// Video select is on: the video comes from
    /// [`video_select_source`](Self::video_select_source), not from the
    /// selected source. Telnet reports it.
    pub video_select: Option<bool>,
    /// The source of the video when video select is on. Telnet reports it.
    /// `None` when an older Marantz receiver reports `SOURCE`: the video of
    /// the selected source.
    pub video_select_source: Option<String>,
    /// Each trigger output is on. The first is trigger 1. Telnet reports them.
    pub triggers: [Option<bool>; 3],
    /// The video settings, for example the video processing mode. Telnet
    /// reports them.
    pub video_settings: VideoSettings,
    /// `InstaPrevue`, the previews of the HDMI sources, is shown. Older
    /// receivers only.
    pub insta_prevue: Option<bool>,
    /// The source of the recording output (REC SELECT). `None` when the
    /// receiver reports `SOURCE`: the selected source. Older receivers only.
    pub record_source: Option<String>,
    /// Names of the network audio presets, as the receiver sends them. The
    /// first is preset 1. A preset that has no name, or that the receiver
    /// has not reported, is `None`. They come after
    /// [`NetworkAudioCommand::PresetNames`](crate::NetworkAudioCommand::PresetNames).
    /// Older receivers only.
    pub network_preset_names: Vec<Option<String>>,
}

/// Sets the level of a channel in a list. Returns true if the list changed.
/// Replaces the active channels. Returns true if the list is different. An
/// empty list is ignored, so a list that has no known channel changes nothing.
fn set_active_channels(active: &mut Vec<Channel>, list: Vec<Channel>) -> bool {
    if list.is_empty() || *active == list {
        return false;
    }
    *active = list;
    true
}

fn set_channel_level(
    levels: &mut Vec<(Channel, ChannelLevel)>,
    channel: Channel,
    level: ChannelLevel,
) -> bool {
    if let Some((_, current)) = levels.iter_mut().find(|(known, _)| *known == channel) {
        return std::mem::replace(current, level) != level;
    }
    levels.push((channel, level));
    true
}

impl DeviceState {
    /// Whether the trigger output is on, if the receiver has reported it.
    #[must_use]
    pub const fn trigger(&self, trigger: Trigger) -> Option<bool> {
        self.triggers[trigger.number() as usize - 1]
    }

    /// Whether the active Dirac filter is a Dirac ART filter, if the state
    /// knows. `Some(false)` if Dirac is off or the filter is of another kind,
    /// for example Bass Control. `None` if the state does not know the filter
    /// or its kind.
    ///
    /// A Dirac ART filter locks the channel levels.
    /// Other filters, such as Bass Control, do not. Tone control is blocked by
    /// any Dirac filter, not only by this one.
    #[must_use]
    pub fn dirac_art_active(&self) -> Option<bool> {
        let slot = match self.dirac_filter? {
            DiracFilter::Off => return Some(false),
            DiracFilter::Slot1 => 0,
            DiracFilter::Slot2 => 1,
            DiracFilter::Slot3 => 2,
        };
        let kind = self.dirac_slots[slot].kind.as_deref()?;
        Some(kind.eq_ignore_ascii_case("ART"))
    }

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
///
/// A value of `None` means that the receiver has not reported it yet, or that
/// it does not have the setting. The doc of a field says if `None` also has
/// another meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    single_zone: bool,
    device: DeviceState,
    zones: [ZoneState; 3],
    /// The source of the main zone came from a telnet `SI` event, so it is a
    /// telnet name or its display name and not a name that the user gave the
    /// source. An `AppCommand` answer or a status page with the same text
    /// keeps it.
    main_source_is_code: bool,
    /// Channels of the `CV` list that is being received.
    pending_channels: Vec<Channel>,
    /// Changes that events caused besides the ones that they returned.
    follow_ups: Vec<Change>,
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
            main_source_is_code: false,
            pending_channels: Vec::new(),
            follow_ups: Vec::new(),
        }
    }

    /// Takes the changes that applied events caused besides the ones that they
    /// returned. A source change of the main zone clears
    /// [`DeviceState::now_playing`], which is a change of its own. Call it
    /// after applying events. The client does it.
    pub fn take_follow_ups(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.follow_ups)
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

    /// What the source of the main zone plays.
    ///
    /// A source from a telnet `SI` event is a telnet name or its display
    /// name, which gives the kind. If an `AppCommand` answer or a status page
    /// then gives the same text, it is also taken as a telnet name. Another
    /// source from there can be a name that the user gave the source. It is
    /// first searched by that name in [`DeviceState::sources`], and the telnet
    /// name of the found source gives the kind. If a name fits more than one
    /// source, for example a name that the user gave is the same as the name
    /// of another source, the found source can be wrong.
    ///
    /// A change of the kind gives a [`Setting::Source`] or a
    /// [`Setting::Sources`] change, also when an `SI` event has the same text
    /// as the source before.
    #[must_use]
    pub fn main_playing_kind(&self) -> Option<PlayingKind> {
        let source = self.zone(Zone::Main).source.as_deref()?.trim_end();
        if self.main_source_is_code {
            return playing_kind(source);
        }
        self.device
            .sources
            .as_slice()
            .iter()
            .find(|info| info.name.as_deref() == Some(source))
            .map_or_else(|| playing_kind(source), |info| playing_kind(&info.code))
    }

    /// Applies a telnet event. Returns what changed.
    pub fn apply_message(&mut self, message: &Message<'_>) -> Option<Change> {
        let parameter = message.parameter;
        let zone = message.zone.zone();
        match message.event {
            Event::Zm if !self.single_zone => self.set_power(zone?, parameter),
            Event::Pw if self.single_zone => self.set_power(Zone::Main, parameter),
            Event::Z2 | Event::Z3 => self.apply_zone_event(zone?, parameter),
            Event::Hd if matches!(zone, Some(Zone::Zone2 | Zone::Zone3)) => {
                self.apply_zone_hdmi_audio(zone?, parameter)
            }
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
            Event::Si => self.apply_source_event(zone?, parameter),
            Event::Ms if zone == Some(Zone::Main) => {
                // Quick and smart select are not sound modes.
                if parameter.starts_with("QUICK") || parameter.starts_with("SMART") {
                    return self.apply_quick_select(Zone::Main, parameter);
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
            Event::Sd if zone == Some(Zone::Main) => {
                let mode = InputModeState::from_code(parameter.trim())?;
                self.device_change(Setting::InputMode, |d| set(&mut d.input_mode, Some(mode)))
            }
            Event::Ss if zone == Some(Zone::Main) => {
                let Some(value) = parameter.strip_prefix("VCTZMALIM") else {
                    return self.apply_device_ss(parameter);
                };
                let value = value.trim();
                let limit = if matches!(value, "OFF" | "") {
                    None
                } else {
                    Some(Volume::from_telnet_clamped(value)?)
                };
                self.zone_change(Zone::Main, Setting::MaxVolume, |z| {
                    set(&mut z.max_volume, limit)
                })
            }
            Event::Sy if parameter.contains("LOCK") => self.apply_lock(parameter),
            Event::Sy => self.signal_change(|signal| signal.apply_sy(parameter)),
            Event::Bl if zone == Some(Zone::Main) => {
                self.sound_change(|s| s.apply_balance(parameter))
            }
            Event::Dim => {
                let value = DimmerMode::from_telnet(parameter.get(1..)?)?;
                self.device_change(Setting::Dimmer, |d| set(&mut d.dimmer, Some(value)))
            }
            Event::Eco if zone == Some(Zone::Main) => {
                let value = EcoMode::from_telnet(parameter)?;
                self.device_change(Setting::EcoMode, |d| set(&mut d.eco_mode, Some(value)))
            }
            Event::Mn => self.apply_menu(parameter),
            Event::Ns => {
                if let Some(name) = parameter.strip_prefix("FRN ") {
                    return self.apply_friendly_name(name.trim());
                }
                let changed = self.apply_network_preset_name(parameter.strip_prefix('H')?)?;
                changed.then_some(Change {
                    zone: None,
                    setting: Setting::NetworkPresetNames,
                })
            }
            // Zone 2 and 3 send their own bass and treble as `PS` events.
            Event::Ps if zone == Some(Zone::Main) => self.apply_sound_setting(parameter),
            event if event.is_device_report() => self.apply_device_report(event, parameter),
            Event::Vs if zone == Some(Zone::Main) => self.apply_video_setting(parameter),
            Event::Ps if matches!(zone, Some(Zone::Zone2 | Zone::Zone3)) => {
                self.apply_zone_tone(zone?, parameter)
            }
            Event::Cv if zone == Some(Zone::Main) => self.apply_channel_volume(parameter),
            Event::Cv if matches!(zone, Some(Zone::Zone2 | Zone::Zone3)) => {
                self.apply_zone_channel_level(zone?, parameter)
            }
            Event::Op => self.apply_option(parameter),
            _ => None,
        }
    }

    /// Stores the physical inputs of the sources, from the input assign page
    /// of the web interface. They are part of [`DeviceState::sources`].
    pub fn apply_input_assignments(&mut self, assignments: &[InputAssignment]) -> Option<Change> {
        self.device
            .sources
            .set_assignments(assignments)
            .then_some(sources_change())
    }

    /// Stores the default and automatic names of the sources and the longest
    /// name, from the source names page of the web interface. They are part of
    /// [`DeviceState::sources`].
    pub fn apply_web_source_names(&mut self, names: &SourceNames) -> Option<Change> {
        self.device
            .sources
            .set_name_details(names)
            .then_some(sources_change())
    }

    /// Stores the availability of a group of settings from the web interface.
    /// It is part of [`DeviceState::settings_availability`].
    pub fn apply_settings_availability(&mut self, group: GroupAvailability) -> Option<Change> {
        self.device
            .settings_availability
            .set(group)
            .then_some(Change {
                zone: None,
                setting: Setting::SettingsAvailability,
            })
    }

    /// Stores the video signal of the web interface page. It is
    /// [`DeviceState::signal`], with the values of telnet.
    pub fn apply_video_signal_page(&mut self, page: &VideoSignalPage) -> Option<Change> {
        self.signal_change(|signal| signal.apply_video_page(page))
    }

    /// Stores the channels of the web interface page. They are
    /// [`DeviceState::signal`], with the values of telnet.
    pub fn apply_audio_channels_page(&mut self, page: &AudioChannelsPage) -> Option<Change> {
        self.signal_change(|signal| signal.apply_channels_page(page))
    }

    /// Stores what a page of the video menu told. It is in
    /// [`DeviceState::video_menu`].
    pub fn apply_video_menu(&mut self, part: VideoMenuPart) -> Option<Change> {
        self.device_change(Setting::VideoMenu, |device| device.video_menu.apply(part))
    }

    /// Stores what a page of the general menu told. It is in
    /// [`DeviceState::general_settings`].
    pub fn apply_general_settings(&mut self, part: GeneralPart) -> Option<Change> {
        self.device_change(Setting::GeneralSettings, |device| {
            device.general_settings.apply(part)
        })
    }

    /// Stores the playback page of two-channel sources. It is
    /// [`DeviceState::playback_2ch`].
    pub fn apply_playback_2ch(&mut self, page: Playback2Ch) -> Option<Change> {
        self.device_change(Setting::Playback2Ch, |device| {
            replace_if_different(&mut device.playback_2ch, Some(page))
        })
    }

    /// Stores the XLR output assignment. It is [`DeviceState::xlr_assign`].
    pub fn apply_xlr_assign(&mut self, page: XlrAssign) -> Option<Change> {
        self.device_change(Setting::XlrAssign, |device| {
            replace_if_different(&mut device.xlr_assign, Some(page))
        })
    }

    /// Stores the levels of the inputs. They are
    /// [`DeviceState::source_levels`].
    pub fn apply_source_levels(&mut self, levels: SourceLevels) -> Option<Change> {
        self.device_change(Setting::SourceLevels, |device| {
            replace_if_different(&mut device.source_levels, levels)
        })
    }

    /// Stores the system settings of the web interface. They are
    /// [`DeviceState::system_settings`].
    pub fn apply_system_settings(&mut self, settings: SystemSettings) -> Option<Change> {
        (self.device.system_settings != settings).then(|| {
            self.device.system_settings = settings;
            Change {
                zone: None,
                setting: Setting::SystemSettings,
            }
        })
    }

    /// Stores the speaker distances of the web interface page, and the number
    /// of subwoofers. They are part of [`DeviceState::speaker_setup`].
    pub fn apply_speaker_distances(&mut self, table: SpeakerTable<Channel>) -> Option<Change> {
        self.device
            .speaker_setup
            .apply_distances(table)
            .then_some(speaker_setup_change())
    }

    /// Stores the speaker crossovers of the web interface page. They join the
    /// crossovers that telnet reports, in [`DeviceState::speaker_setup`].
    pub fn apply_speaker_crossovers(
        &mut self,
        table: &SpeakerTable<SpeakerGroup>,
    ) -> Option<Change> {
        self.device
            .speaker_setup
            .apply_crossover_table(table)
            .then_some(speaker_setup_change())
    }

    /// Stores the bass page of the web interface. It is part of
    /// [`DeviceState::speaker_setup`].
    pub fn apply_bass_settings(&mut self, bass: BassSettings) -> Option<Change> {
        self.device
            .speaker_setup
            .apply_bass(bass)
            .then_some(speaker_setup_change())
    }

    /// Stores the channel levels of the channel level page of the web
    /// control, for the channels that the page shows. They join the levels
    /// that telnet reports in [`DeviceState::channel_volumes`]. A level that
    /// is not a multiple of 0.5 dB is ignored.
    pub fn apply_channel_level_table(&mut self, table: &SpeakerTable<Channel>) -> Option<Change> {
        let mut changed = false;
        let shown: Vec<Channel> = table
            .entries
            .iter()
            .filter(|entry| entry.is_shown())
            .map(|entry| entry.key)
            .collect();
        changed |= set_active_channels(&mut self.device.active_channels, shown);
        for entry in table.entries.iter().filter(|entry| entry.is_shown()) {
            let level = (entry.value % 5 == 0)
                .then(|| i8::try_from(entry.value / 5).ok())
                .flatten()
                .and_then(ChannelLevel::from_half_db);
            if let Some(level) = level {
                changed |= set_channel_level(&mut self.device.channel_volumes, entry.key, level);
            }
        }
        changed.then_some(Change {
            zone: Some(Zone::Main),
            setting: Setting::ChannelVolumes,
        })
    }

    /// Applies one entry of the `AppCommand` answer to `GetRenameSource`, or of
    /// the source lists of a status page: the name of the source in the XML,
    /// and its new name. `None` is no new name.
    pub fn apply_renamed_source(&mut self, name: &str, rename: Option<&str>) -> Option<Change> {
        let code = xml_to_telnet_source(name);
        let changed = self.device.sources.set_name(code, rename);
        changed.then_some(Change {
            zone: None,
            setting: Setting::Sources,
        })
    }

    /// Applies one entry of the `AppCommand` answer to `GetDeletedSource`: the
    /// name of the source in the XML, and its `use` value, `1` or `0`. Another
    /// value is ignored.
    pub fn apply_source_use(&mut self, name: &str, value: Option<&str>) -> Option<Change> {
        let in_use = value.and_then(on_off_digit)?;
        self.apply_source_in_use(name, in_use)
    }

    /// Sets if the source with this name in the XML is in use, for example
    /// from [`status_page::sources`](crate::status_page::sources).
    pub fn apply_source_in_use(&mut self, name: &str, in_use: bool) -> Option<Change> {
        let code = xml_to_telnet_source(name);
        let changed = self.device.sources.set_in_use(code, in_use);
        changed.then_some(Change {
            zone: None,
            setting: Setting::Sources,
        })
    }

    /// Applies a value from an `AppCommand` answer. Returns what changed.
    ///
    /// `value` is `None` for an empty element or a missing attribute. Values
    /// of fields that the state does not hold are ignored.
    pub fn apply_field(&mut self, zone: Zone, field: Field, value: Option<&str>) -> Option<Change> {
        match field {
            Field::Muted => {
                let muted = value.and_then(on_off_answer);
                self.zone_change(zone, Setting::Muted, |z| set(&mut z.muted, muted))
            }
            Field::Power => {
                let power = value.and_then(power_from_answer);
                self.zone_change(zone, Setting::Power, |z| set(&mut z.power, power))
            }
            Field::InputFunc => {
                let current = self.zone(Zone::Main).source.as_deref().map(str::trim_end);
                let is_code = self.main_source_is_code && value.map(str::trim_end) == current;
                self.main_source_change(zone, is_code, |z| set_optional_text(&mut z.source, value))
            }
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
            Field::HdmiOutput => {
                let output = value.map(str::trim).and_then(HdmiOutput::from_appcommand);
                self.device_change(Setting::VideoSettings, |d| {
                    set(&mut d.video_settings.hdmi_output, output)
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
                self.zone_change(zone, Setting::Bass, |z| set(&mut z.bass, level))
            }
            Field::Treble => {
                let level = value.and_then(ToneLevel::from_answer);
                self.zone_change(zone, Setting::Treble, |z| set(&mut z.treble, level))
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
            Field::ZoneName => {
                let name = value.map(str::trim).filter(|name| !name.is_empty());
                self.zone_change(zone, Setting::ZoneName, |z| {
                    set_optional_text(&mut z.name, name)
                })
            }
            // An empty name does not change the value.
            Field::FriendlyName => self.apply_friendly_name(value?.trim()),
            Field::VideoSignalIn
            | Field::VideoSignalOut
            | Field::AudioSignal
            | Field::AudioSound
            | Field::SamplingRate => {
                self.signal_change(|signal| signal.apply_appcommand(field, value))
            }
            _ => None,
        }
    }

    /// Stores the channels of the audio signal that the receiver gets
    /// (`is_input`) or plays, as the `AppCommand0300.xml` grids give them. They
    /// are in [`DeviceState::signal`], with the values of telnet. `None`
    /// clears the channels. For an answer item use
    /// [`State::apply_channel_item`], which keeps the values on an error.
    pub fn apply_channel_grid(
        &mut self,
        is_input: bool,
        layout: Option<ChannelLayout>,
    ) -> Option<Change> {
        self.signal_change(|signal| signal.channels.set(is_input, layout))
    }

    /// Stores the channels of the answer to `GetInputSignal` (`is_input`) or
    /// `GetActiveSpeaker`, as [`State::apply_channel_grid`] does. An item with
    /// an error has no channels, and the state keeps its values.
    pub fn apply_channel_item(
        &mut self,
        is_input: bool,
        item: &crate::appcommand::Item<'_>,
    ) -> Option<Change> {
        if item.is_error() {
            return None;
        }
        self.apply_channel_grid(is_input, ChannelLayout::from_appcommand(item))
    }

    /// Applies a `PS` event. The state holds the tone control, the Audyssey
    /// and the Dirac settings. The other `PS` events are ignored.
    fn apply_sound_setting(&mut self, parameter: &str) -> Option<Change> {
        // These names end with a colon, and the value follows with no space.
        let (name, value) = if let Some(value) = parameter.strip_prefix("MULTEQ:") {
            ("MULTEQ", value)
        } else if let Some(value) = parameter.strip_prefix("SP:") {
            ("SP:", value)
        } else if let Some((name, value)) = COLON_NAMES
            .iter()
            .find_map(|name| Some((*name, parameter.strip_prefix(name)?.strip_prefix(':')?)))
        {
            (name, value)
        } else {
            parameter.split_once(' ')?
        };
        match name {
            "TONE" => {
                let on = on_off(value.strip_prefix("CTRL ")?)?;
                self.device_change(Setting::ToneControl, |d| set(&mut d.tone_control, Some(on)))
            }
            "BAS" => {
                let level = ToneLevel::from_telnet(value)?;
                self.zone_change(Zone::Main, Setting::Bass, |z| set(&mut z.bass, Some(level)))
            }
            "TRE" => {
                let level = ToneLevel::from_telnet(value)?;
                self.zone_change(Zone::Main, Setting::Treble, |z| {
                    set(&mut z.treble, Some(level))
                })
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
            _ => {
                let changed = self.device.sound_settings.apply(name, value);
                changed.then_some(Change {
                    zone: None,
                    setting: Setting::SoundSettings,
                })
            }
        }
    }

    /// Applies an event about what plays. `apply` says if the text is a value
    /// and if it changed the value.
    fn apply_now_playing_event(
        &mut self,
        apply: impl FnOnce(&mut NowPlaying) -> Option<bool>,
    ) -> Option<Change> {
        apply(&mut self.device.now_playing)?.then_some(Change {
            zone: None,
            setting: Setting::NowPlaying,
        })
    }

    /// Stores the band and frequency of the status page of the tuner. A value
    /// that the page does not have stays. It is [`DeviceState::tuner`].
    pub fn apply_tuner_page(&mut self, page: TunerPage) -> Option<Change> {
        self.device.tuner.apply_page(page).then_some(Change {
            zone: None,
            setting: Setting::Tuner,
        })
    }

    /// Stores what plays, read from a status page of the receiver. It
    /// replaces all values. An empty value clears what was known. It is
    /// [`DeviceState::now_playing`].
    pub fn apply_now_playing(&mut self, now_playing: NowPlaying) -> Option<Change> {
        (self.device.now_playing != now_playing).then(|| {
            self.device.now_playing = now_playing;
            Change {
                zone: None,
                setting: Setting::NowPlaying,
            }
        })
    }

    /// Applies a tuner event. `apply` says if the text is a value and if it
    /// changed the tuner state.
    fn apply_tuner(
        &mut self,
        apply: impl FnOnce(&mut TunerState) -> Option<bool>,
    ) -> Option<Change> {
        apply(&mut self.device.tuner)?.then_some(Change {
            zone: None,
            setting: Setting::Tuner,
        })
    }

    /// Applies an `SP` event, for example `PR 1`.
    fn apply_speaker_preset(&mut self, parameter: &str) -> Option<Change> {
        let preset = SpeakerPreset::from_code(parameter.strip_prefix("PR ")?.trim())?;
        self.device_change(Setting::SpeakerPreset, |d| {
            set(&mut d.speaker_preset, Some(preset))
        })
    }

    /// Applies the quick select of a zone, for example `QUICK3` or `SMART0`.
    fn apply_quick_select(&mut self, zone: Zone, parameter: &str) -> Option<Change> {
        let number: u8 = parameter
            .get(5..)?
            .trim()
            .parse()
            .ok()
            .filter(|n| *n <= MAX_QUICK_SELECT)?;
        self.zone_change(zone, Setting::QuickSelect, |z| {
            set(&mut z.quick_select, Some(number))
        })
    }

    /// Applies the events about settings of the whole receiver: triggers,
    /// Bluetooth, picture, digital input, illumination, speaker preset and
    /// tuner.
    fn apply_device_report(&mut self, event: Event, parameter: &str) -> Option<Change> {
        match event {
            Event::Tr => self.apply_trigger(parameter),
            Event::Bt => self.apply_bluetooth(parameter),
            Event::Pv => self.apply_picture_setting(parameter),
            Event::Dc => self.apply_digital_codec(parameter),
            Event::Ilb => self.apply_illumination(parameter),
            Event::Sp => self.apply_speaker_preset(parameter),
            Event::Sv => self.apply_video_select(parameter),
            Event::Sr => self.apply_record_source(parameter),
            Event::Tf => self.apply_tuner(|tuner| tuner.apply_frequency(parameter)),
            Event::Tp => self.apply_tuner(|tuner| tuner.apply_preset(parameter)),
            Event::Tm => self.apply_tuner(|tuner| tuner.apply_band_or_mode(parameter)),
            Event::Da => self.apply_tuner(|tuner| tuner.dab.apply(parameter)),
            Event::Nse => self.apply_now_playing_event(|now| now.apply_net_audio_line(parameter)),
            Event::Hd => self
                .apply_now_playing_event(|now| now.apply_hd_radio(parameter))
                .or_else(|| self.apply_tuner(|tuner| tuner.hd.apply_status(parameter))),
            _ => None,
        }
    }

    /// Applies an `SV` event: `ON`, `OFF` or a source, for example `DVD`.
    /// Older Marantz receivers send `SOURCE`: the video of the selected
    /// source, so no video select source.
    fn apply_video_select(&mut self, parameter: &str) -> Option<Change> {
        let parameter = parameter.trim();
        if let Some(on) = on_off(parameter) {
            return self
                .device_change(Setting::VideoSelect, |d| set(&mut d.video_select, Some(on)));
        }
        // `LAST` is a command only.
        match parameter {
            "" | "?" | "LAST" => None,
            "SOURCE" => self.device_change(Setting::VideoSelect, |d| {
                set(&mut d.video_select_source, None)
            }),
            source => {
                let source = telnet_to_display_source(source);
                self.device_change(Setting::VideoSelect, |d| {
                    set_text(&mut d.video_select_source, source)
                })
            }
        }
    }

    /// Applies an `SR` event: a source, for example `CD`, or `SOURCE`.
    fn apply_record_source(&mut self, parameter: &str) -> Option<Change> {
        let source = match parameter.trim() {
            "" | "?" => return None,
            "SOURCE" => None,
            source => Some(telnet_to_display_source(source)),
        };
        self.device_change(Setting::RecordSource, |d| match source {
            Some(source) => set_text(&mut d.record_source, source),
            None => set(&mut d.record_source, None),
        })
    }

    /// Applies an `NSH` answer without `NSH`, for example `00Radio 1`: the
    /// preset number from `00` and the name. Returns `None` if it is not a
    /// preset name, and otherwise true if the names changed. An empty name
    /// means no name.
    fn apply_network_preset_name(&mut self, rest: &str) -> Option<bool> {
        let digits = rest.get(..2)?;
        if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let index = usize::from(digits.parse::<u8>().ok()?);
        if index >= usize::from(MAX_NETWORK_PRESET) {
            return None;
        }
        let name = rest[2..].trim();
        let names = &mut self.device.network_preset_names;
        let is_new = names.len() <= index;
        if is_new {
            names.resize(index + 1, None);
        }
        Some(set_optional_text(&mut names[index], Some(name).filter(|n| !n.is_empty())) || is_new)
    }

    /// Applies a `PV` event, for example `MOV` or `CN 050`.
    fn apply_picture_setting(&mut self, parameter: &str) -> Option<Change> {
        let changed = self.device.picture_settings.apply(parameter)?;
        changed.then_some(Change {
            zone: None,
            setting: Setting::PictureSettings,
        })
    }

    /// Applies a `DC` event, for example `AUTO`.
    fn apply_digital_codec(&mut self, parameter: &str) -> Option<Change> {
        let mode = DigitalCodec::from_code(parameter.trim())?;
        self.device_change(Setting::DigitalCodec, |d| {
            set(&mut d.digital_codec, Some(mode))
        })
    }

    /// Applies an `ILB` event, for example ` AUTO`.
    fn apply_illumination(&mut self, parameter: &str) -> Option<Change> {
        let mode = Illumination::from_code(parameter.trim())?;
        self.device_change(Setting::Illumination, |d| {
            set(&mut d.illumination, Some(mode))
        })
    }

    /// Applies a `BT` event: `TX ON`, `TX OFF`, `TX SP`, `TX BT` or the level,
    /// for example `LEV 80`.
    fn apply_bluetooth(&mut self, parameter: &str) -> Option<Change> {
        if let Some(level) = parameter.strip_prefix("LEV ") {
            let level = level.trim();
            if level.len() != 2 || !level.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let level: u8 = level
                .parse()
                .ok()
                .filter(|l| BLUETOOTH_LEVELS.contains(l))?;
            return self.device_change(Setting::Bluetooth, |d| {
                set(&mut d.bluetooth_level, Some(level))
            });
        }
        let value = parameter.strip_prefix("TX ")?.trim();
        if let Some(on) = on_off(value) {
            return self.device_change(Setting::Bluetooth, |d| {
                set(&mut d.bluetooth_transmitter, Some(on))
            });
        }
        let mode = BluetoothOutputMode::from_code(value)?;
        self.device_change(Setting::Bluetooth, |d| {
            set(&mut d.bluetooth_output, Some(mode))
        })
    }

    /// Applies a `TR` event, for example `1 ON`.
    fn apply_trigger(&mut self, parameter: &str) -> Option<Change> {
        let (number, value) = parameter.split_at_checked(1)?;
        let trigger = Trigger::from_number(number.parse().ok()?)?;
        let on = on_off(value.trim())?;
        self.device_change(Setting::Triggers, |d| {
            set(&mut d.triggers[usize::from(trigger.number()) - 1], Some(on))
        })
    }

    /// Applies a `VS` event of the main zone.
    fn apply_video_setting(&mut self, parameter: &str) -> Option<Change> {
        let changed = self.device.video_settings.apply(parameter)?;
        changed.then_some(Change {
            zone: None,
            setting: Setting::VideoSettings,
        })
    }

    /// Applies an `OP` event: the list of sound modes, the audio channels,
    /// auto lip sync and the Dirac Live slots. The other `OP` events are
    /// ignored.
    fn apply_option(&mut self, parameter: &str) -> Option<Change> {
        if parameter.starts_with("INF") {
            return self.signal_change(|signal| signal.apply_op(parameter));
        }
        if let Some(changed) = self.device.sound_modes.apply_op(parameter) {
            return changed.then_some(Change {
                zone: None,
                setting: Setting::SoundModes,
            });
        }
        if let Some(value) = parameter.strip_prefix("ALSSET ") {
            return self.apply_auto_lip_sync(value);
        }
        if let Some(rest) = parameter.strip_prefix("TPN") {
            return self.apply_tuner(|tuner| tuner.apply_preset_name(rest));
        }
        self.apply_dirac_slot(parameter)
    }

    /// Applies a lock event: `REMOTE LOCK ON`, `PANEL LOCK ON` (the panel),
    /// `PANEL+V LOCK ON` (the panel and the master volume) and `PANEL LOCK OFF`.
    /// Other lock events are ignored.
    fn apply_lock(&mut self, parameter: &str) -> Option<Change> {
        if let Some(value) = parameter.strip_prefix("REMOTE LOCK ") {
            let on = on_off(value.trim())?;
            return self.device_change(Setting::RemoteLock, |d| set(&mut d.remote_lock, Some(on)));
        }
        let (name, value) = parameter.split_once(" LOCK ")?;
        let on = on_off(value.trim())?;
        let lock = match (name, on) {
            ("PANEL" | "PANEL+V", false) => PanelLock::Off,
            ("PANEL", true) => PanelLock::Panel,
            ("PANEL+V", true) => PanelLock::PanelAndVolume,
            _ => return None,
        };
        self.device_change(Setting::PanelLock, |d| set(&mut d.panel_lock, Some(lock)))
    }

    /// Applies the value of an auto lip sync event, `ON` or `OFF`. Text that
    /// is not a value keeps the last value.
    fn apply_auto_lip_sync(&mut self, value: &str) -> Option<Change> {
        let on = on_off(value.trim())?;
        self.device_change(Setting::AutoLipSync, |d| {
            set(&mut d.auto_lip_sync, Some(on))
        })
    }

    /// Applies an `OP` event about a Dirac Live slot, for example
    /// `DLSSL1NAME Cinema`. The other `OP` events are ignored.
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

    /// Applies an event of zone 2 or 3 that starts with the zone: the power,
    /// `HPFON` and `HPFOFF` for the high pass filter, and `CSST` and `CSMONO`
    /// for stereo or mono.
    fn apply_zone_event(&mut self, zone: Zone, parameter: &str) -> Option<Change> {
        if let Some(value) = parameter.strip_prefix("HPF") {
            let on = on_off(value.trim())?;
            self.zone_change(zone, Setting::HighPassFilter, |z| {
                set(&mut z.high_pass_filter, Some(on))
            })
        } else if let Some(value) = parameter.strip_prefix("CS") {
            let mode = ZoneChannelMode::from_code(value.trim())?;
            self.zone_change(zone, Setting::ChannelMode, |z| {
                set(&mut z.channel_mode, Some(mode))
            })
        } else if parameter.starts_with("QUICK") || parameter.starts_with("SMART") {
            self.apply_quick_select(zone, parameter)
        } else {
            self.set_power(zone, parameter)
        }
    }

    /// Applies an `SI` event: the telnet name of a source, for example `CD`.
    fn apply_source_event(&mut self, zone: Zone, parameter: &str) -> Option<Change> {
        let source = telnet_to_display_source(parameter);
        self.main_source_change(zone, true, |z| set_text(&mut z.source, source))
    }

    /// Applies the HDMI audio of zone 2 or 3, for example `A PCM`.
    fn apply_zone_hdmi_audio(&mut self, zone: Zone, parameter: &str) -> Option<Change> {
        let audio = ZoneHdmiAudio::from_code(parameter.strip_prefix('A')?.trim())?;
        self.zone_change(zone, Setting::ZoneHdmiAudio, |z| {
            set(&mut z.hdmi_audio, Some(audio))
        })
    }

    /// Applies the bass or the treble of zone 2 or 3, for example `BAS 51`.
    /// Other `PS` events of these zones are ignored.
    fn apply_zone_tone(&mut self, zone: Zone, parameter: &str) -> Option<Change> {
        let (name, value) = parameter.split_once(' ')?;
        let level = ToneLevel::from_telnet(value.trim())?;
        match name {
            "BAS" => self.zone_change(zone, Setting::Bass, |z| set(&mut z.bass, Some(level))),
            "TRE" => self.zone_change(zone, Setting::Treble, |z| set(&mut z.treble, Some(level))),
            _ => None,
        }
    }

    /// Applies the level of the left or right channel of zone 2 or 3, for
    /// example `FL 52`. The end of a list and other channels are ignored.
    fn apply_zone_channel_level(&mut self, zone: Zone, parameter: &str) -> Option<Change> {
        let (name, value) = parameter.split_once(' ')?;
        let level = ChannelLevel::from_code(value.trim())?;
        match name {
            "FL" => self.zone_change(zone, Setting::ChannelLevels, |z| {
                set(&mut z.left_level, Some(level))
            }),
            "FR" => self.zone_change(zone, Setting::ChannelLevels, |z| {
                set(&mut z.right_level, Some(level))
            }),
            _ => None,
        }
    }

    /// Applies a volume setting of a zone, for example `LIM 070`. `LIM` is
    /// the volume limit: whole dB above -80 dB, so `070` is -10 dB, and `OFF`
    /// or no value means no limit. `LEV` is `VAR` for a variable volume or the
    /// fixed volume, `PON` the volume at power on, `MLV` the mute level and
    /// `DIS` how the volume is shown.
    fn apply_zone_volume(&mut self, zone: Zone, kind: &str, value: &str) -> Option<Change> {
        let value = value.trim();
        match kind {
            "LIM" => {
                let limit = if matches!(value, "OFF" | "") {
                    None
                } else {
                    Some(Volume::from_whole_db_above_min(value)?)
                };
                self.zone_change(zone, Setting::MaxVolume, |z| set(&mut z.max_volume, limit))
            }
            "LEV" => {
                let mode = VolumeMode::from_telnet(value)?;
                self.zone_change(zone, Setting::VolumeMode, |z| {
                    set(&mut z.volume_mode, Some(mode))
                })
            }
            "PON" => {
                let volume = PowerOnVolume::from_telnet(value)?;
                self.zone_change(zone, Setting::PowerOnVolume, |z| {
                    set(&mut z.power_on_volume, Some(volume))
                })
            }
            "MLV" => {
                let level = MuteLevel::from_code(value)?;
                self.zone_change(zone, Setting::MuteLevel, |z| {
                    set(&mut z.mute_level, Some(level))
                })
            }
            "DIS" => {
                let display = VolumeDisplay::from_code(value)?;
                self.zone_change(zone, Setting::VolumeDisplay, |z| {
                    set(&mut z.volume_display, Some(display))
                })
            }
            _ => None,
        }
    }

    /// Applies an `SS` event that is about the receiver, not a zone: sources,
    /// the category of the sound mode, quick select names, auto lip sync, speaker
    /// levels and the signal.
    fn apply_device_ss(&mut self, parameter: &str) -> Option<Change> {
        if let Some((zone, kind, value)) = zone_volume_event(parameter) {
            return self.apply_zone_volume(zone, kind, value);
        }
        if let Some(changed) = self.device.sources.apply_ss(parameter) {
            return changed.then_some(Change {
                zone: None,
                setting: Setting::Sources,
            });
        }
        if let Some(code) = parameter.strip_prefix("SMG ") {
            let category = SoundModeCategory::from_code(code.trim_end())?;
            return self.zone_change(Zone::Main, Setting::SoundModeCategory, |z| {
                set(&mut z.sound_mode_category, Some(category))
            });
        }
        if let Some(changed) = self.apply_quick_select_name(parameter) {
            return changed.then_some(Change {
                zone: None,
                setting: Setting::QuickSelectNames,
            });
        }
        // Auto lip sync: `ALSSET`, `HOSALS` and `HOS` are read for both brands.
        if let Some(value) = ["ALSSET ", "HOSALS ", "HOS "]
            .iter()
            .find_map(|prefix| parameter.strip_prefix(prefix))
        {
            return self.apply_auto_lip_sync(value);
        }
        if let Some(changed) = self.apply_speaker_level(parameter) {
            return changed.then_some(Change {
                zone: None,
                setting: Setting::SpeakerLevels,
            });
        }
        if parameter.starts_with("TTR") {
            return self.apply_tactile_transducer(parameter);
        }
        if let Some(changed) = self.device.speaker_setup.apply(parameter) {
            return changed.then_some(Change {
                zone: None,
                setting: Setting::SpeakerSetup,
            });
        }
        self.signal_change(|signal| signal.apply_ss(parameter))
    }

    /// Applies an `SS` event of the tactile transducer, for example
    /// `TTRLEV 50`. The line `TTR END` closes the list and is ignored.
    fn apply_tactile_transducer(&mut self, parameter: &str) -> Option<Change> {
        let (name, value) = parameter.split_once(' ')?;
        if value.trim() == "END" {
            return None;
        }
        self.device
            .sound_settings
            .apply(name, value)
            .then_some(Change {
                zone: None,
                setting: Setting::SoundSettings,
            })
    }

    /// Applies an `SS` event like `QS1QSN Quick Select 1`, or the form of
    /// the `SSQSNZMA ?` answer, `QSNZMAQS1 Quick Select 1`. Returns `None` if
    /// the event is not about the name of a quick select, and otherwise true
    /// if the names changed. An empty name means no name.
    fn apply_quick_select_name(&mut self, parameter: &str) -> Option<bool> {
        let (rest, answer_form) = match parameter.strip_prefix("QSNZMAQS") {
            Some(rest) => (rest, true),
            None => (parameter.strip_prefix("QS")?, false),
        };
        let digit = *rest.as_bytes().first()?;
        let index = usize::from(digit.checked_sub(b'1').filter(|n| *n < MAX_QUICK_SELECT)?);
        // The first byte is an ASCII digit, so the index is a char boundary.
        let after = &rest[1..];
        let name = if answer_form {
            if !after.is_empty() && !after.starts_with(' ') {
                return None;
            }
            after
        } else {
            after.strip_prefix("QSN")?
        }
        .trim();
        let names = &mut self.device.quick_select_names;
        let is_new = names.len() <= index;
        if is_new {
            names.resize(index + 1, None);
        }
        Some(set_optional_text(&mut names[index], Some(name).filter(|n| !n.is_empty())) || is_new)
    }

    /// Applies an `SS` event like `LEVFL 505`. Returns `None` if the event
    /// is not the level of a known channel, and otherwise true if the levels
    /// changed.
    fn apply_speaker_level(&mut self, parameter: &str) -> Option<bool> {
        let (code, value) = parameter.strip_prefix("LEV")?.split_once(' ')?;
        let channel = Channel::from_code(code)?;
        let level = ChannelLevel::from_code(value.trim())?;
        Some(set_channel_level(
            &mut self.device.speaker_levels,
            channel,
            level,
        ))
    }

    /// Applies a `CV` event of the main zone like `FL 505`. The line `END`
    /// closes a list: the channels of the list are then the active channels.
    /// An unknown channel or level is ignored.
    fn apply_channel_volume(&mut self, parameter: &str) -> Option<Change> {
        if let Some(value) = parameter.strip_prefix("TTR ") {
            return self.sound_change(|s| s.apply_tactile_channel(value));
        }
        let changed = if parameter.trim() == "END" {
            let list = std::mem::take(&mut self.pending_channels);
            set_active_channels(&mut self.device.active_channels, list)
        } else {
            let (code, value) = parameter.split_once(' ')?;
            let channel = Channel::from_code(code)?;
            let level = ChannelLevel::from_code(value.trim())?;
            if !self.pending_channels.contains(&channel) {
                self.pending_channels.push(channel);
            }
            set_channel_level(&mut self.device.channel_volumes, channel, level)
        };
        changed.then_some(Change {
            zone: Some(Zone::Main),
            setting: Setting::ChannelVolumes,
        })
    }

    fn apply_friendly_name(&mut self, name: &str) -> Option<Change> {
        if name.is_empty() {
            return None;
        }
        self.device_change(Setting::FriendlyName, |d| {
            set_text(&mut d.friendly_name, name)
        })
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

    /// Applies an `MN` event: the setup menu is open (`MEN ON`), all zone
    /// stereo is on (`ZST ON`) or `InstaPrevue` is shown (`PRV ON`).
    fn apply_menu(&mut self, parameter: &str) -> Option<Change> {
        let (setting, text) = if let Some(text) = parameter.strip_prefix("MEN") {
            (Setting::SettingsMenu, text)
        } else if let Some(text) = parameter.strip_prefix("PRV") {
            (Setting::InstaPrevue, text)
        } else {
            (Setting::AllZoneStereo, parameter.strip_prefix("ZST")?)
        };
        let value = match text.get(1..)? {
            "ON" => true,
            "OFF" => false,
            _ => return None,
        };
        self.device_change(setting, |d| {
            let slot = match setting {
                Setting::SettingsMenu => &mut d.settings_menu,
                Setting::InstaPrevue => &mut d.insta_prevue,
                _ => &mut d.all_zone_stereo,
            };
            set(slot, Some(value))
        })
    }

    /// Like `zone_change` for the source. What plays belongs to the source of
    /// the main zone, so a new source there clears it.
    fn source_change(
        &mut self,
        zone: Zone,
        update: impl FnOnce(&mut ZoneState) -> bool,
    ) -> Option<Change> {
        let change = self.zone_change(zone, Setting::Source, update);
        if change.is_some()
            && zone == Zone::Main
            && self.device.now_playing != NowPlaying::default()
        {
            self.device.now_playing = NowPlaying::default();
            let change = Change {
                zone: None,
                setting: Setting::NowPlaying,
            };
            // One is enough, so it stays small if nobody takes them.
            if !self.follow_ups.contains(&change) {
                self.follow_ups.push(change);
            }
        }
        change
    }

    /// Like [`State::source_change`], and also notes if the source of the
    /// main zone is a telnet name (`is_code`). If only that changes the kind
    /// of the source, it is a source change too. For other zones `is_code` is
    /// not used.
    fn main_source_change(
        &mut self,
        zone: Zone,
        is_code: bool,
        update: impl FnOnce(&mut ZoneState) -> bool,
    ) -> Option<Change> {
        if zone != Zone::Main || self.main_source_is_code == is_code {
            return self.source_change(zone, update);
        }
        let before = self.main_playing_kind();
        self.main_source_is_code = is_code;
        self.source_change(zone, update).or_else(|| {
            if self.main_playing_kind() == before {
                None
            } else {
                self.source_change(zone, |_| true)
            }
        })
    }

    fn sound_change(&mut self, update: impl FnOnce(&mut SoundSettings) -> bool) -> Option<Change> {
        update(&mut self.device.sound_settings).then_some(Change {
            zone: None,
            setting: Setting::SoundSettings,
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

/// Splits a volume setting event, for example `VCTZ2SLIM 070`, into the zone,
/// the kind (`LIM`) and the value. `MA` is the main zone, `2S` and `3S` are
/// zone 2 and 3.
fn zone_volume_event(parameter: &str) -> Option<(Zone, &str, &str)> {
    let rest = parameter.strip_prefix("VCTZ")?;
    let zone = match rest.get(..2)? {
        "MA" => Zone::Main,
        "2S" => Zone::Zone2,
        "3S" => Zone::Zone3,
        _ => return None,
    };
    let rest = rest.get(2..)?;
    Some((zone, rest.get(..3)?, rest.get(3..)?))
}

/// `ON` and `OFF`, as telnet events have them.
fn on_off(text: &str) -> Option<bool> {
    match text {
        "ON" => Some(true),
        "OFF" => Some(false),
        _ => None,
    }
}

/// `on` and `off` in any case, as an `AppCommand` answer has them.
fn on_off_answer(text: &str) -> Option<bool> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("on") {
        Some(true)
    } else if text.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    }
}

/// `PS` names of older receivers that end with a colon. The value follows
/// with no space, for example `PSMODE:CINEMA`.
const COLON_NAMES: [&str; 3] = ["MODE", "SB", "FH"];

const fn sources_change() -> Change {
    Change {
        zone: None,
        setting: Setting::Sources,
    }
}

/// Stores the value. Returns true if it was different.
fn replace_if_different<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

const fn speaker_setup_change() -> Change {
    Change {
        zone: None,
        setting: Setting::SpeakerSetup,
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
    use crate::Crossover;

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
    fn balance_and_lfe_of_a_7_1_input_follow_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::SoundSettings);
        assert_eq!(apply(&mut state, "BLL03"), changed());
        assert_eq!(apply(&mut state, "BLL03"), None);
        assert_eq!(state.device().sound_settings.balance, Some(-3));
        assert_eq!(apply(&mut state, "BL000"), changed());
        assert_eq!(state.device().sound_settings.balance, Some(0));
        assert_eq!(apply(&mut state, "Z2BLR01"), None);
        assert_eq!(apply(&mut state, "PSLFL 10"), changed());
        assert_eq!(state.device().sound_settings.lfe_external_input, Some(10));
        // A value that is not known is kept as text.
        assert_eq!(apply(&mut state, "PSLFL 07"), changed());
        assert_eq!(state.device().sound_settings.lfe_external_input, Some(10));
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
    fn old_lowest_volume_code_is_the_lowest_volume() {
        let mut state = State::new(false);
        for (text, zone) in [
            ("MV99", Zone::Main),
            ("MV995", Zone::Main),
            ("Z299", Zone::Zone2),
            ("Z399", Zone::Zone3),
        ] {
            apply(&mut state, text);
            assert_eq!(state.zone(zone).volume, Some(Volume::MIN), "{text}");
            apply(&mut state, &format!("{}50", &text[..2]));
        }
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
    fn zone_volume_limits_follow_events() {
        let mut state = State::new(false);
        let limit = |zone| change(Some(zone), Setting::MaxVolume);
        assert_eq!(apply(&mut state, "SSVCTZ2SLIM 070"), limit(Zone::Zone2));
        assert_eq!(apply(&mut state, "SSVCTZ2SLIM 070"), None);
        assert_eq!(apply(&mut state, "SSVCTZ3SLIM 098"), limit(Zone::Zone3));
        assert_eq!(
            state.zone(Zone::Zone2).max_volume.map(Volume::db),
            Some(-10.0)
        );
        assert_eq!(
            state.zone(Zone::Zone3).max_volume.map(Volume::db),
            Some(18.0)
        );
        assert_eq!(state.zone(Zone::Main).max_volume, None);
        assert_eq!(apply(&mut state, "SSVCTZ2SLIM OFF"), limit(Zone::Zone2));
        assert_eq!(state.zone(Zone::Zone2).max_volume, None);
        // Not a value, or too high.
        for line in [
            "SSVCTZ2SLIM x",
            "SSVCTZ2SLIM 099",
            "SSVCTZ2SLIM 300",
            "SSVCTZ3SXXX VAR",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn zone_bass_treble_and_channel_levels_follow_events() {
        let mut state = State::new(false);
        let tone = |zone, setting| change(Some(zone), setting);
        // The values of zone 2 of a real receiver.
        assert_eq!(
            apply(&mut state, "Z2PSBAS 51"),
            tone(Zone::Zone2, Setting::Bass)
        );
        assert_eq!(apply(&mut state, "Z2PSBAS 51"), None);
        assert_eq!(
            apply(&mut state, "Z2PSTRE 49"),
            tone(Zone::Zone2, Setting::Treble)
        );
        assert_eq!(
            apply(&mut state, "Z3PSBAS 50"),
            tone(Zone::Zone3, Setting::Bass)
        );
        assert_eq!(
            apply(&mut state, "Z2CVFL 52"),
            tone(Zone::Zone2, Setting::ChannelLevels)
        );
        assert_eq!(
            apply(&mut state, "Z2CVFR 50"),
            tone(Zone::Zone2, Setting::ChannelLevels)
        );
        assert_eq!(apply(&mut state, "Z2CVFR 50"), None);
        let zone2 = state.zone(Zone::Zone2);
        assert_eq!(zone2.bass, ToneLevel::from_db(1));
        assert_eq!(zone2.treble, ToneLevel::from_db(-1));
        assert_eq!(zone2.left_level.map(ChannelLevel::db), Some(2.0));
        assert_eq!(zone2.right_level.map(ChannelLevel::db), Some(0.0));
        assert_eq!(state.zone(Zone::Zone3).bass, ToneLevel::from_db(0));
        // The main zone and the receiver values are not changed.
        assert_eq!(state.zone(Zone::Main).bass, None);
        // Other text.
        for line in [
            "Z2PSBAS x",
            "Z2PSDRC OFF",
            "Z2CVSW 50",
            "Z2CVEND",
            "Z2CVFL 99",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn sound_events_of_other_zones_are_not_the_receiver_values() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "PSBAS 52"),
            change(Some(Zone::Main), Setting::Bass)
        );
        // Zone 2 and zone 3 have their own bass and treble. Other sound
        // events of these zones are not kept.
        for line in ["Z2PSBAS 44", "Z3PSBAS 44", "Z2PSTRE 44"] {
            assert!(apply(&mut state, line).is_some(), "{line}");
        }
        assert_eq!(apply(&mut state, "Z2PSRSTR LOW"), None);
        assert_eq!(state.zone(Zone::Main).bass, ToneLevel::from_db(2));
        assert_eq!(state.zone(Zone::Main).treble, None);
        assert_eq!(state.device().sound_settings.audio_restorer, None);
    }

    #[test]
    fn zone_filter_channel_and_hdmi_audio_follow_events() {
        let mut state = State::new(false);
        let changed = |zone, setting| change(Some(zone), setting);
        // The events of zone 2 of a real receiver.
        assert_eq!(
            apply(&mut state, "Z2HPFON"),
            changed(Zone::Zone2, Setting::HighPassFilter)
        );
        assert_eq!(apply(&mut state, "Z2HPFON"), None);
        assert_eq!(
            apply(&mut state, "Z2CSMONO"),
            changed(Zone::Zone2, Setting::ChannelMode)
        );
        assert_eq!(
            apply(&mut state, "Z2HDA PCM"),
            changed(Zone::Zone2, Setting::ZoneHdmiAudio)
        );
        let zone2 = state.zone(Zone::Zone2);
        assert_eq!(zone2.high_pass_filter, Some(true));
        assert_eq!(zone2.channel_mode, Some(ZoneChannelMode::Mono));
        assert_eq!(zone2.hdmi_audio, Some(ZoneHdmiAudio::Pcm));
        assert_eq!(
            apply(&mut state, "Z2HPFOFF"),
            changed(Zone::Zone2, Setting::HighPassFilter)
        );
        assert_eq!(
            apply(&mut state, "Z3CSST"),
            changed(Zone::Zone3, Setting::ChannelMode)
        );
        assert_eq!(
            apply(&mut state, "Z3HDA THR"),
            changed(Zone::Zone3, Setting::ZoneHdmiAudio)
        );
        assert_eq!(state.zone(Zone::Zone2).high_pass_filter, Some(false));
        // The power of the zone still works, and other text changes nothing.
        assert_eq!(
            apply(&mut state, "Z2ON"),
            changed(Zone::Zone2, Setting::Power)
        );
        for line in ["Z2HPFX", "Z2CSXX", "Z2HDA X", "Z2HDB PCM", "Z2HPF"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn zone_volume_settings_follow_events() {
        let mut state = State::new(false);
        let changed = |zone, setting| change(Some(zone), setting);
        // The values of a real receiver.
        assert_eq!(
            apply(&mut state, "SSVCTZMAPON 35"),
            changed(Zone::Main, Setting::PowerOnVolume)
        );
        assert_eq!(apply(&mut state, "SSVCTZMAPON 35"), None);
        assert_eq!(
            apply(&mut state, "SSVCTZ2SPON MUT"),
            changed(Zone::Zone2, Setting::PowerOnVolume)
        );
        assert_eq!(
            apply(&mut state, "SSVCTZMAMLV MUT"),
            changed(Zone::Main, Setting::MuteLevel)
        );
        assert_eq!(
            apply(&mut state, "SSVCTZMAMLV 060"),
            changed(Zone::Main, Setting::MuteLevel)
        );
        assert_eq!(
            apply(&mut state, "SSVCTZMADIS REL"),
            changed(Zone::Main, Setting::VolumeDisplay)
        );
        let main = state.zone(Zone::Main);
        assert_eq!(
            main.power_on_volume,
            Volume::from_db_rounded(-45.0).map(PowerOnVolume::Level)
        );
        assert_eq!(main.mute_level, Some(MuteLevel::Minus20Db));
        assert_eq!(main.volume_display, Some(VolumeDisplay::Relative));
        assert_eq!(
            state.zone(Zone::Zone2).power_on_volume,
            Some(PowerOnVolume::Muted)
        );
        // The main zone can use the last volume.
        assert_eq!(
            apply(&mut state, "SSVCTZMAPON LAS"),
            changed(Zone::Main, Setting::PowerOnVolume)
        );
        assert_eq!(
            state.zone(Zone::Main).power_on_volume,
            Some(PowerOnVolume::Last)
        );
        // Zone 2: variable, and fixed at -70 dB and at -79 dB.
        let mode = |zone| change(Some(zone), Setting::VolumeMode);
        assert_eq!(apply(&mut state, "SSVCTZ2SLEV VAR"), mode(Zone::Zone2));
        assert_eq!(apply(&mut state, "SSVCTZ2SLEV VAR"), None);
        assert_eq!(apply(&mut state, "SSVCTZ2SLEV 10"), mode(Zone::Zone2));
        assert_eq!(
            state.zone(Zone::Zone2).volume_mode,
            Volume::from_db_rounded(-70.0).map(VolumeMode::Fixed)
        );
        assert_eq!(apply(&mut state, "SSVCTZ2SLEV 01"), mode(Zone::Zone2));
        assert_eq!(apply(&mut state, "SSVCTZ2SLEV VAR"), mode(Zone::Zone2));
        assert_eq!(
            state.zone(Zone::Zone2).volume_mode,
            Some(VolumeMode::Variable)
        );
        // A value that is not known keeps the last value.
        for line in [
            "SSVCTZMAMLV 123",
            "SSVCTZMAPON x",
            "SSVCTZMADIS ???",
            "SSVCTZ2SLEV x",
            "SSVCTZXXPON 1",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(
            state.zone(Zone::Main).mute_level,
            Some(MuteLevel::Minus20Db)
        );
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
        // Quick and smart select change the quick select, not the sound mode.
        let quick = |zone| change(Some(zone), Setting::QuickSelect);
        assert_eq!(apply(&mut state, "MSQUICK1"), quick(Zone::Main));
        assert_eq!(apply(&mut state, "MSSMART2"), quick(Zone::Main));
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
            "NSE3Hello",
            "VSXYZ",
            "CVXX 50",
            "TR4 ON",
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
        // Only `on` and `off` are values of the mute, in any case.
        state.apply_field(zone, Field::Muted, Some("OFF"));
        assert_eq!(state.zone(zone).muted, Some(false));
        state.apply_field(zone, Field::Muted, Some("On"));
        assert_eq!(state.zone(zone).muted, Some(true));
        state.apply_field(zone, Field::Muted, Some(" off\n"));
        assert_eq!(state.zone(zone).muted, Some(false));
        for text in ["garbage", ""] {
            state.apply_field(zone, Field::Muted, Some("on"));
            state.apply_field(zone, Field::Muted, Some(text));
            assert_eq!(state.zone(zone).muted, None, "{text:?}");
        }
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
        let main = |setting| change(Some(Zone::Main), setting);
        assert_eq!(apply(&mut state, &message("BAS 53")), main(Setting::Bass));
        assert_eq!(apply(&mut state, &message("TRE 44")), main(Setting::Treble));
        let tone = device(&state);
        assert_eq!(tone.tone_control, Some(true));
        let main_zone = state.zone(Zone::Main).clone();
        assert_eq!(main_zone.bass, ToneLevel::from_db(3));
        assert_eq!(main_zone.treble, Some(ToneLevel::MAIN_MIN));
        // Text that is not a value keeps the last value.
        for text in ["BAS 99", "BAS", "TRE x", "TONE CTRL MAYBE"] {
            assert_eq!(apply(&mut state, &message(text)), None, "{text}");
        }
        assert_eq!(device(&state), tone);
        assert_eq!(state.zone(Zone::Main), &main_zone);
        // An answer has the steps above the lowest level.
        assert_eq!(
            state.apply_field(Zone::Main, Field::Bass, Some("12")),
            main(Setting::Bass)
        );
        assert_eq!(state.zone(Zone::Main).bass, Some(ToneLevel::MAIN_MAX));
        assert_eq!(
            state.apply_field(Zone::Main, Field::ToneControlAdjust, Some("0")),
            setting(Setting::ToneControl)
        );
        assert_eq!(state.device().tone_control, Some(false));
        assert_eq!(
            state.apply_field(Zone::Main, Field::Treble, Some("13")),
            main(Setting::Treble)
        );
        assert_eq!(state.zone(Zone::Main).treble, None);
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
    fn sound_mode_category_follows_events() {
        let mut state = State::new(false);
        let category = || change(Some(Zone::Main), Setting::SoundModeCategory);
        assert_eq!(apply(&mut state, "SSSMG MUS"), category());
        assert_eq!(apply(&mut state, "SSSMG MUS"), None);
        assert_eq!(apply(&mut state, "SSSMG GAM"), category());
        assert_eq!(apply(&mut state, "SSSMG XYZ"), None);
        assert_eq!(
            state.zone(Zone::Main).sound_mode_category,
            Some(SoundModeCategory::Game)
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
    fn main_playing_kind_finds_a_renamed_source_by_its_code() {
        let mut state = State::new(false);
        let main = Zone::Main;
        assert_eq!(state.main_playing_kind(), None);
        state.apply_field(main, Field::InputFunc, Some("TUNER"));
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::Tuner));
        state.apply_field(main, Field::InputFunc, Some("CD"));
        assert_eq!(state.main_playing_kind(), None);
        // Only the main zone counts.
        state.apply_field(Zone::Zone2, Field::InputFunc, Some("TUNER"));
        assert_eq!(state.main_playing_kind(), None);
        state.apply_field(main, Field::InputFunc, Some("Car Radio"));
        assert_eq!(state.main_playing_kind(), None);
        state.apply_renamed_source("HDRADIO", Some("Car Radio"));
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::HdRadio));
        // A name with spaces at the end is found.
        state.apply_field(main, Field::InputFunc, Some("Car Radio "));
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::HdRadio));
    }

    #[test]
    fn main_playing_kind_uses_the_telnet_name_of_an_si_event() {
        let mut state = State::new(false);
        let main = Zone::Main;
        state.apply_renamed_source("TUNER", Some("CD"));
        state.apply_renamed_source("GAME", Some("NET"));
        state.apply_renamed_source("BD", Some("Spotify"));
        // A name from an `AppCommand` answer is a name that the user gave.
        state.apply_field(main, Field::InputFunc, Some("CD"));
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::Tuner));
        state.apply_field(main, Field::InputFunc, Some("NET"));
        assert_eq!(state.main_playing_kind(), None);
        state.apply_field(main, Field::InputFunc, Some("Spotify"));
        assert_eq!(state.main_playing_kind(), None);
        // A telnet name is the source itself.
        assert_eq!(
            apply(&mut state, "SICD"),
            change(Some(main), Setting::Source)
        );
        assert_eq!(state.main_playing_kind(), None);
        apply(&mut state, "SINET");
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::NetAudio));
        // `SPOTIFY` is shown as `Spotify`, the same text as the name of `BD`.
        apply(&mut state, "SISPOTIFY");
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::NetAudio));
        // The same text from an `AppCommand` answer keeps the telnet name.
        assert_eq!(
            state.apply_field(main, Field::InputFunc, Some("Spotify")),
            None
        );
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::NetAudio));
        // Spaces at the end are not compared.
        state.apply_field(main, Field::InputFunc, Some("Spotify "));
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::NetAudio));
        let source = || change(Some(main), Setting::Source);
        assert_eq!(
            state.apply_field(main, Field::InputFunc, Some("CD")),
            source()
        );
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::Tuner));
        // An `SI` event with the same text is a change if the kind changes.
        assert_eq!(apply(&mut state, "SICD"), source());
        assert_eq!(state.main_playing_kind(), None);
        assert_eq!(apply(&mut state, "SICD"), None);
        assert_eq!(state.apply_field(main, Field::InputFunc, Some("CD")), None);
        assert_eq!(state.main_playing_kind(), None);
        // It is no change if the kind stays the same.
        state.apply_field(main, Field::InputFunc, Some("TUNER"));
        assert_eq!(apply(&mut state, "SITUNER"), None);
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::Tuner));
        apply(&mut state, "SISPOTIFY");
        // Another zone does not change where the main source came from.
        state.apply_field(Zone::Zone2, Field::InputFunc, Some("CD"));
        assert_eq!(state.main_playing_kind(), Some(PlayingKind::NetAudio));
    }

    #[test]
    fn appcommand_source_lists_use_the_telnet_codes() {
        let mut state = State::new(false);
        let sources = || change(None, Setting::Sources);
        assert_eq!(state.apply_renamed_source("CBL/SAT", Some("TV")), sources());
        assert_eq!(state.apply_renamed_source("CBL/SAT", Some("TV")), None);
        assert_eq!(state.apply_renamed_source("TV AUDIO", None), sources());
        assert_eq!(state.apply_source_use("CBL/SAT", Some("0")), sources());
        assert_eq!(state.apply_source_use("CBL/SAT", Some("0")), None);
        assert_eq!(state.apply_source_use("Media Player", Some("x")), None);
        assert_eq!(state.apply_source_use("Media Player", None), None);
        let list = &state.device().sources;
        assert_eq!(list.name("SAT/CBL"), Some("TV"));
        assert_eq!(list.get("SAT/CBL").unwrap().in_use, Some(false));
        assert_eq!(list.name("TV"), None);
        // Telnet and HTTP fill the same entry.
        assert_eq!(apply(&mut state, "SSFUNSAT/CBL Sat"), sources());
        assert_eq!(state.device().sources.name("SAT/CBL"), Some("Sat"));
    }

    #[test]
    fn names_follow_events_and_answers() {
        let mut state = State::new(false);
        // Zone names come from the AppCommand answer, one for each zone.
        assert_eq!(
            state.apply_field(Zone::Zone2, Field::ZoneName, Some("ZONE2 ")),
            change(Some(Zone::Zone2), Setting::ZoneName)
        );
        assert_eq!(
            state.apply_field(Zone::Zone2, Field::ZoneName, Some("ZONE2")),
            None
        );
        assert_eq!(state.zone(Zone::Zone2).name.as_deref(), Some("ZONE2"));
        assert_eq!(state.zone(Zone::Main).name, None);
        assert_eq!(
            state.apply_field(Zone::Zone2, Field::ZoneName, None),
            change(Some(Zone::Zone2), Setting::ZoneName)
        );
        assert_eq!(state.zone(Zone::Zone2).name, None);
        // The friendly name comes from telnet and from the answer. An empty
        // name keeps the value.
        assert_eq!(apply(&mut state, "NSFRN "), None);
        assert_eq!(state.device().friendly_name, None);
        assert_eq!(
            apply(&mut state, "NSFRN Denon AVC-A10H   "),
            change(None, Setting::FriendlyName)
        );
        assert_eq!(apply(&mut state, "NSFRN Denon AVC-A10H"), None);
        assert_eq!(apply(&mut state, "NSFRN   "), None);
        assert_eq!(
            state.apply_field(Zone::Main, Field::FriendlyName, None),
            None
        );
        assert_eq!(
            state.apply_field(Zone::Main, Field::FriendlyName, Some("Living room")),
            change(None, Setting::FriendlyName)
        );
        assert_eq!(state.device().friendly_name.as_deref(), Some("Living room"));
        // Other `NS` lines are not names.
        assert_eq!(apply(&mut state, "NSFNT P01"), None);
    }

    #[test]
    fn locks_follow_events() {
        let mut state = State::new(false);
        let lock = |setting| change(None, setting);
        // The events of a real receiver.
        assert_eq!(
            apply(&mut state, "SYREMOTE LOCK OFF"),
            lock(Setting::RemoteLock)
        );
        assert_eq!(apply(&mut state, "SYREMOTE LOCK OFF"), None);
        assert_eq!(
            apply(&mut state, "SYPANEL+V LOCK ON"),
            lock(Setting::PanelLock)
        );
        assert_eq!(state.device().panel_lock, Some(PanelLock::PanelAndVolume));
        assert_eq!(
            apply(&mut state, "SYPANEL LOCK OFF"),
            lock(Setting::PanelLock)
        );
        assert_eq!(state.device().panel_lock, Some(PanelLock::Off));
        assert_eq!(
            apply(&mut state, "SYPANEL LOCK ON"),
            lock(Setting::PanelLock)
        );
        assert_eq!(state.device().panel_lock, Some(PanelLock::Panel));
        assert_eq!(
            apply(&mut state, "SYREMOTE LOCK ON"),
            lock(Setting::RemoteLock)
        );
        assert_eq!(state.device().remote_lock, Some(true));
        // Other text keeps the values.
        for line in [
            "SYMNLOCK OFF",
            "SYREMOTE LOCK ?",
            "SYPANEL LOCK x",
            "SYPANEL+V LOCK",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(state.device().panel_lock, Some(PanelLock::Panel));
    }

    #[test]
    fn auto_lip_sync_follows_events() {
        let mut state = State::new(false);
        let lip_sync = || change(None, Setting::AutoLipSync);
        assert_eq!(apply(&mut state, "OPALSSET ON"), lip_sync());
        assert_eq!(apply(&mut state, "OPALSSET ON"), None);
        assert_eq!(state.device().auto_lip_sync, Some(true));
        assert_eq!(apply(&mut state, "OPALSSET OFF"), lip_sync());
        assert_eq!(state.device().auto_lip_sync, Some(false));
        // Text that is not a value keeps the value. The other lines are not
        // handled.
        assert_eq!(apply(&mut state, "OPALSSET ??"), None);
        assert_eq!(apply(&mut state, "OPALSVAL 000"), None);
        assert_eq!(state.device().auto_lip_sync, Some(false));
        // The `SS` events: `ALSSET`, `HOSALS` and `HOS` are read for both
        // brands.
        assert_eq!(apply(&mut state, "SSALSSET ON"), lip_sync());
        assert_eq!(apply(&mut state, "SSALSSET ON"), None);
        assert_eq!(apply(&mut state, "SSALSDSP OFF"), None);
        assert_eq!(apply(&mut state, "SSHOSALS OFF"), lip_sync());
        assert_eq!(apply(&mut state, "SSHOS ON"), lip_sync());
        assert_eq!(state.device().auto_lip_sync, Some(true));
        assert_eq!(apply(&mut state, "SSHOSIPS ATH"), None);
        assert_eq!(apply(&mut state, "SSHOSALS ??"), None);
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
        let name = format!("OPDLSSL1NAME {:<55}", "Cinema");
        assert_eq!(apply(&mut state, &name), slot(1));
        assert_eq!(apply(&mut state, &name), None);
        assert_eq!(apply(&mut state, "OPDLSSL1IMP ON"), slot(1));
        assert_eq!(apply(&mut state, "OPDLSSL1TYP BC"), slot(1));
        assert_eq!(apply(&mut state, "OPDLSSL2NAME Dirac ART"), slot(2));
        assert_eq!(apply(&mut state, "OPDLSSL2TYP ART"), slot(2));
        assert_eq!(apply(&mut state, "OPDLSSL3IMP OFF"), slot(3));
        let slots = &state.device().dirac_slots;
        assert_eq!(slots[0].name.as_deref(), Some("Cinema"));
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
            state.device().dirac_filter_named("cinema"),
            Some(DiracFilter::Slot1)
        );
        assert_eq!(
            state.device().dirac_filter_named(" Cinema "),
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
    fn tactile_transducer_follows_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::SoundSettings);
        assert_eq!(apply(&mut state, "SSTTR OFF"), changed());
        assert_eq!(apply(&mut state, "SSTTRLEV 50"), changed());
        assert_eq!(apply(&mut state, "SSTTRLPF 080"), changed());
        assert_eq!(apply(&mut state, "SSTTR END"), None);
        assert_eq!(apply(&mut state, "SSTTR OFF"), None);
        let settings = &state.device().sound_settings;
        assert_eq!(settings.tactile_transducer, Some(false));
        assert_eq!(settings.tactile_low_pass, Some(80));
        assert!(settings.other.is_empty());
        // The channel has its own switch and level, separate from `SSTTR`.
        assert_eq!(apply(&mut state, "CVTTR ON"), changed());
        assert_eq!(apply(&mut state, "CVTTR 52"), changed());
        assert_eq!(apply(&mut state, "CVTTR 52"), None);
        assert_eq!(apply(&mut state, "CVTTR ?"), None);
        let settings = &state.device().sound_settings;
        assert_eq!(settings.tactile_channel, Some(true));
        assert_eq!(
            settings.tactile_channel_level,
            ChannelLevel::from_code("52")
        );
        assert_eq!(settings.tactile_level, ChannelLevel::from_code("50"));
        assert_eq!(settings.tactile_transducer, Some(false));
    }

    #[test]
    fn tuner_follows_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::Tuner);
        assert_eq!(apply(&mut state, "TFAN008750"), changed());
        assert_eq!(apply(&mut state, "TFAN008750"), None);
        assert_eq!(apply(&mut state, "TPAN05"), changed());
        assert_eq!(apply(&mut state, "TMANAUTO"), changed());
        assert_eq!(apply(&mut state, "TFANNAMEBBC_R2__"), changed());
        let tuner = &state.device().tuner;
        assert_eq!(
            tuner.frequency.map(|f| f.to_string()).as_deref(),
            Some("87.50 MHz")
        );
        assert_eq!(tuner.preset, Some(5));
        assert_eq!(tuner.mode, Some(crate::TunerMode::Auto));
        assert_eq!(tuner.station_name.as_deref(), Some("BBC R2"));
        for line in ["TFANUP", "TPANDOWN", "TMAN?", "TFAN?"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        // DAB models.
        assert_eq!(apply(&mut state, "DASTN BBC Radio 2"), changed());
        assert_eq!(apply(&mut state, "DASTN BBC Radio 2"), None);
        assert_eq!(apply(&mut state, "DAPTY Pop Music"), changed());
        assert_eq!(apply(&mut state, "DADRC HLF"), changed());
        assert_eq!(apply(&mut state, "DAODR ALP"), changed());
        for line in ["DA ?", "DADRC ?", "DAXYZ 1", "DADRC MAYBE"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        let dab = &state.device().tuner.dab;
        assert_eq!(dab.station_name.as_deref(), Some("BBC Radio 2"));
        assert_eq!(dab.program_type.as_deref(), Some("Pop Music"));
        assert_eq!(dab.drc, Some(crate::DabDrc::Half));
        assert_eq!(
            dab.station_order,
            Some(crate::DabStationOrder::Alphanumeric)
        );
        // Marantz receivers send the name on `TP`.
        assert_eq!(apply(&mut state, "TPANNAMEKEXP____"), changed());
        assert_eq!(state.device().tuner.station_name.as_deref(), Some("KEXP"));
        assert_eq!(state.device().tuner.preset, Some(5));
    }

    #[test]
    fn speaker_preset_follows_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::SpeakerPreset);
        assert_eq!(apply(&mut state, "SPPR 1"), changed());
        assert_eq!(apply(&mut state, "SPPR 1"), None);
        assert_eq!(apply(&mut state, "SPPR 2"), changed());
        assert_eq!(state.device().speaker_preset, Some(SpeakerPreset::Two));
        for line in ["SPPR 3", "SPPR ?", "SPPR TOGGLE", "SPXX 1"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn video_select_follows_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::VideoSelect);
        // The events of a real receiver.
        assert_eq!(apply(&mut state, "SVMPLAY"), changed());
        assert_eq!(apply(&mut state, "SVOFF"), changed());
        assert_eq!(apply(&mut state, "SVOFF"), None);
        assert_eq!(state.device().video_select, Some(false));
        assert_eq!(state.device().video_select_source.as_deref(), Some("MPLAY"));
        assert_eq!(apply(&mut state, "SVON"), changed());
        assert_eq!(apply(&mut state, "SVGAME2"), changed());
        assert_eq!(state.device().video_select, Some(true));
        assert_eq!(state.device().video_select_source.as_deref(), Some("GAME2"));
        // Older Marantz receivers: the video of the selected source.
        assert_eq!(apply(&mut state, "SVSOURCE"), changed());
        assert_eq!(state.device().video_select_source, None);
        assert_eq!(apply(&mut state, "SVSOURCE"), None);
        for line in ["SV?", "SVLAST"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn picture_settings_and_the_digital_codec_follow_events() {
        let mut state = State::new(false);
        let picture = || change(None, Setting::PictureSettings);
        assert_eq!(apply(&mut state, "PVMOV"), picture());
        assert_eq!(apply(&mut state, "PVMOV"), None);
        assert_eq!(apply(&mut state, "PVCN 060"), picture());
        assert_eq!(apply(&mut state, "PVENH 03"), picture());
        let settings = &state.device().picture_settings;
        assert_eq!(settings.mode, Some(crate::PictureMode::Movie));
        assert_eq!(settings.contrast, Some(10));
        assert_eq!(settings.enhancer, Some(3));
        assert_eq!(
            apply(&mut state, "DCPCM"),
            change(None, Setting::DigitalCodec)
        );
        assert_eq!(state.device().digital_codec, Some(DigitalCodec::Pcm));
        for line in ["PV?", "PVXYZ", "DCXXX", "DC?"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn bluetooth_illumination_and_quick_select_follow_events() {
        let mut state = State::new(false);
        let changed = |setting| change(None, setting);
        assert_eq!(apply(&mut state, "BTLEV 80"), changed(Setting::Bluetooth));
        assert_eq!(apply(&mut state, "BTLEV 80"), None);
        assert_eq!(state.device().bluetooth_level, Some(80));
        for line in ["BTLEV 29", "BTLEV 91", "BTLEV UP", "BTLEV ?", "BTLEV 8"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(apply(&mut state, "BTTX OFF"), changed(Setting::Bluetooth));
        assert_eq!(apply(&mut state, "BTTX OFF"), None);
        assert_eq!(apply(&mut state, "BTTX SP"), changed(Setting::Bluetooth));
        assert_eq!(state.device().bluetooth_transmitter, Some(false));
        assert_eq!(
            state.device().bluetooth_output,
            Some(BluetoothOutputMode::BluetoothAndSpeakers)
        );
        assert_eq!(apply(&mut state, "ILB BRI"), changed(Setting::Illumination));
        assert_eq!(state.device().illumination, Some(Illumination::Bright));
        assert_eq!(
            apply(&mut state, "MSQUICK3"),
            change(Some(Zone::Main), Setting::QuickSelect)
        );
        assert_eq!(state.zone(Zone::Main).quick_select, Some(3));
        // Some models have six.
        assert_eq!(
            apply(&mut state, "MSQUICK6"),
            change(Some(Zone::Main), Setting::QuickSelect)
        );
        assert_eq!(state.zone(Zone::Main).quick_select, Some(6));
        assert_eq!(
            apply(&mut state, "MSQUICK3"),
            change(Some(Zone::Main), Setting::QuickSelect)
        );
        assert_eq!(
            apply(&mut state, "MSQUICK0"),
            change(Some(Zone::Main), Setting::QuickSelect)
        );
        assert_eq!(state.zone(Zone::Main).quick_select, Some(0));
        assert_eq!(
            apply(&mut state, "Z2SMART2"),
            change(Some(Zone::Zone2), Setting::QuickSelect)
        );
        assert_eq!(state.zone(Zone::Zone2).quick_select, Some(2));
        for line in [
            "BTTX ?",
            "BTTX MAYBE",
            "ILB ?",
            "MSQUICK7",
            "MSQUICK9",
            "MSQUICK",
            "Z3QUICKX",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn triggers_follow_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::Triggers);
        assert_eq!(apply(&mut state, "TR1 ON"), changed());
        assert_eq!(apply(&mut state, "TR1 ON"), None);
        assert_eq!(apply(&mut state, "TR3 OFF"), changed());
        assert_eq!(state.device().trigger(Trigger::One), Some(true));
        assert_eq!(state.device().trigger(Trigger::Two), None);
        assert_eq!(state.device().trigger(Trigger::Three), Some(false));
        for line in ["TR?", "TR4 ON", "TR0 ON", "TR1 MAYBE", "TRX ON"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn hdmi_output_from_an_answer() {
        let mut state = State::new(false);
        let main = Zone::Main;
        let changed = || change(None, Setting::VideoSettings);
        assert_eq!(
            state.apply_field(main, Field::HdmiOutput, Some("1")),
            changed()
        );
        assert_eq!(state.apply_field(main, Field::HdmiOutput, Some("1")), None);
        assert_eq!(
            state.apply_field(main, Field::HdmiOutput, Some(" 3 ")),
            changed()
        );
        assert_eq!(
            state.device().video_settings.hdmi_output,
            Some(HdmiOutput::Auto)
        );
        // A blank or unknown value is unknown.
        assert_eq!(state.apply_field(main, Field::HdmiOutput, None), changed());
        assert_eq!(state.device().video_settings.hdmi_output, None);
        state.apply_field(main, Field::HdmiOutput, Some("2"));
        state.apply_field(main, Field::HdmiOutput, Some("MONI1"));
        assert_eq!(state.device().video_settings.hdmi_output, None);
    }

    #[test]
    fn video_settings_follow_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::VideoSettings);
        assert_eq!(apply(&mut state, "VSVPMBYP"), changed());
        assert_eq!(apply(&mut state, "VSVPMBYP"), None);
        assert_eq!(apply(&mut state, "VSMONI1"), changed());
        assert_eq!(apply(&mut state, "VSAUDIO AMP"), changed());
        assert_eq!(apply(&mut state, "VSSCHOFF"), changed());
        let video = &state.device().video_settings;
        assert_eq!(
            video.processing_mode,
            Some(crate::VideoProcessingMode::Bypass)
        );
        assert_eq!(video.hdmi_output, Some(HdmiOutput::Hdmi1));
        assert_eq!(video.hdmi_audio, Some(crate::HdmiAudioDecode::Amp));
        assert_eq!(video.hdmi_upscaler, Some(crate::HdmiUpscaler::Off));
        for line in ["VSVPM ?", "VSXYZ", "VSVPMXX"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn sound_settings_follow_events() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::SoundSettings);
        assert_eq!(apply(&mut state, "PSRSTR LOW"), changed());
        assert_eq!(apply(&mut state, "PSRSTR LOW"), None);
        assert_eq!(apply(&mut state, "PSSP:HF"), changed());
        assert_eq!(apply(&mut state, "PSCINEMA EQ.ON"), changed());
        assert_eq!(apply(&mut state, "PSXYZ HEV"), changed());
        let settings = &state.device().sound_settings;
        assert_eq!(settings.audio_restorer, Some(crate::AudioRestorer::Low));
        assert_eq!(
            settings.effect_speakers,
            Some(crate::EffectSpeakerSelection::HeightAndFloor)
        );
        assert_eq!(settings.cinema_eq, Some(true));
        assert_eq!(settings.other["XYZ"], "HEV");
        // Settings that have their own value are not mixed up with these.
        assert_eq!(
            apply(&mut state, "PSLFC ON").map(|c| c.setting),
            Some(Setting::Lfc)
        );
        assert_eq!(state.device().sound_settings.other.len(), 1);
        for line in ["PSRSTR ?", "PSRSTR", "PSSP: ?"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
    }

    #[test]
    fn speaker_levels_follow_events() {
        let mut state = State::new(false);
        let levels = || change(None, Setting::SpeakerLevels);
        assert_eq!(apply(&mut state, "SSLEVFL 505"), levels());
        assert_eq!(apply(&mut state, "SSLEVFL 505"), None);
        assert_eq!(apply(&mut state, "SSLEVSW2 48"), levels());
        assert_eq!(apply(&mut state, "SSLEVFL 495"), levels());
        let db = |channel| {
            let levels = &state.device().speaker_levels;
            levels
                .iter()
                .find(|(c, _)| *c == channel)
                .map(|(_, l)| l.db())
        };
        assert_eq!(db(Channel::FrontLeft), Some(-0.5));
        assert_eq!(db(Channel::Subwoofer2), Some(-2.0));
        assert_eq!(state.device().speaker_levels.len(), 2);
        // The end of the list, and a channel or level that is not known.
        for line in ["SSLEV END", "SSLEVXX 50", "SSLEVFR 99", "SSLEVFR"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(state.device().speaker_levels.len(), 2);
    }

    #[test]
    fn input_mode_follows_events() {
        use crate::{InputMode, InputReport};
        let mut state = State::new(false);
        let mode = || change(None, Setting::InputMode);
        let input = |state: &State| state.device().input_mode;
        assert_eq!(apply(&mut state, "SDHDMI"), mode());
        assert_eq!(apply(&mut state, "SDHDMI"), None);
        assert_eq!(input(&state), Some(InputModeState::Mode(InputMode::Hdmi)));
        assert_eq!(apply(&mut state, "SDAUTO"), mode());
        assert_eq!(input(&state), Some(InputModeState::Mode(InputMode::Auto)));
        assert_eq!(apply(&mut state, "SDXYZ"), None);
        assert_eq!(input(&state), Some(InputModeState::Mode(InputMode::Auto)));
        assert_eq!(apply(&mut state, "SD7.1IN"), mode());
        assert_eq!(
            input(&state),
            Some(InputModeState::Mode(InputMode::SevenOneIn))
        );
        for (text, report) in [
            ("SDARC", InputReport::Arc),
            ("SDEARC", InputReport::Earc),
            ("SDNO", InputReport::NoInput),
        ] {
            assert_eq!(apply(&mut state, text), mode(), "{text}");
            assert_eq!(input(&state), Some(InputModeState::Report(report)));
        }
    }

    #[test]
    fn older_receiver_reports() {
        use crate::{DynamicCompression, ProLogicMode, SurroundBackMode};
        let mut state = State::new(false);
        let device = |setting| change(None, setting);
        // Sound settings of older receivers.
        assert_eq!(
            apply(&mut state, "PSMODE:PRO LOGIC"),
            device(Setting::SoundSettings)
        );
        assert_eq!(
            apply(&mut state, "PSSB:MTRX ON"),
            device(Setting::SoundSettings)
        );
        assert_eq!(apply(&mut state, "PSFH:ON"), device(Setting::SoundSettings));
        assert_eq!(
            apply(&mut state, "PSDCO LOW"),
            device(Setting::SoundSettings)
        );
        assert_eq!(
            apply(&mut state, "PSAFD ON"),
            device(Setting::SoundSettings)
        );
        let sound = &state.device().sound_settings;
        assert_eq!(sound.pro_logic_mode, Some(ProLogicMode::ProLogic));
        assert_eq!(sound.surround_back_mode, Some(SurroundBackMode::MatrixOn));
        assert_eq!(sound.dynamic_compression, Some(DynamicCompression::Low));
        assert_eq!(sound.auto_flag_detect, Some(true));
        assert_eq!(sound.front_height, Some(true));
        assert!(sound.other.is_empty());
        assert_eq!(
            state.device().sound_settings.effect_speakers,
            None,
            "the colon names do not reach the effect speakers"
        );
        // InstaPrevue.
        assert_eq!(apply(&mut state, "MNPRV ON"), device(Setting::InstaPrevue));
        assert_eq!(apply(&mut state, "MNPRV ON"), None);
        assert_eq!(state.device().insta_prevue, Some(true));
        assert_eq!(apply(&mut state, "MNPRV OFF"), device(Setting::InstaPrevue));
        assert_eq!(apply(&mut state, "MNPRV X"), None);
        assert_eq!(state.device().settings_menu, None);
        assert_eq!(state.device().all_zone_stereo, None);
        // REC SELECT.
        assert_eq!(apply(&mut state, "SRCD"), device(Setting::RecordSource));
        assert_eq!(apply(&mut state, "SRCD"), None);
        assert_eq!(state.device().record_source.as_deref(), Some("CD"));
        assert_eq!(apply(&mut state, "SR?"), None);
        assert_eq!(apply(&mut state, "SRSOURCE"), device(Setting::RecordSource));
        assert_eq!(state.device().record_source, None);
    }

    #[test]
    fn network_preset_names_follow_answers() {
        let mut state = State::new(false);
        let names = || change(None, Setting::NetworkPresetNames);
        assert_eq!(apply(&mut state, "NSH01Radio 2              "), names());
        assert_eq!(apply(&mut state, "NSH01Radio 2"), None);
        assert_eq!(
            state.device().network_preset_names,
            [None, Some("Radio 2".to_owned())]
        );
        assert_eq!(apply(&mut state, "NSH00Radio 1"), names());
        assert_eq!(apply(&mut state, "NSH55"), names());
        assert_eq!(state.device().network_preset_names.len(), 56);
        assert_eq!(state.device().network_preset_names[55], None);
        assert_eq!(apply(&mut state, "NSH01    "), names());
        assert_eq!(state.device().network_preset_names[1], None);
        for text in ["NSH56Name", "NSH9", "NSHxxName", "NSH", "NSH?", "NSH 1Name"] {
            assert_eq!(apply(&mut state, text), None, "{text}");
        }
        assert_eq!(state.device().network_preset_names.len(), 56);
    }

    #[test]
    fn hd_radio_status_goes_to_the_tuner() {
        let mut state = State::new(false);
        assert_eq!(
            apply(&mut state, "HDSIG LEV 3"),
            change(None, Setting::Tuner)
        );
        assert_eq!(state.device().tuner.hd.signal_level, Some(3));
        assert_eq!(
            apply(&mut state, "HDARTISTBand"),
            change(None, Setting::NowPlaying)
        );
        assert_eq!(apply(&mut state, "HDARTISTBand"), None);
        assert_eq!(
            apply(&mut state, "TFHD008750"),
            change(None, Setting::Tuner)
        );
        assert_eq!(
            state.device().tuner.hd.frequency.unwrap().hundredths(),
            8750
        );
        assert_eq!(state.device().tuner.frequency, None);
    }

    #[test]
    fn speaker_setup_follows_events() {
        let mut state = State::new(false);
        let setup = || change(None, Setting::SpeakerSetup);
        assert_eq!(apply(&mut state, "SSCFRFRO FUL"), setup());
        assert_eq!(apply(&mut state, "SSCFRFRO FUL"), None);
        assert_eq!(apply(&mut state, "SSCFR END"), None);
        assert_eq!(apply(&mut state, "SSSPCSBK 2SP"), setup());
        assert_eq!(apply(&mut state, "SSAEQLRSFRO 500:500"), setup());
        assert_eq!(apply(&mut state, "SSAEQLRSEND"), None);
        let speakers = &state.device().speaker_setup;
        assert_eq!(speakers.crossovers.len(), 1);
        assert_eq!(speakers.config["SBK-SP"], "2SP");
        assert_eq!(speakers.graphic_eq["FRO"], "500:500");
    }

    #[test]
    fn speaker_pages_join_the_telnet_values() {
        let mut state = State::new(false);
        apply(&mut state, "SSCFRCEN 070");
        let distances = SpeakerTable::parse_distances(
            br#"<Distances><SubwooferType><CurrentNumber>7</CurrentNumber></SubwooferType>
            <List><Speaker index="0">25</Speaker></List></Distances>"#,
        )
        .unwrap();
        let crossovers = SpeakerTable::parse_crossovers(
            br#"<Crossovers><List><Speaker index="1">80</Speaker></List></Crossovers>"#,
        )
        .unwrap();
        let setup = || change(None, Setting::SpeakerSetup);
        assert_eq!(state.apply_speaker_distances(distances.clone()), setup());
        assert_eq!(state.apply_speaker_distances(distances), None);
        assert_eq!(state.apply_speaker_crossovers(&crossovers), setup());
        assert_eq!(state.apply_speaker_crossovers(&crossovers), None);
        let bass = BassSettings::parse(b"<Bass><LPFforLFE>120</LPFforLFE></Bass>").unwrap();
        assert_eq!(state.apply_bass_settings(bass.clone()), setup());
        assert_eq!(state.apply_bass_settings(bass), None);
        let stored = &state.device().speaker_setup;
        assert_eq!(stored.subwoofer_count, Some(2));
        assert_eq!(stored.distances.as_ref().unwrap().entries[0].value, 25);
        assert_eq!(
            stored.crossovers,
            [(SpeakerGroup::Center, Crossover::Hz(80))]
        );
        assert_eq!(stored.bass.as_ref().unwrap().lpf_for_lfe, Some(120));
    }

    #[test]
    fn the_parsers_refuse_a_document_of_another_kind() {
        // The receiver can answer a page with a document of another kind.
        let foreign = b"<Error/>";
        assert!(SpeakerTable::parse_distances(foreign).is_err());
        assert!(SpeakerTable::parse_crossovers(foreign).is_err());
        assert!(SpeakerTable::parse_channel_levels(foreign).is_err());
        assert!(BassSettings::parse(foreign).is_err());
        assert!(SystemSettings::parse(foreign).is_err());
        assert!(NowPlaying::parse_net_audio(foreign).is_err());
        assert!(NowPlaying::parse_hd_radio(foreign).is_err());
    }

    #[test]
    fn channel_levels_of_the_web_page_join_the_telnet_levels() {
        let mut state = State::new(false);
        apply(&mut state, "CVFL 505");
        let table = SpeakerTable::parse_channel_levels(
            br#"<ChannelLevel><List><Speaker index="0" display="3">-20</Speaker>
            <Speaker index="3" display="3">5</Speaker>
            <Speaker index="4" display="1">10</Speaker>
            <Speaker index="5" display="3">3</Speaker></List></ChannelLevel>"#,
        )
        .unwrap();
        let levels = || change(Some(Zone::Main), Setting::ChannelVolumes);
        // The center is new, the front left is the same as telnet reported.
        assert_eq!(state.apply_channel_level_table(&table), levels());
        assert_eq!(state.apply_channel_level_table(&table), None);
        let volumes = &state.device().channel_volumes;
        let level = |channel| {
            volumes
                .iter()
                .find(|(c, _)| *c == channel)
                .map(|(_, l)| l.db())
        };
        // A shown channel is active, also if its level cannot be used.
        assert_eq!(
            state.device().active_channels,
            [Channel::Center, Channel::FrontLeft, Channel::SurroundLeft]
        );
        assert_eq!(level(Channel::Center), Some(-2.0));
        assert_eq!(level(Channel::FrontLeft), Some(0.5));
        // A hidden channel and a level between the steps are left out.
        assert_eq!(level(Channel::FrontRight), None);
        assert_eq!(level(Channel::SurroundLeft), None);
    }

    #[test]
    fn source_details_of_the_web_pages_are_part_of_the_sources() {
        let mut state = State::new(false);
        apply(&mut state, "SSFUNMPLAY Apple TV");
        let assignments = InputAssignment::parse(
            br#"<InputAssign><Source index="5"><Name>Apple TV</Name><HDMI>4</HDMI></Source>
            <SelectionList><HDMI><List><Item index="4">2</Item></List></HDMI></SelectionList>
            </InputAssign>"#,
        )
        .unwrap();
        let names = SourceNames::parse(
            br#"<SourceRename><CharacterMax>16</CharacterMax><Source index="5">
            <Auto>UDP-203</Auto></Source></SourceRename>"#,
        )
        .unwrap();
        let sources = || change(None, Setting::Sources);
        assert_eq!(state.apply_input_assignments(&assignments), sources());
        assert_eq!(state.apply_input_assignments(&assignments), None);
        assert_eq!(state.apply_web_source_names(&names), sources());
        assert_eq!(state.apply_web_source_names(&names), None);
        let source = state.device().sources.get("MPLAY").unwrap();
        assert_eq!(source.hdmi_input.as_deref(), Some("2"));
        assert_eq!(source.auto_name.as_deref(), Some("UDP-203"));
        assert_eq!(source.name.as_deref(), Some("Apple TV"));
    }

    #[test]
    fn active_channels_follow_the_cv_lists() {
        let mut state = State::new(false);
        let levels = || change(Some(Zone::Main), Setting::ChannelVolumes);
        assert_eq!(apply(&mut state, "CVFL 50"), levels());
        assert_eq!(apply(&mut state, "CVC 51"), levels());
        // The list is not active until its end.
        assert_eq!(state.device().active_channels, []);
        assert_eq!(apply(&mut state, "CVEND"), levels());
        assert_eq!(
            state.device().active_channels,
            [Channel::FrontLeft, Channel::Center]
        );
        // The same list again changes nothing, also not the levels.
        for line in ["CVFL 50", "CVC 51"] {
            assert_eq!(apply(&mut state, line), None);
        }
        assert_eq!(apply(&mut state, "CVEND"), None);
        // A shorter list replaces the active channels. The level of the
        // center stays known.
        assert_eq!(apply(&mut state, "CVFL 50"), None);
        assert_eq!(apply(&mut state, "CVEND"), levels());
        assert_eq!(state.device().active_channels, [Channel::FrontLeft]);
        assert_eq!(state.device().channel_volumes.len(), 2);
        // An empty list and a list of unknown channels change nothing.
        assert_eq!(apply(&mut state, "CVEND"), None);
        assert_eq!(apply(&mut state, "CVXX 50"), None);
        assert_eq!(apply(&mut state, "CVEND"), None);
        assert_eq!(state.device().active_channels, [Channel::FrontLeft]);
    }

    #[test]
    fn dirac_art_is_known_from_the_filter_and_its_kind() {
        let mut state = State::new(false);
        assert_eq!(state.device().dirac_art_active(), None);
        apply(&mut state, "PSDIRAC 2");
        // The kind of the slot is not known yet.
        assert_eq!(state.device().dirac_art_active(), None);
        apply(&mut state, "OPDLSSL2TYP ART");
        assert_eq!(state.device().dirac_art_active(), Some(true));
        apply(&mut state, "OPDLSSL1TYP BC");
        apply(&mut state, "PSDIRAC 1");
        assert_eq!(state.device().dirac_art_active(), Some(false));
        apply(&mut state, "PSDIRAC OFF");
        assert_eq!(state.device().dirac_art_active(), Some(false));
    }

    #[test]
    fn settings_availability_are_stored_when_they_change() {
        use crate::{Availability, SettingsGroup};
        let mut state = State::new(false);
        let group = |value: &str| {
            GroupAvailability::parse(
                SettingsGroup::Speakers,
                format!("<listGroupAvailability><Crossovers>{value}</Crossovers></listGroupAvailability>")
                    .as_bytes(),
            )
            .unwrap()
        };
        let changed = || change(None, Setting::SettingsAvailability);
        assert_eq!(state.apply_settings_availability(group("2")), changed());
        assert_eq!(state.apply_settings_availability(group("2")), None);
        assert_eq!(state.apply_settings_availability(group("3")), changed());
        let item = state
            .device()
            .settings_availability
            .item(SettingsGroup::Speakers, "Crossovers")
            .unwrap();
        assert_eq!(item.availability, Availability::Available);
    }

    #[test]
    fn system_settings_are_stored_when_they_change() {
        let mut state = State::new(false);
        let settings = |value: &str| {
            SystemSettings::parse(
                format!("<AdvancedMenu><MenuLock>{value}</MenuLock></AdvancedMenu>").as_bytes(),
            )
            .unwrap()
        };
        let changed = || change(None, Setting::SystemSettings);
        assert_eq!(state.apply_system_settings(settings("2")), changed());
        assert_eq!(state.apply_system_settings(settings("2")), None);
        assert_eq!(state.apply_system_settings(settings("1")), changed());
        assert_eq!(state.device().system_settings.menu_lock, Some(true));
    }

    #[test]
    fn signal_pages_join_the_values_of_telnet() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::Signal);
        let video = VideoSignalPage::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-video-signal.xml"
        ))
        .unwrap();
        assert_eq!(state.apply_video_signal_page(&video), changed());
        assert_eq!(state.apply_video_signal_page(&video), None);
        let signal = &state.device().signal;
        assert_eq!(signal.video.input.as_deref(), Some("4K50"));
        assert_eq!(signal.allm, Some(false));
        // Telnet gives the same value, so nothing changes.
        assert_eq!(apply(&mut state, "SSINFSIGRES I4K50"), None);
        // A page without a signal clears the values.
        let none = VideoSignalPage::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-video-no-signal.xml"
        ))
        .unwrap();
        assert_eq!(state.apply_video_signal_page(&none), changed());
        assert_eq!(state.device().signal.video.input, None);
        let audio = AudioChannelsPage::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-audio-channels.xml"
        ))
        .unwrap();
        assert_eq!(state.apply_audio_channels_page(&audio), changed());
        assert_eq!(state.apply_audio_channels_page(&audio), None);
    }

    #[test]
    fn menu_pages_change_the_state_only_when_a_value_differs() {
        use crate::general_menu::GeneralPage;
        use crate::speaker_extras::{Playback2Ch, XlrAssign};
        use crate::video_menu::VideoMenuPage;
        let mut state = State::new(false);
        let video = || {
            VideoMenuPage::ScreenSaver
                .parse(b"<ScreenSaver>1</ScreenSaver>")
                .unwrap()
        };
        assert_eq!(
            state.apply_video_menu(video()),
            change(None, Setting::VideoMenu)
        );
        assert_eq!(state.apply_video_menu(video()), None);
        let general = || {
            GeneralPage::UsageData
                .parse(b"<UsageData><Send>1</Send></UsageData>")
                .unwrap()
        };
        assert_eq!(
            state.apply_general_settings(general()),
            change(None, Setting::GeneralSettings)
        );
        assert_eq!(state.apply_general_settings(general()), None);
        let playback =
            || Playback2Ch::parse(b"<Playback2Ch><Setting>1</Setting></Playback2Ch>").unwrap();
        assert_eq!(
            state.apply_playback_2ch(playback()),
            change(None, Setting::Playback2Ch)
        );
        assert_eq!(state.apply_playback_2ch(playback()), None);
        let xlr = || {
            XlrAssign::parse(
                b"<SwXLROutputAssign><Speaker index=\"0\"><Value>1</Value></Speaker></SwXLROutputAssign>",
            )
            .unwrap()
        };
        assert_eq!(
            state.apply_xlr_assign(xlr()),
            change(None, Setting::XlrAssign)
        );
        assert_eq!(state.apply_xlr_assign(xlr()), None);
        let levels = SourceLevels {
            analog_db: Some(1),
            digital_db: None,
        };
        assert_eq!(
            state.apply_source_levels(levels),
            change(None, Setting::SourceLevels)
        );
        assert_eq!(state.apply_source_levels(levels), None);
    }

    #[test]
    fn all_zone_stereo_follows_zst_events() {
        let mut state = State::new(false);
        assert_eq!(state.device().all_zone_stereo, None);
        let changed = || change(None, Setting::AllZoneStereo);
        assert_eq!(apply(&mut state, "MNZST ON"), changed());
        assert_eq!(state.device().all_zone_stereo, Some(true));
        assert_eq!(apply(&mut state, "MNZST ON"), None);
        assert_eq!(apply(&mut state, "MNZST OFF"), changed());
        assert_eq!(state.device().all_zone_stereo, Some(false));
        // Text that is not on or off changes nothing.
        assert_eq!(apply(&mut state, "MNZST ?"), None);
        assert_eq!(apply(&mut state, "MNZSTON"), None);
        assert_eq!(state.device().all_zone_stereo, Some(false));
        // The setup menu is the other `MN` event.
        assert_eq!(state.device().settings_menu, None);
        assert_eq!(
            apply(&mut state, "MNMEN ON"),
            change(None, Setting::SettingsMenu)
        );
    }

    #[test]
    fn a_new_source_of_the_main_zone_clears_what_plays() {
        let mut state = State::new(false);
        apply(&mut state, "SIMPLAY");
        apply(&mut state, "NSE1Song");
        assert_eq!(state.device().now_playing.title.as_deref(), Some("Song"));
        // The same source changes nothing.
        assert_eq!(apply(&mut state, "SIMPLAY"), None);
        assert_eq!(state.take_follow_ups(), []);
        // Another zone has its own source.
        apply(&mut state, "Z2CD");
        assert_eq!(state.take_follow_ups(), []);
        assert_eq!(state.device().now_playing.title.as_deref(), Some("Song"));

        assert_eq!(
            apply(&mut state, "SIGAME"),
            change(Some(Zone::Main), Setting::Source)
        );
        assert_eq!(
            state.take_follow_ups(),
            [Change {
                zone: None,
                setting: Setting::NowPlaying
            }]
        );
        assert_eq!(state.take_follow_ups(), []);
        assert_eq!(state.device().now_playing, NowPlaying::default());
        // Nothing plays, so a new source has nothing to clear.
        apply(&mut state, "SIMPLAY");
        assert_eq!(state.take_follow_ups(), []);

        // The same for an `AppCommand` answer.
        apply(&mut state, "NSE1Song");
        state.apply_field(Zone::Main, Field::InputFunc, Some("CD"));
        assert_eq!(state.take_follow_ups().len(), 1);
        assert_eq!(state.device().now_playing, NowPlaying::default());

        // An `SI` event with the same text clears it if the kind changes.
        state.apply_renamed_source("BD", Some("Spotify"));
        state.apply_field(Zone::Main, Field::InputFunc, Some("Spotify"));
        apply(&mut state, "NSE1Song");
        state.take_follow_ups();
        assert_eq!(
            apply(&mut state, "SISPOTIFY"),
            change(Some(Zone::Main), Setting::Source)
        );
        assert_eq!(state.take_follow_ups().len(), 1);
        assert_eq!(state.device().now_playing, NowPlaying::default());
        // The same text from an `AppCommand` answer keeps it.
        apply(&mut state, "NSE1Song");
        state.take_follow_ups();
        state.apply_field(Zone::Main, Field::InputFunc, Some("Spotify"));
        assert_eq!(state.take_follow_ups(), []);
        assert_eq!(state.device().now_playing.title.as_deref(), Some("Song"));
        // An `SI` event that keeps the kind keeps it.
        state.apply_field(Zone::Main, Field::InputFunc, Some("TUNER"));
        apply(&mut state, "NSE1Song");
        state.take_follow_ups();
        assert_eq!(apply(&mut state, "SITUNER"), None);
        assert_eq!(state.take_follow_ups(), []);
        assert_eq!(state.device().now_playing.title.as_deref(), Some("Song"));
    }

    #[test]
    fn follow_ups_that_nobody_takes_do_not_pile_up() {
        let mut state = State::new(false);
        for index in 0..50 {
            apply(&mut state, "NSE1Song");
            apply(
                &mut state,
                if index % 2 == 0 { "SIGAME" } else { "SIMPLAY" },
            );
        }
        assert_eq!(state.take_follow_ups().len(), 1);
    }

    #[test]
    fn now_playing_follows_events_and_pages() {
        let mut state = State::new(false);
        let changed = || change(None, Setting::NowPlaying);
        assert_eq!(apply(&mut state, "NSE0Now Playing"), changed());
        assert_eq!(apply(&mut state, "NSE0Now Playing"), None);
        assert_eq!(apply(&mut state, "NSE1Song"), changed());
        assert_eq!(apply(&mut state, "NSE2Band"), changed());
        assert_eq!(apply(&mut state, "NSE3Bitrate"), None);
        assert_eq!(apply(&mut state, "HDTITLEHd song"), changed());
        assert_eq!(apply(&mut state, "HDA PCM"), None);
        let now = &state.device().now_playing;
        assert_eq!(now.title.as_deref(), Some("Hd song"));
        assert_eq!(now.artist.as_deref(), Some("Band"));
        assert_eq!(now.is_playing(), Some(true));
        // A page replaces everything, an empty one clears it.
        assert_eq!(state.apply_now_playing(NowPlaying::default()), changed());
        assert_eq!(state.apply_now_playing(NowPlaying::default()), None);
        assert!(state.device().now_playing.is_empty());
    }

    #[test]
    fn channel_volumes_follow_events() {
        let mut state = State::new(false);
        let change = || change(Some(Zone::Main), Setting::ChannelVolumes);
        assert_eq!(apply(&mut state, "CVFL 505"), change());
        assert_eq!(apply(&mut state, "CVFL 505"), None);
        assert_eq!(apply(&mut state, "CVSW2 48"), change());
        assert_eq!(apply(&mut state, "CVFL 50"), change());
        let levels = &state.device().channel_volumes;
        assert_eq!(levels.len(), 2);
        assert_eq!(
            levels[0],
            (Channel::FrontLeft, ChannelLevel::from_half_db(0).unwrap())
        );
        assert_eq!(levels[1].1.db(), -2.0);
        // The end of the list makes its channels the active ones, once.
        assert_eq!(apply(&mut state, "CVEND"), change());
        for line in ["CVEND", "CV END", "CVXX 50", "CVFR 99", "CVFR"] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(
            state.device().active_channels,
            [Channel::FrontLeft, Channel::Subwoofer2]
        );
        assert_eq!(state.device().channel_volumes.len(), 2);
        assert_eq!(state.device().speaker_levels.len(), 0);
    }

    #[test]
    fn quick_select_names_follow_events() {
        let mut state = State::new(false);
        let names = || change(None, Setting::QuickSelectNames);
        assert_eq!(apply(&mut state, "SSQS2QSN Movie Night  "), names());
        assert_eq!(apply(&mut state, "SSQS2QSN Movie Night  "), None);
        assert_eq!(apply(&mut state, "SSQS1QSN Quick Select 1  "), names());
        assert_eq!(
            state.device().quick_select_names,
            [
                Some("Quick Select 1".to_owned()),
                Some("Movie Night".to_owned())
            ]
        );
        // An empty name means no name.
        assert_eq!(apply(&mut state, "SSQS2QSN       "), names());
        assert_eq!(state.device().quick_select_names[1], None);
        // Other quick select events are not names.
        for line in [
            "SSQS1VOL ON",
            "SSQS1 END",
            "SSQSN1 ?",
            "SSQS0QSN x",
            "SSQSQSN x",
            // Above the highest quick select.
            "SSQS7QSN x",
            "SSQSNZMAQS7 x",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
        assert_eq!(state.device().quick_select_names.len(), 2);
        // The answer to `SSQSNZMA ?`.
        assert_eq!(apply(&mut state, "SSQSNZMAQS1 Night"), names());
        assert_eq!(apply(&mut state, "SSQSNZMAQS4"), names());
        assert_eq!(
            state.device().quick_select_names,
            [Some("Night".to_owned()), None, None, None]
        );
        for line in [
            "SSQSNZMA END",
            "SSQSNZMAQS0 x",
            "SSQSNZMAQS1x",
            "SSQSNZMA ?",
        ] {
            assert_eq!(apply(&mut state, line), None, "{line}");
        }
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
            "OPINFINS 22111111111111000000",
            "OPINFASP 22121111011000000000000001100000",
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
        assert_eq!(signal_state.channels.input.unwrap().to_string(), "2.0");
        assert_eq!(signal_state.channels.output.unwrap().to_string(), "2.1");
        assert_eq!(apply(&mut state, "OPINFASP 22222222000000000000"), signal);
        assert_eq!(apply(&mut state, "SSINFSIGRES I4K50"), None);
        assert_eq!(apply(&mut state, "SSINFSIGRES I4K60"), signal);
        // The volume limit shares the event and is not mixed up with it.
        assert_eq!(
            apply(&mut state, "SSVCTZMALIM 60"),
            change(Some(Zone::Main), Setting::MaxVolume)
        );
        // The tactile transducer shares the event and is not mixed up with it.
        assert_eq!(
            apply(&mut state, "SSTTR OFF"),
            change(None, Setting::SoundSettings)
        );
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
