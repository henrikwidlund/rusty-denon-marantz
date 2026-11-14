//! Sans-IO protocol for Denon and Marantz AV receivers.

#[macro_use]
mod macros;

mod brand;
mod channel;
mod containment;
mod device_description;
mod device_info;
mod device_settings;
mod error;
mod general_menu;
mod input_pages;
mod now_playing;
mod picture;
mod reading;
mod remote_key;
mod settings_availability;
mod signal;
mod signal_pages;
mod sleep;
mod sound_mode;
mod sound_settings;
mod speaker_extras;
mod speaker_setup;
mod speaker_tables;
mod state;
mod system_settings;
mod text;
mod tone;
mod tuner;
mod values;
mod video;
mod video_menu;
mod volume;
mod xml;
mod zone_settings;

/// Most sources that a list or a page keeps. A receiver has less than 40, and
/// a source that comes after this is ignored.
pub(crate) const MAX_SOURCES: usize = 64;

pub mod appcommand;
mod sound_mode_list;
pub mod source;
mod source_list;
pub mod status_page;
pub mod telnet;
pub mod zone;

pub use brand::Brand;
pub use channel::{Channel, ChannelLevel, ChannelVolumeCommand, Subwoofer};
pub use containment::ContainmentAmount;
pub use device_description::DeviceDescription;
pub use device_info::{DEVICE_INFO_PATH, DeviceInfo, TunerBands, TunerInfo, is_avr_x_model};
pub use device_settings::DeviceSettingValue;
pub use error::Error;
pub use general_menu::{
    GeneralPage, GeneralPart, GeneralSettings, Language, TriggerAssignment, TriggerItem,
    TriggerItemKind,
};
pub use input_pages::{
    INPUT_ASSIGN_PATH, InputAssignment, SOURCE_LEVELS_PATH, SOURCE_NAMES_PATH, SourceLevels,
    SourceNameEntry, SourceNames, source_for_index,
};
pub use now_playing::{
    COVER_ARTWORK_PATH, HD_RADIO_STATUS_PATH, NET_AUDIO_STATUS_PATH, NowPlaying, PlayingKind,
    STATIC_ARTWORK_PATH,
};
pub use picture::{PictureSettingValue, PictureSettings};
pub use reading::{Reading, Sides};
pub use remote_key::RemoteKey;
pub use settings_availability::{
    Availability, GroupAvailability, SettingAvailability, SettingsAvailability, SettingsGroup,
};
pub use signal::{ChannelLayout, InOut, SignalState};
pub use signal_pages::{
    AUDIO_CHANNELS_PATH, AudioChannelsPage, VIDEO_SIGNAL_PATH, VideoSignalPage,
};
pub use sleep::Sleep;
pub use sound_mode::SoundModeGroup;
pub use sound_mode_list::{SoundModeCategory, SoundModeEntry, SoundModeList};
pub use sound_settings::{CenterWidthName, SoundSettingValue, SoundSettings, SoundStep};
pub use source_list::{SourceInfo, SourceList};
pub use speaker_extras::{
    LfeDistribution, PLAYBACK_2CH_PATH, Playback2Ch, SubwooferOutput2Ch, XLR_ASSIGN_PATH,
    XlrAssign, XlrOutput, XlrTarget,
};
pub use speaker_setup::{Crossover, SpeakerGroup, SpeakerSetup};
pub use speaker_tables::{
    BASS_PATH, BassSettings, CHANNEL_LEVELS_PATH, CROSSOVERS_PATH, DISTANCES_PATH, DistanceUnit,
    LFE_DISTRIBUTION_OFF, SpeakerTable, SubwooferOutputMode, TableEntry, WEB_INTERFACE_PORT,
};
pub use state::{Change, DeviceState, DiracSlot, Setting, State, ZoneState};
pub use system_settings::{NetworkControl, SYSTEM_SETTINGS_PATH, SystemSettings};
pub use tone::ToneLevel;
pub use tuner::{
    TUNER_QUERIES, TUNER_STATUS_PATH, TunerCommand, TunerFrequency, TunerPage, TunerState,
};
pub use values::{
    AspectRatio, AudioRestorer, Auro3dMode, AuroMatic3dPreset, AutoStandby, BluetoothOutputMode,
    ColorSpace, DacFilter, DialogEnhancerLevel, DigitalCodec, DimmerMode, DiracFilter, Drc,
    DynamicVolume, EcoMode, EffectSpeakerSelection, FrontSpeakers, HdmiAudioDecode, HdmiOutput,
    Illumination, ImaxAudio, ImaxMode, ImaxSubwooferOutput, InputMode, Mdax, MultiEq, MuteLevel,
    NoiseReduction, PanelLock, PictureMode, PixelDepth, Power, ReferenceLevelOffset, RoomSize,
    SpeakerPreset, SurroundLevelCompensation, Trigger, TunerBand, TunerMode, VideoProcessingMode,
    VideoResolution, VolumeDisplay, ZoneChannelMode, ZoneHdmiAudio,
};
pub use video::{HdmiUpscaler, VideoSettingValue, VideoSettings};
pub use video_menu::{
    HdcpMode, NowPlayingDisplay, OnScreenDisplay, OsdVolume, SignalFormat, SourceSetting, TvFormat,
    VideoMenu, VideoMenuPage, VideoMenuPart,
};
pub use volume::{PowerOnVolume, Volume, VolumeMode};
pub use zone::{ReceiverType, Zone};
pub use zone_settings::{MAX_QUICK_SELECT, ZoneSettingValue};
