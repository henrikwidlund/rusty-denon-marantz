//! Sans-IO protocol for Denon and Marantz AV receivers.

#[macro_use]
mod macros;

mod brand;
mod channel;
mod containment;
mod device_description;
mod device_info;
mod error;
mod sleep;
mod sound_mode;
mod state;
mod text;
mod tone;
mod values;
mod volume;
mod xml;

pub mod appcommand;
pub mod source;
pub mod status_page;
pub mod telnet;
pub mod zone;

pub use brand::Brand;
pub use channel::{Channel, ChannelLevel, Subwoofer};
pub use containment::ContainmentAmount;
pub use device_description::DeviceDescription;
pub use device_info::{DEVICE_INFO_PATH, DeviceInfo, is_avr_x_model};
pub use error::Error;
pub use sleep::Sleep;
pub use sound_mode::SoundModeGroup;
pub use state::{Change, DeviceState, Setting, State, ZoneState};
pub use tone::ToneLevel;
pub use values::{
    AudioRestorer, Auro3dMode, AuroMatic3dPreset, AutoStandby, BluetoothOutputMode, DacFilter,
    DialogEnhancerLevel, DigitalCodec, DimmerMode, DiracFilter, Drc, DynamicVolume, EcoMode,
    EffectSpeakerSelection, HdmiAudioDecode, HdmiOutput, Illumination, Mdax, MultiEq, PictureMode,
    Power, ReferenceLevelOffset, RoomSize, VideoProcessingMode,
};
pub use volume::Volume;
pub use zone::{ReceiverType, Zone};
