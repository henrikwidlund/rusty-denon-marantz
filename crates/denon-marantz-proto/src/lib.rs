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
mod input_pages;
mod picture;
mod remote_key;
mod signal;
mod sleep;
mod sound_mode;
mod sound_settings;
mod speaker_setup;
mod speaker_tables;
mod state;
mod text;
mod tone;
mod tuner;
mod values;
mod video;
mod volume;
mod xml;
mod zone_settings;

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
pub use device_info::{DEVICE_INFO_PATH, DeviceInfo, is_avr_x_model};
pub use device_settings::DeviceSettingValue;
pub use error::Error;
pub use input_pages::{
    INPUT_ASSIGN_PATH, InputAssignment, SOURCE_NAMES_PATH, SourceNameEntry, SourceNames,
    source_for_index,
};
pub use picture::{PictureSettingValue, PictureSettings};
pub use remote_key::RemoteKey;
pub use signal::{ChannelLayout, InOut, SignalState};
pub use sleep::Sleep;
pub use sound_mode::SoundModeGroup;
pub use sound_mode_list::{SoundModeCategory, SoundModeEntry, SoundModeList};
pub use sound_settings::{CenterWidthName, SoundSettingValue, SoundSettings, SoundStep};
pub use source_list::{SourceInfo, SourceList};
pub use speaker_setup::{Crossover, SpeakerGroup, SpeakerSetup};
pub use speaker_tables::{
    AJAX_PORT, BASS_PATH, BassSettings, CHANNEL_LEVELS_PATH, CROSSOVERS_PATH, DISTANCES_PATH,
    DistanceUnit, LFE_DISTRIBUTION_OFF, SpeakerTable, TableEntry,
};
pub use state::{Change, DeviceState, DiracSlot, Setting, State, ZoneState};
pub use tone::ToneLevel;
pub use tuner::{TUNER_QUERIES, TunerCommand, TunerFrequency, TunerState};
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
pub use volume::{PowerOnVolume, Volume, VolumeMode};
pub use zone::{ReceiverType, Zone};
pub use zone_settings::ZoneSettingValue;
