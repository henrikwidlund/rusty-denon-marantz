//! Sans-IO protocol for Denon and Marantz AV receivers.

#[macro_use]
mod macros;

mod brand;
mod channel;
mod error;
mod sound_mode;
mod values;
mod volume;

pub mod source;
pub mod telnet;
pub mod zone;

pub use brand::Brand;
pub use channel::{Channel, ChannelLevel, Subwoofer};
pub use error::Error;
pub use sound_mode::SoundModeGroup;
pub use values::{
    AudioRestorer, Auro3dMode, AuroMatic3dPreset, AutoStandby, BluetoothOutputMode, DacFilter,
    DialogEnhancerLevel, DigitalCodec, DimmerMode, DiracFilter, Drc, DynamicVolume, EcoMode,
    EffectSpeakerSelection, HdmiAudioDecode, HdmiOutput, Illumination, Mdax, MultiEq, PictureMode,
    ReferenceLevelOffset, RoomSize, VideoProcessingMode,
};
pub use volume::Volume;
pub use zone::{ReceiverType, Zone};
