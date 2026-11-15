//! Known AppCommand queries and where their values are in the answer.

use super::pattern::{Definition, Field, Pattern, Step};
use super::request::{Cmd, Endpoint, Param};

const fn query(text: &'static str, patterns: &'static [Pattern]) -> Definition {
    Definition {
        cmd: Cmd::get(text),
        patterns,
        blank_is_unknown: true,
    }
}

/// Mute state of all zones.
pub const GET_ALL_ZONE_MUTE_STATUS: Definition = query(
    "GetAllZoneMuteStatus",
    &[Pattern::text(Field::Muted, &[Step::ZoneTag])],
);

/// Power state of all zones.
pub const GET_ALL_ZONE_POWER_STATUS: Definition = query(
    "GetAllZonePowerStatus",
    &[Pattern::text(Field::Power, &[Step::ZoneTag])],
);

/// Selected source of all zones.
pub const GET_ALL_ZONE_SOURCE: Definition = query(
    "GetAllZoneSource",
    &[Pattern::text(
        Field::InputFunc,
        &[Step::ZoneTag, Step::Tag("source")],
    )],
);

/// Volume and volume limit of all zones.
pub const GET_ALL_ZONE_VOLUME: Definition = query(
    "GetAllZoneVolume",
    &[
        Pattern::text(Field::Volume, &[Step::ZoneTag, Step::Tag("volume")]),
        Pattern::text(Field::MaxVolume, &[Step::ZoneTag, Step::Tag("limit")]),
    ],
);

/// Auto standby time of each zone.
pub const GET_AUTO_STANDBY: Definition = query(
    "GetAutoStandby",
    &[Pattern::text(
        Field::AutoStandby,
        &[
            Step::Tag("list"),
            Step::TagWithZoneChild {
                tag: "listvalue",
                child: "zone",
            },
            Step::Tag("value"),
        ],
    )],
);

/// Display dimmer mode.
pub const GET_DIMMER: Definition = query(
    "GetDimmer",
    &[Pattern::text(Field::Dimmer, &[Step::Tag("value")])],
);

/// Eco mode.
pub const GET_ECO: Definition = query(
    "GetECO",
    &[Pattern::text(Field::EcoMode, &[Step::Tag("mode")])],
);

/// Raw surround mode.
pub const GET_SURROUND_MODE_STATUS: Definition = query(
    "GetSurroundModeStatus",
    &[Pattern::text(Field::SoundModeRaw, &[Step::Tag("surround")])],
);

/// Tone control state, bass and treble.
pub const GET_TONE_CONTROL: Definition = Definition {
    cmd: Cmd::get("GetToneControl"),
    patterns: &[
        Pattern::text(Field::ToneControlStatus, &[Step::Tag("status")]),
        Pattern::text(Field::ToneControlAdjust, &[Step::Tag("adjust")]),
        Pattern::text(Field::BassLevel, &[Step::Tag("basslevel")]),
        Pattern::text(Field::Bass, &[Step::Tag("bassvalue")]),
        Pattern::text(Field::TrebleLevel, &[Step::Tag("treblelevel")]),
        Pattern::text(Field::Treble, &[Step::Tag("treblevalue")]),
    ],
    // Blank while tone control works and telnet reports the values.
    blank_is_unknown: false,
};

/// Renamed sources. The answer has `functionrename` with `list` elements.
pub const GET_RENAME_SOURCE: Definition = query("GetRenameSource", &[]);

/// Deleted sources. The answer has `functiondelete` with `list` elements.
pub const GET_DELETED_SOURCE: Definition = query("GetDeletedSource", &[]);

/// Names of all zones.
pub const GET_ZONE_NAME: Definition = query(
    "GetZoneName",
    &[Pattern::text(Field::ZoneName, &[Step::ZoneTag])],
);

/// Friendly name.
pub const GET_FRIENDLY_NAME: Definition = query(
    "GetFriendlyName",
    &[Pattern::text(
        Field::FriendlyName,
        &[Step::Tag("friendlyname")],
    )],
);

// Attribute that tells if an Audyssey parameter can be changed.
const CONTROL: &str = "control";

