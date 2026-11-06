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

/// Friendly name. The answer has a `friendlyname` element.
pub const GET_FRIENDLY_NAME: Definition = query("GetFriendlyName", &[]);

const MULTEQ_STEPS: &[Step] = &[
    Step::Tag("list"),
    Step::TagWithAttribute {
        tag: "param",
        name: "name",
        value: "multeq",
    },
];
const DYNAMICEQ_STEPS: &[Step] = &[
    Step::Tag("list"),
    Step::TagWithAttribute {
        tag: "param",
        name: "name",
        value: "dynamiceq",
    },
];
const REFLEVOFFSET_STEPS: &[Step] = &[
    Step::Tag("list"),
    Step::TagWithAttribute {
        tag: "param",
        name: "name",
        value: "reflevoffset",
    },
];
const DYNAMICVOL_STEPS: &[Step] = &[
    Step::Tag("list"),
    Step::TagWithAttribute {
        tag: "param",
        name: "name",
        value: "dynamicvol",
    },
];

const AUDYSSEY_PARAMS: &[Param<'static>] = &[
    Param {
        name: "dynamiceq",
        text: "",
    },
    Param {
        name: "reflevoffset",
        text: "",
    },
    Param {
        name: "dynamicvol",
        text: "",
    },
    Param {
        name: "multeq",
        text: "",
    },
];

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
        Pattern::attribute(Field::MultEqControl, MULTEQ_STEPS, "control"),
        Pattern::text(Field::DynamicEq, DYNAMICEQ_STEPS),
        Pattern::attribute(Field::DynamicEqControl, DYNAMICEQ_STEPS, "control"),
        Pattern::text(Field::RefLevOffset, REFLEVOFFSET_STEPS),
        Pattern::attribute(Field::RefLevOffsetControl, REFLEVOFFSET_STEPS, "control"),
        Pattern::text(Field::DynamicVol, DYNAMICVOL_STEPS),
        Pattern::attribute(Field::DynamicVolControl, DYNAMICVOL_STEPS, "control"),
    ],
};
