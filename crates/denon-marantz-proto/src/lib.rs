//! Sans-IO protocol for Denon and Marantz AV receivers.

#[macro_use]
mod macros;

mod brand;
mod channel;
mod containment;
mod device_description;
mod device_info;
mod error;
mod signal;
mod sleep;
mod sound_mode;
mod sound_settings;
mod state;
mod text;
mod tone;
mod values;
mod volume;
mod xml;

pub mod appcommand;
mod sound_mode_list;
pub mod source;
mod source_list;
pub mod status_page;
pub mod telnet;
pub mod zone;

pub use brand::Brand;
pub use channel::{Channel, ChannelLevel, Subwoofer};
pub use containment::ContainmentAmount;
pub use device_description::DeviceDescription;
pub use device_info::{DEVICE_INFO_PATH, DeviceInfo, is_avr_x_model};
pub use error::Error;
pub use signal::{ChannelLayout, InOut, SignalState};
pub use sleep::Sleep;
pub use sound_mode::SoundModeGroup;
pub use sound_mode_list::{SoundModeCategory, SoundModeEntry, SoundModeList};
pub use sound_settings::{SoundSettingValue, SoundSettings};
pub use source_list::{SourceInfo, SourceList};
pub use state::{Change, DeviceState, DiracSlot, Setting, State, ZoneState};
pub use tone::ToneLevel;
pub use values::{
    AudioRestorer, Auro3dMode, AuroMatic3dPreset, AutoStandby, BluetoothOutputMode, ColorSpace,
    DacFilter, DialogEnhancerLevel, DigitalCodec, DimmerMode, DiracFilter, Drc, DynamicVolume,
    EcoMode, EffectSpeakerSelection, HdmiAudioDecode, HdmiOutput, Illumination, ImaxAudio,
    ImaxMode, ImaxSubwooferOutput, Mdax, MultiEq, MuteLevel, PictureMode, PixelDepth, Power,
    ReferenceLevelOffset, RoomSize, SurroundLevelCompensation, VideoProcessingMode, VolumeDisplay,
};
pub use volume::{PowerOnVolume, Volume, VolumeMode};
pub use zone::{ReceiverType, Zone};