// Names of the Audyssey parameters. They are in the request and in the answer.
const DYNAMIC_EQ: &str = "dynamiceq";
const REF_LEV_OFFSET: &str = "reflevoffset";
const DYNAMIC_VOL: &str = "dynamicvol";
const MULT_EQ: &str = "multeq";

/// Steps to the `param` element with this name in the answer.
const fn param_steps(name: &'static str) -> [Step; 2] {
    [
        Step::Tag("list"),
        Step::TagWithAttribute {
            tag: "param",
            name: "name",
            value: name,
        },
    ]
}

const MULTEQ_STEPS: &[Step] = &param_steps(MULT_EQ);
const DYNAMICEQ_STEPS: &[Step] = &param_steps(DYNAMIC_EQ);
const REFLEVOFFSET_STEPS: &[Step] = &param_steps(REF_LEV_OFFSET);
const DYNAMICVOL_STEPS: &[Step] = &param_steps(DYNAMIC_VOL);

/// A parameter without a value, as the query has it.
const fn blank_param(name: &'static str) -> Param<'static> {
    Param { name, text: "" }
}

const AUDYSSEY_PARAMS: &[Param<'static>] = &[
    blank_param(DYNAMIC_EQ),
    blank_param(REF_LEV_OFFSET),
    blank_param(DYNAMIC_VOL),
    blank_param(MULT_EQ),
];

const VIDEO_SIGNAL_IN: &str = "hdmisigin";
const VIDEO_SIGNAL_OUT: &str = "hdmisigout";
const AUDIO_SIGNAL: &str = "signal";
const AUDIO_SOUND: &str = "sound";
const SAMPLING_RATE: &str = "fs";

const VIDEO_SIGNAL_IN_STEPS: &[Step] = &param_steps(VIDEO_SIGNAL_IN);
const VIDEO_SIGNAL_OUT_STEPS: &[Step] = &param_steps(VIDEO_SIGNAL_OUT);
const AUDIO_SIGNAL_STEPS: &[Step] = &param_steps(AUDIO_SIGNAL);
const AUDIO_SOUND_STEPS: &[Step] = &param_steps(AUDIO_SOUND);
const SAMPLING_RATE_STEPS: &[Step] = &param_steps(SAMPLING_RATE);

const VIDEO_INFO_PARAMS: &[Param<'static>] = &[
    blank_param("videooutput"),
    blank_param(VIDEO_SIGNAL_IN),
    blank_param(VIDEO_SIGNAL_OUT),
];

const AUDIO_INFO_PARAMS: &[Param<'static>] = &[
    blank_param("inputmode"),
    blank_param("output"),
    blank_param(AUDIO_SIGNAL),
    blank_param(AUDIO_SOUND),
    blank_param(SAMPLING_RATE),
];

/// The video signal: the resolution that the receiver gets and sends. This is
/// a query for `AppCommand0300.xml`.
pub const GET_VIDEO_INFO: Definition = Definition {
    cmd: Cmd {
        endpoint: Endpoint::AppCommand0300,
        text: None,
        name: Some("GetVideoInfo"),
        params: Some(VIDEO_INFO_PARAMS),
        set: None,
    },
    blank_is_unknown: true,
    patterns: &[
        Pattern::text(Field::VideoSignalIn, VIDEO_SIGNAL_IN_STEPS),
        Pattern::text(Field::VideoSignalOut, VIDEO_SIGNAL_OUT_STEPS),
    ],
};

/// The audio signal: its kind, the kind of sound that is played and the
/// sampling rate. This is a query for `AppCommand0300.xml`.
pub const GET_AUDIO_INFO: Definition = Definition {
    cmd: Cmd {
        endpoint: Endpoint::AppCommand0300,
        text: None,
        name: Some("GetAudioInfo"),
        params: Some(AUDIO_INFO_PARAMS),
        set: None,
    },
    blank_is_unknown: true,
    patterns: &[
        Pattern::text(Field::AudioSignal, AUDIO_SIGNAL_STEPS),
        Pattern::text(Field::AudioSound, AUDIO_SOUND_STEPS),
        Pattern::text(Field::SamplingRate, SAMPLING_RATE_STEPS),
    ],
};

const VIDEO_OUT: &str = "videoout";

/// The HDMI monitor output. This is a query for `AppCommand0300.xml`. Only
/// receivers whose `Deviceinfo.xml` lists it answer it.
pub const GET_OUTPUT_SETTINGS: Definition = Definition {
    cmd: Cmd {
        endpoint: Endpoint::AppCommand0300,
        text: None,
        name: Some("GetOutputSettings"),
        params: Some(&[blank_param(VIDEO_OUT)]),
        set: None,
    },
    blank_is_unknown: true,
    patterns: &[Pattern::text(Field::HdmiOutput, &param_steps(VIDEO_OUT))],
};

/// The channels of the audio signal that the receiver gets, as a grid. Read
/// the answer with [`Item::params`](super::Item::params) and
/// `ChannelLayout::from_grid`. This is a query for `AppCommand0300.xml`.
pub const GET_INPUT_SIGNAL: Definition = Definition {
    cmd: Cmd {
        endpoint: Endpoint::AppCommand0300,
        text: None,
        name: Some("GetInputSignal"),
        params: Some(&[blank_param("inputsigall")]),
        set: None,
    },
    blank_is_unknown: true,
    patterns: &[],
};

/// The speakers that play, as a grid. Read the answer as for
/// [`GET_INPUT_SIGNAL`]. This is a query for `AppCommand0300.xml`.
pub const GET_ACTIVE_SPEAKER: Definition = Definition {
    cmd: Cmd {
        endpoint: Endpoint::AppCommand0300,
        text: None,
        name: Some("GetActiveSpeaker"),
        params: Some(&[blank_param("activespall")]),
        set: None,
    },
    blank_is_unknown: true,
    patterns: &[],
};

/// Audyssey settings. This is a query for `AppCommand0300.xml`.
pub const GET_AUDYSSEY: Definition = Definition {
    cmd: Cmd {
        endpoint: Endpoint::AppCommand0300,
        text: None,
        name: Some("GetAudyssey"),
        params: Some(AUDYSSEY_PARAMS),
        set: None,
    },
    blank_is_unknown: true,
    patterns: &[
        Pattern::text(Field::MultEq, MULTEQ_STEPS),
        Pattern::attribute(Field::MultEqControl, MULTEQ_STEPS, CONTROL),
        Pattern::text(Field::DynamicEq, DYNAMICEQ_STEPS),
        Pattern::attribute(Field::DynamicEqControl, DYNAMICEQ_STEPS, CONTROL),
        Pattern::text(Field::RefLevOffset, REFLEVOFFSET_STEPS),
        Pattern::attribute(Field::RefLevOffsetControl, REFLEVOFFSET_STEPS, CONTROL),
        Pattern::text(Field::DynamicVol, DYNAMICVOL_STEPS),
        Pattern::attribute(Field::DynamicVolControl, DYNAMICVOL_STEPS, CONTROL),
    ],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appcommand::write_body;

    #[test]
    fn audyssey_query_body() {
        let mut body = Vec::new();
        write_body(&[GET_AUDYSSEY.cmd], &mut body).unwrap();
        assert_eq!(
            String::from_utf8(body).unwrap(),
            "<?xml version='1.0' encoding='utf-8'?>\n<tx><cmd id=\"3\"><name>GetAudyssey</name><list>\
             <param name=\"dynamiceq\" /><param name=\"reflevoffset\" /><param name=\"dynamicvol\" />\
             <param name=\"multeq\" /></list></cmd></tx>"
        );
    }

    #[test]
    fn audyssey_answer_names_match_the_query() {
        // Each parameter in the query is read from the answer under the same name.
        for param in GET_AUDYSSEY.cmd.params.unwrap() {
            let found = GET_AUDYSSEY.patterns.iter().any(|pattern| {
                pattern
                    .steps
                    .iter()
                    .any(|step| matches!(step, Step::TagWithAttribute { value, .. } if *value == param.name))
            });
            assert!(found, "{}", param.name);
        }
    }

    #[test]
    fn queries_are_for_the_standard_endpoint() {
        for definition in [GET_DIMMER, GET_ECO, GET_TONE_CONTROL, GET_ALL_ZONE_VOLUME] {
            assert_eq!(definition.cmd.endpoint, Endpoint::AppCommand);
        }
        assert_eq!(GET_AUDYSSEY.cmd.endpoint, Endpoint::AppCommand0300);
    }
}
