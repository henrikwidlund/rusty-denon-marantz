//! Settings that `AppCommand0300.xml` gives. Telnet gives them too, so they
//! are only needed when telnet is not open.
//!
//! Each value is turned into the telnet event with the same meaning, so the
//! state reads it in one place. A value with `control` 0 does not apply to
//! the current sound mode and can be wrong, so it is skipped, as a blank
//! value or a value out of range is.

use super::request::{Cmd, Endpoint, Param, write_body};
use super::response::Response;
use crate::capabilities::DeviceFeatures;
use crate::device_info::ListedQueries;
use crate::error::Error;
use crate::sound_settings::{
    MAX_AUDIO_DELAY, MAX_AURO_STRENGTH, MAX_BASS_SYNC, MAX_DELAY_TIME, MAX_DIALOG_CONTROL,
    MAX_EFFECT_LEVEL, MAX_LFE_LEVEL,
};
use crate::telnet::Query as TelnetQuery;
use crate::tone::ToneLevel;
use crate::zone_settings::MAX_CHANNEL_DB;

/// Which settings request to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingsPart {
    /// Queries that the receiver answers fast.
    Fast,
    /// Queries that take longer.
    Slow,
}

/// How the text of a parameter becomes the value of a telnet event.
#[derive(Debug, Clone, Copy)]
enum Form {
    /// The number of a list entry, with its telnet code.
    Codes(&'static [Code]),
    /// A number up to the maximum, written with this many digits.
    Digits(usize, u16),
    /// A level in dB from 0 down to minus the maximum, written without the
    /// sign.
    MinusDb(usize, u16),
    /// A level in dB from minus to plus the maximum, written as two digits
    /// with 50 for 0 dB.
    Base50(i8),
    /// `1` gives this value, `0` gives no event.
    Flag(&'static str),
}

/// One entry of a list.
#[derive(Debug, Clone, Copy)]
struct Code {
    /// `CmdNo` in `Deviceinfo.xml` and the value in the answer.
    number: &'static str,
    /// Value of the telnet event.
    telnet: &'static str,
    /// `DispName` in `Deviceinfo.xml`. Only tests read it.
    #[cfg_attr(not(test), expect(dead_code, reason = "tests compare it"))]
    name: &'static str,
}

const fn code(number: &'static str, telnet: &'static str, name: &'static str) -> Code {
    Code {
        number,
        telnet,
        name,
    }
}

/// One parameter and the telnet event for it.
#[derive(Debug, Clone, Copy)]
struct ParamRule {
    /// Name of the parameter.
    name: &'static str,
    /// Start of the telnet event, before the value.
    telnet: &'static str,
    /// Starts of the commands that set the value, if they differ from
    /// `telnet`.
    commands: &'static [&'static str],
    /// The telnet query that asks for the same value.
    query: TelnetQuery,
    form: Form,
    /// Zones that the receiver must have for this parameter.
    zones: u8,
    /// Path under `DeviceCapabilities/Setup` of the list in
    /// `Deviceinfo.xml`. Only tests read it.
    #[cfg_attr(not(test), expect(dead_code, reason = "tests compare it"))]
    list: &'static [&'static str],
}

const fn setting(
    name: &'static str,
    telnet: &'static str,
    query: TelnetQuery,
    form: Form,
) -> ParamRule {
    ParamRule {
        name,
        telnet,
        commands: &[],
        query,
        form,
        zones: 1,
        list: &[],
    }
}

const fn listed(
    name: &'static str,
    telnet: &'static str,
    query: TelnetQuery,
    codes: &'static [Code],
    list: &'static [&'static str],
) -> ParamRule {
    ParamRule {
        name,
        telnet,
        commands: &[],
        query,
        form: Form::Codes(codes),
        zones: 1,
        list,
    }
}

impl ParamRule {
    /// The same rule, with commands that do not start with the event.
    const fn set_by(self, commands: &'static [&'static str]) -> Self {
        Self { commands, ..self }
    }

    /// Returns true if a receiver with this many zones and these features is
    /// asked for the parameter.
    const fn is_asked(&self, zones: u8, features: Option<DeviceFeatures>) -> bool {
        self.zones <= zones && self.query.listed_in(features)
    }
}

/// Highest zone tone level in dB.
const TONE: i8 = ToneLevel::MAX_DB;
/// Highest zone channel level in dB.
const LEVEL: i8 = MAX_CHANNEL_DB;

const fn zone(
    name: &'static str,
    telnet: &'static str,
    query: TelnetQuery,
    max: i8,
    zones: u8,
) -> ParamRule {
    ParamRule {
        name,
        telnet,
        commands: &[],
        query,
        form: Form::Base50(max),
        zones,
        list: &[],
    }
}

/// One query with its parameters.
#[derive(Debug, Clone, Copy)]
struct Query {
    name: &'static str,
    part: SettingsPart,
    settings: &'static [ParamRule],
}

/// Most parameters that the receiver accepts in one command.
const PARAMS_PER_CMD: usize = 16;

const OFF_ON: &[Code] = &[code("0", "OFF", "Off"), code("1", "ON", "On")];

/// The input modes of `GetInputSelect`.
const INPUT_MODES: &[Code] = &[
    code("0", "NO", "No Input"),
    code("1", "AUTO", "Auto"),
    code("2", "HDMI", "HDMI"),
    code("3", "DIGITAL", "Digital"),
    code("4", "ANALOG", "Analog"),
    code(EXTERNAL_INPUT, "7.1IN", "7.1CH IN"),
    code("6", "ARC", "ARC"),
    code("7", "EARC", "eARC"),
];

/// The input mode 7.1CH IN, which has its own LFE level.
const EXTERNAL_INPUT: &str = "5";

const SURROUND_PARAMETER: &[ParamRule] = &[
    listed(
        "cinemaeq",
        "PSCINEMA EQ.",
        TelnetQuery::CinemaEq,
        OFF_ON,
        &["SurroundParameter", "CinemaEq"],
    ),
    listed(
        "loudness",
        "PSLOM ",
        TelnetQuery::LoudnessManagement,
        OFF_ON,
        &["SurroundParameter", "Loudness"],
    ),
    listed(
        "dyncomp",
        "PSDRC ",
        TelnetQuery::Drc,
        &[
            code("0", "OFF", "Off"),
            code("1", "LOW", "Low"),
            code("2", "MID", "Medium"),
            code("3", "HI", "High"),
            code("4", "AUTO", "Auto"),
        ],
        &["SurroundParameter", "DynamicCompression"],
    ),
    setting(
        "dialogcontrol",
        "PSDIC ",
        TelnetQuery::DialogControl,
        Form::Digits(2, MAX_DIALOG_CONTROL),
    ),
    setting(
        "lfe",
        "PSLFE ",
        TelnetQuery::LfeLevel,
        Form::MinusDb(2, MAX_LFE_LEVEL),
    ),
    listed(
        "spvirtual",
        "PSSPV ",
        TelnetQuery::SpeakerVirtualizer,
        OFF_ON,
        &["SurroundParameter", "SpeakerVirtualizer"],
    ),
    listed(
        "cspread",
        "PSCES ",
        TelnetQuery::CenterSpread,
        OFF_ON,
        &["SurroundParameter", "CenterSpread"],
    ),
    listed(
        "dtsneux",
        "PSNEURAL ",
        TelnetQuery::NeuralX,
        OFF_ON,
        &["SurroundParameter", "DTSNeuralX"],
    ),
    listed(
        "imax",
        "PSIMAX ",
        TelnetQuery::Imax,
        &[
            code("0", "OFF", "Off"),
            code("1", "ON", "On"),
            code("2", "AUTO", "Auto"),
        ],
        &["SurroundParameter", "IMAX"],
    ),
    listed(
        "imaxaud",
        "PSIMAXAUD ",
        TelnetQuery::ImaxAudio,
        &[code("1", "AUTO", "Auto"), code("2", "MANUAL", "Manual")],
        &["SurroundParameter", "IMAXAudioSettings"],
    ),
    listed(
        "imaxswo",
        "PSIMAXSWO ",
        TelnetQuery::ImaxSubwooferOutput,
        &[code("0", "LFE", "LFE"), code("1", "L+M", "LFE+Main")],
        &["SurroundParameter", "SubwooferOutput"],
    ),
    listed(
        "imaxswm",
        "PSIMAXSWM ",
        TelnetQuery::ImaxSubwooferMode,
        OFF_ON,
        &["SurroundParameter", "SubwooferMode"],
    ),
    listed(
        "auro3dpre",
        "PSAUROPR ",
        TelnetQuery::AuroPreset,
        &[
            code("1", "SMA", "Small"),
            code("2", "MED", "Medium"),
            code("3", "LAR", "Large"),
            code("4", "SPE", "Speech"),
            code("5", "MOV", "Movie"),
        ],
        &["SurroundParameter", "AuroMatic3DPreset"],
    ),
    setting(
        "auro3dstr",
        "PSAUROST ",
        TelnetQuery::AuroStrength,
        Form::Digits(2, MAX_AURO_STRENGTH),
    ),
    listed(
        "auro3dmode",
        "PSAUROMODE ",
        TelnetQuery::AuroMode,
        &[
            code("1", "EXP", "Channel Expansion"),
            code("2", "DRCT", "Direct"),
        ],
        &["SurroundParameter", "Auro3DMode"],
    ),
    setting(
        "delaytime",
        "PSDEL ",
        TelnetQuery::DelayTime,
        Form::Digits(3, MAX_DELAY_TIME),
    ),
    setting(
        "effectlev",
        "PSEFF ",
        TelnetQuery::Effect,
        Form::Digits(2, MAX_EFFECT_LEVEL),
    ),
    listed(
        "roomsize",
        "PSRSZ ",
        TelnetQuery::RoomSize,
        &[
            code("1", "S", "Small"),
            code("2", "MS", "Medium small"),
            code("3", "M", "Medium"),
            code("4", "ML", "Medium large"),
            code("5", "L", "Large"),
        ],
        &["SurroundParameter", "RoomSize"],
    ),
    listed(
        "spsel",
        "PSSP:",
        TelnetQuery::EffectSpeakers,
        &[code("1", "FL", "Floor"), code("2", "HF", "Floor&Height")],
        &["SurroundParameter", "SpeakerSelect"],
    ),
    listed(
        "sw",
        "PSSWR ",
        TelnetQuery::Subwoofer,
        OFF_ON,
        &["SurroundParameter", "Subwoofer"],
    ),
    setting(
        "panorama",
        "PSPAN ",
        TelnetQuery::Panorama,
        Form::Codes(OFF_ON),
    ),
];

const QUERIES: &[Query] = &[
    Query {
        name: "GetSurroundParameter",
        part: SettingsPart::Fast,
        settings: SURROUND_PARAMETER,
    },
    Query {
        name: "GetBTTX",
        part: SettingsPart::Fast,
        settings: &[listed(
            "output",
            "BTTX ",
            TelnetQuery::BluetoothTransmitter,
            &[
                code("1", "BT", "Bluetooth Only"),
                code("2", "SP", "Bluetooth + Speakers"),
            ],
            &["BTTX", "OutputMode"],
        )],
    },
    Query {
        name: "GetEQSetting",
        part: SettingsPart::Fast,
        settings: &[listed(
            "eqenable",
            "PSGEQ ",
            TelnetQuery::GraphicEq,
            OFF_ON,
            &["GraphicEQ"],
        )],
    },
    Query {
        name: "GetBassSync",
        part: SettingsPart::Fast,
        settings: &[setting(
            "basssync",
            "PSBSC ",
            TelnetQuery::BassSync,
            Form::Digits(2, MAX_BASS_SYNC),
        )],
    },
    Query {
        name: "GetSoundMode",
        part: SettingsPart::Fast,
        settings: &[
            setting(
                "movie",
                "SSSMG ",
                TelnetQuery::SoundModeCategory,
                Form::Flag("MOV"),
            ),
            setting(
                "music",
                "SSSMG ",
                TelnetQuery::SoundModeCategory,
                Form::Flag("MUS"),
            ),
            setting(
                "game",
                "SSSMG ",
                TelnetQuery::SoundModeCategory,
                Form::Flag("GAM"),
            ),
            setting(
                "pure",
                "SSSMG ",
                TelnetQuery::SoundModeCategory,
                Form::Flag("PUR"),
            ),
        ],
    },
    Query {
        name: "GetHdmiSetup",
        part: SettingsPart::Fast,
        settings: &[listed(
            "audioout",
            "VSAUDIO ",
            TelnetQuery::HdmiAudioDecode,
            &[code("1", "AMP", "AVR"), code("2", "TV", "TV")],
            &["HdmiSetup", "HdmiAudioOut"],
        )],
    },
    Query {
        name: "GetInputSelect",
        part: SettingsPart::Fast,
        settings: &[listed(
            "inputmode",
            "SD",
            TelnetQuery::AudioInputMode,
            INPUT_MODES,
            &["InputSelect", "InputMode"],
        )],
    },
    Query {
        name: "GetBassTreble",
        part: SettingsPart::Fast,
        settings: &[
            zone("z2bass", "Z2PSBAS ", TelnetQuery::Zone2Bass, TONE, 2),
            zone("z2treble", "Z2PSTRE ", TelnetQuery::Zone2Treble, TONE, 2),
            zone("z3bass", "Z3PSBAS ", TelnetQuery::Zone3Bass, TONE, 3),
            zone("z3treble", "Z3PSTRE ", TelnetQuery::Zone3Treble, TONE, 3),
        ],
    },
    Query {
        name: "GetLRchLevel",
        part: SettingsPart::Fast,
        settings: &[
            zone(
                "z2lchlevel",
                "Z2CVFL ",
                TelnetQuery::Zone2ChannelLevels,
                LEVEL,
                2,
            ),
            zone(
                "z2rchlevel",
                "Z2CVFR ",
                TelnetQuery::Zone2ChannelLevels,
                LEVEL,
                2,
            ),
            zone(
                "z3lchlevel",
                "Z3CVFL ",
                TelnetQuery::Zone3ChannelLevels,
                LEVEL,
                3,
            ),
            zone(
                "z3rchlevel",
                "Z3CVFR ",
                TelnetQuery::Zone3ChannelLevels,
                LEVEL,
                3,
            ),
        ],
    },
    Query {
        name: "GetAudioDelay",
        part: SettingsPart::Slow,
        settings: &[
            setting(
                "audiodelay",
                "PSDELAY ",
                TelnetQuery::AudioDelay,
                Form::Digits(3, MAX_AUDIO_DELAY),
            ),
            listed(
                "autolipsync",
                "SSALSSET ",
                TelnetQuery::AutoLipSync,
                OFF_ON,
                &["AudioDelay", "AutoLipSync"],
            )
            .set_by(&["OPALSSET ", "SSHOSALS "]),
        ],
    },
    Query {
        name: "GetSpeakerPreset",
        part: SettingsPart::Slow,
        settings: &[listed(
            "preset",
            "SPPR ",
            TelnetQuery::SpeakerPreset,
            &[code("1", "1", "Preset 1"), code("2", "2", "Preset 2")],
            &["SpeakerPreset"],
        )],
    },
    Query {
        name: "GetDialogEnhancer",
        part: SettingsPart::Slow,
        settings: &[listed(
            "mode",
            "PSDEH ",
            TelnetQuery::DialogEnhancer,
            &[
                code("0", "OFF", "Off"),
                code("1", "LOW", "Low"),
                code("2", "MED", "Medium"),
                code("3", "HIGH", "High"),
            ],
            &["DialogEnhancer"],
        )],
    },
    Query {
        name: "GetRestorerMode",
        part: SettingsPart::Slow,
        settings: &[listed(
            "mode",
            "PSRSTR ",
            TelnetQuery::AudioRestorer,
            &[
                code("0", "OFF", "Off"),
                code("1", "LOW", "Low"),
                code("2", "MED", "Medium"),
                code("3", "HI", "High"),
            ],
            &["Restorer"],
        )],
    },
];

/// Appends the body of the settings request for `part` to `out`.
///
/// It has the queries that `listed` has. `zones` is the number of zones of
/// the receiver, the main zone included: parameters of other zones are left
/// out. If `features` is known, parameters of a feature that it does not
/// list are left out, as the telnet queries of that feature are. Returns the
/// number of commands, `0` if there is nothing to ask. Then `out` is not
/// changed. Parse the answer with [`Response::parse_at_most`] and this
/// number.
///
/// Nothing is asked if `listed` does not have version 2 of
/// `GetSurroundParameter`. Older receivers list version 1 of
/// `GetSurroundParameter`, `GetHdmiSetup` and `GetAudioDelay`. Their other
/// settings queries may answer differently, so none are asked.
///
/// # Errors
///
/// Fails only if a built-in name is not valid, which tests rule out.
pub fn write_settings_request(
    listed: &ListedQueries,
    zones: u8,
    features: Option<DeviceFeatures>,
    part: SettingsPart,
    out: &mut Vec<u8>,
) -> Result<usize, Error> {
    let queries = || asked_queries(listed).filter(move |query| query.part == part);
    let params: Vec<Vec<Param<'static>>> = queries()
        .map(|query| {
            query
                .settings
                .iter()
                .filter(|setting| setting.is_asked(zones, features))
                .map(|setting| Param {
                    name: setting.name,
                    text: "",
                })
                .collect()
        })
        .collect();
    let mut cmds = Vec::with_capacity(params.len() + 1);
    for (query, params) in queries().zip(&params) {
        for chunk in params.chunks(PARAMS_PER_CMD) {
            cmds.push(Cmd {
                endpoint: Endpoint::AppCommand0300,
                text: None,
                name: Some(query.name),
                params: Some(chunk),
                set: None,
            });
        }
    }
    if cmds.is_empty() {
        return Ok(0);
    }
    write_body(&cmds, out)?;
    Ok(cmds.len())
}

/// The queries that a receiver with this list is asked. None without
/// version 2 of `GetSurroundParameter`, see [`write_settings_request`].
fn asked_queries(listed: &ListedQueries) -> impl Iterator<Item = &'static Query> + Clone {
    QUERIES
        .iter()
        .filter(move |query| listed.surround_parameter && listed.lists(query.name))
}

/// Returns true if [`write_settings_request`] asks for the value of `query`
/// with these arguments in one of the parts. The LFE level is only read
/// when the input mode is not 7.1CH IN, see [`settings_lines`].
#[must_use]
pub fn settings_request_asks(
    query: TelnetQuery,
    listed: &ListedQueries,
    zones: u8,
    features: Option<DeviceFeatures>,
) -> bool {
    asked_queries(listed)
        .flat_map(|entry| entry.settings)
        .any(|setting| setting.query == query && setting.is_asked(zones, features))
}

/// The parts of the settings with the values that a telnet command changes.
///
/// A sound mode command (`MS`) can change several settings, so it gives the
/// fast part. A source command of the main zone (`SI`) and a quick select
/// (`MSQUICK`, `MSSMART`) give both parts: the receiver keeps a sound mode and
/// settings such as the audio delay for each source. Saving a quick select
/// (`MEMORY`) changes nothing. Empty if the command changes none of these
/// settings.
#[must_use]
pub fn settings_parts_of_command(line: &str) -> &'static [SettingsPart] {
    if line.starts_with("MSQUICK") || line.starts_with("MSSMART") {
        if line.contains("MEMORY") {
            return &[];
        }
        return &[SettingsPart::Fast, SettingsPart::Slow];
    }
    if line.starts_with("SI") {
        return &[SettingsPart::Fast, SettingsPart::Slow];
    }
    if line.starts_with("MS") {
        return &[SettingsPart::Fast];
    }
    QUERIES
        .iter()
        .find(|query| {
            query.settings.iter().any(|setting| {
                line.starts_with(setting.telnet)
                    || setting
                        .commands
                        .iter()
                        .any(|command| line.starts_with(command))
            })
        })
        .map_or(&[], |query| match query.part {
            SettingsPart::Fast => &[SettingsPart::Fast],
            SettingsPart::Slow => &[SettingsPart::Slow],
        })
}

/// Calls `on_line` with a telnet event for each value of a settings answer,
/// and with the telnet query that asks for the same value.
///
/// A value is skipped if it is blank, if its `control` is 0, if it is not
/// known, or if it is out of range. The items are matched by their `name`
/// element, so a short answer works too. Each line is a telnet event: parse
/// it with [`Message::parse`](crate::telnet::Message::parse), which never
/// fails for these lines, and apply it with
/// [`State::apply_message`](crate::State::apply_message).
///
/// The LFE level is read only when the same answer gives an input mode other
/// than 7.1CH IN: that input has its own LFE level, and it is not known which
/// one the receiver gives. A busy receiver can leave the input mode out.
pub fn settings_lines(response: &Response<'_>, mut on_line: impl FnMut(&str, TelnetQuery)) {
    let mut external_input = None;
    for item in response.items() {
        if item
            .name()
            .is_some_and(|name| name.trim() == "GetInputSelect")
        {
            item.params(|param, control, text| {
                if param.trim() == "inputmode"
                    && control != Some(0)
                    && INPUT_MODES.iter().any(|code| code.number == text)
                {
                    external_input = Some(text == EXTERNAL_INPUT);
                }
            });
        }
    }
    let mut line = String::new();
    for item in response.items() {
        let Some(name) = item.name() else {
            continue;
        };
        let Some(query) = QUERIES.iter().find(|query| query.name == name.trim()) else {
            continue;
        };
        item.params(|param, control, text| {
            if control == Some(0) || text.is_empty() {
                return;
            }
            let Some(setting) = query
                .settings
                .iter()
                .find(|setting| setting.name == param.trim())
            else {
                return;
            };
            if setting.name == "lfe" && external_input != Some(false) {
                return;
            }
            line.clear();
            line.push_str(setting.telnet);
            if write_value(setting.form, text, &mut line) {
                on_line(&line, setting.query);
            }
        });
    }
}

/// Appends the telnet value for `text` to `out`. Returns false if `text` is
/// not a value of `form`.
fn write_value(form: Form, text: &str, out: &mut String) -> bool {
    use std::fmt::Write as _;
    match form {
        Form::Codes(codes) => {
            let Some(code) = codes.iter().find(|code| code.number == text) else {
                return false;
            };
            out.push_str(code.telnet);
        }
        Form::Digits(width, max) => {
            let Ok(number) = text.parse::<u16>() else {
                return false;
            };
            if number > max {
                return false;
            }
            let _ = write!(out, "{number:0width$}");
        }
        Form::MinusDb(width, max) => {
            let Ok(db) = text.parse::<i16>() else {
                return false;
            };
            if db > 0 || db.unsigned_abs() > max {
                return false;
            }
            let _ = write!(out, "{:0width$}", db.unsigned_abs());
        }
        Form::Base50(max) => {
            let Ok(db) = text.parse::<i8>() else {
                return false;
            };
            if !(-max..=max).contains(&db) {
                return false;
            }
            let _ = write!(out, "{:02}", db + 50);
        }
        Form::Flag(value) => {
            if text != "1" {
                return false;
            }
            out.push_str(value);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(body: &[u8]) -> Vec<String> {
        let response = Response::parse_at_most(body, 32).unwrap();
        let mut lines = Vec::new();
        settings_lines(&response, |line, _| lines.push(line.to_owned()));
        lines
    }

    fn request(listed: &ListedQueries, zones: u8, part: SettingsPart) -> (usize, String) {
        let mut out = Vec::new();
        let count = write_settings_request(listed, zones, None, part, &mut out).unwrap();
        (count, String::from_utf8(out).unwrap())
    }

    fn all_listed() -> ListedQueries {
        ListedQueries::all()
    }

    #[test]
    fn request_splits_parameters_and_commands() {
        let (count, body) = request(&all_listed(), 3, SettingsPart::Fast);
        // 21 surround parameters need two commands.
        assert_eq!(count, 10);
        assert_eq!(body.matches("<tx>").count(), 2);
        assert_eq!(body.matches("<name>GetSurroundParameter</name>").count(), 2);
        assert!(body.contains("<param name=\"z3bass\" />"));
        let (count, body) = request(&all_listed(), 2, SettingsPart::Slow);
        assert_eq!(count, 4);
        assert!(body.contains("<name>GetRestorerMode</name>"));
        assert!(!body.contains("GetBTTX"));
    }

    #[test]
    fn request_leaves_out_zones_and_unlisted_queries() {
        let listed = ListedQueries {
            zone_tone: true,
            surround_parameter: true,
            ..ListedQueries::default()
        };
        let (count, body) = request(&listed, 2, SettingsPart::Fast);
        // Two commands for the surround parameters, one for zone 2.
        assert_eq!(count, 3);
        assert!(body.contains("z2treble") && !body.contains("z3"));
        let mut out = b"x".to_vec();
        let count = write_settings_request(&listed, 1, None, SettingsPart::Slow, &mut out).unwrap();
        assert_eq!((count, out.as_slice()), (0, b"x".as_slice()));
    }

    #[test]
    fn request_leaves_out_parameters_of_features_that_are_not_listed() {
        use crate::capabilities::DeviceFeature;
        let features = DeviceFeatures::default().with(DeviceFeature::CinemaEq);
        let mut out = Vec::new();
        let count = write_settings_request(
            &all_listed(),
            1,
            Some(features),
            SettingsPart::Fast,
            &mut out,
        )
        .unwrap();
        let body = String::from_utf8(out).unwrap();
        // Parameters that need no feature stay, for example the panorama.
        assert!(
            body.contains("\"cinemaeq\"") && body.contains("\"panorama\""),
            "{body}"
        );
        assert!(
            !body.contains("\"imax\"") && !body.contains("\"auro3dmode\""),
            "{body}"
        );
        // A query without parameters is left out, not sent empty.
        assert!(
            !body.contains("GetBTTX") && !body.contains("GetBassSync"),
            "{body}"
        );
        assert!(!body.contains("<list />"), "{body}");
        assert_eq!(count, body.matches("<cmd ").count());
    }

    #[test]
    fn request_asks_follows_the_request() {
        use crate::capabilities::DeviceFeature;
        let listed = ListedQueries {
            zone_tone: true,
            surround_parameter: true,
            ..ListedQueries::default()
        };
        let asked = |query, zones, features| settings_request_asks(query, &listed, zones, features);
        assert!(asked(TelnetQuery::CinemaEq, 1, None));
        assert!(asked(TelnetQuery::Zone2Bass, 2, None));
        assert!(!asked(TelnetQuery::Zone2Bass, 1, None));
        assert!(!asked(TelnetQuery::Zone3Bass, 2, None));
        assert!(!asked(TelnetQuery::DialogEnhancer, 1, None));
        assert!(!asked(TelnetQuery::Volume, 1, None));
        let cinema = DeviceFeatures::default().with(DeviceFeature::CinemaEq);
        assert!(asked(TelnetQuery::CinemaEq, 1, Some(cinema)));
        assert!(!asked(TelnetQuery::Imax, 1, Some(cinema)));
        assert!(asked(TelnetQuery::Panorama, 1, Some(cinema)));
        let old = ListedQueries {
            surround_parameter: false,
            ..all_listed()
        };
        assert!(!settings_request_asks(TelnetQuery::CinemaEq, &old, 3, None));
    }

    #[test]
    fn nothing_is_asked_without_version_2_of_the_surround_parameters() {
        let listed = ListedQueries {
            surround_parameter: false,
            ..all_listed()
        };
        for part in [SettingsPart::Fast, SettingsPart::Slow] {
            let mut out = Vec::new();
            let count = write_settings_request(&listed, 3, None, part, &mut out).unwrap();
            assert_eq!((count, out.len()), (0, 0));
        }
    }

    #[test]
    fn answer_gives_telnet_events() {
        let body = br#"<?xml version="1.0" encoding="utf-8" ?>
<rx>
<cmd><name>GetSurroundParameter</name><list>
<param name="cinemaeq" control="2">1</param>
<param name="dyncomp" control="1">3</param>
<param name="lfe" control="2">-5</param>
<param name="imax" control="0"></param>
<param name="roomsize" control="2">4</param>
<param name="chexpander" control="2">1</param>
</list></cmd>
<cmd><name>GetDialogEnhancer</name><list>
<param name="mode" control="0">0</param>
</list></cmd>
<cmd><name>GetRestorerMode</name><list>
<param name="mode" control="1">3</param>
</list></cmd>
<cmd><name>GetBassTreble</name><list>
<param name="z2bass" control="2">-10</param>
<param name="z3treble" control="2">10</param>
</list></cmd>
<cmd><name>GetAudioDelay</name><list>
<param name="audiodelay" control="2">39</param>
<param name="autolipsync" control="2">1</param>
</list></cmd>
<cmd><name>GetInputSelect</name><list>
<param name="inputmode" control="1">0</param>
</list></cmd>
<error>1</error>
</rx>"#;
        assert_eq!(
            lines(body),
            [
                "PSCINEMA EQ.ON",
                "PSDRC HI",
                "PSLFE 05",
                "PSRSZ ML",
                "PSRSTR HI",
                "Z2PSBAS 40",
                "Z3PSTRE 60",
                "PSDELAY 039",
                "SSALSSET ON",
                "SDNO",
            ]
        );
    }

    /// Answers of a receiver in Matrix and in Multi Ch Stereo mode. Telnet
    /// gave `PSDEL 030` and `PSSP:HF`.
    #[test]
    fn delay_time_and_speaker_select_give_telnet_events() {
        let body = br#"<?xml version="1.0" encoding="utf-8" ?>
<rx>
<cmd><name>GetSurroundParameter</name><list>
<param name="delaytime" control="2">30</param>
</list></cmd>
<cmd><name>GetSurroundParameter</name><list>
<param name="spsel" control="2">2</param>
</list></cmd>
</rx>"#;
        assert_eq!(lines(body), ["PSDEL 030", "PSSP:HF"]);
    }

    /// Answers of a receiver that played Dolby Atmos, to the requests of the
    /// app of the maker: they have more commands than these requests.
    /// Telnet gave the same values. Values with `control` 0 are left out: the dialog enhancer
    /// answered `0` there, but telnet gave `HIGH`.
    #[test]
    fn receiver_answers_give_telnet_events() {
        let fast = include_bytes!("../../tests/data/xml/AVC-A10H-AppCommand0300-settings-fast.xml");
        assert_eq!(
            lines(fast),
            [
                "PSCINEMA EQ.ON",
                "PSDRC OFF",
                "PSLFE 00",
                "PSLOM OFF",
                "BTTX SP",
                "PSBSC 00",
                "SSSMG MOV",
                "VSAUDIO AMP",
                "SDHDMI",
                "Z2PSBAS 50",
                "Z2PSTRE 50",
                "Z3PSBAS 50",
                "Z3PSTRE 50",
                "Z2CVFL 50",
                "Z2CVFR 50",
                "Z3CVFL 50",
                "Z3CVFR 50",
            ]
        );
        let slow = include_bytes!("../../tests/data/xml/AVC-A10H-AppCommand0300-settings-slow.xml");
        assert_eq!(
            lines(slow),
            ["PSDELAY 139", "SSALSSET ON", "SPPR 1", "PSRSTR OFF"]
        );
    }

    #[test]
    fn values_out_of_form_are_skipped() {
        let mut out = String::new();
        for (form, text) in [
            (Form::Codes(OFF_ON), "5"),
            (Form::Digits(2, 6), "-1"),
            (Form::Digits(2, 6), "7"),
            (Form::MinusDb(2, 10), "3"),
            (Form::MinusDb(2, 10), "-11"),
            (Form::Base50(12), "13"),
            (Form::Base50(12), "-13"),
            (Form::Base50(12), "x"),
            (Form::Flag("MOV"), "0"),
        ] {
            assert!(!write_value(form, text, &mut out), "{text}");
        }
    }

    /// Each value of each table changes the state when it is read as a
    /// telnet event.
    #[test]
    fn each_line_changes_the_state() {
        let mut state = crate::State::new(false);
        let mut line = String::new();
        for query in QUERIES {
            for setting in query.settings {
                let texts: &[&str] = match setting.form {
                    Form::Codes(codes) => &codes.iter().map(|code| code.number).collect::<Vec<_>>(),
                    Form::Digits(..) | Form::Base50(_) | Form::Flag(_) => &["1"],
                    Form::MinusDb(..) => &["-1"],
                };
                for text in texts {
                    line.clear();
                    line.push_str(setting.telnet);
                    assert!(write_value(setting.form, text, &mut line));
                    let message = crate::telnet::Message::parse(&line).unwrap();
                    assert!(
                        state.apply_message(&message).is_some(),
                        "{}: {line}",
                        setting.name
                    );
                    // An unknown `PS` value is kept as text and still counts
                    // as a change, so check that none was kept.
                    assert!(
                        state.device().sound_settings.other.is_empty(),
                        "{}: {line}",
                        setting.name
                    );
                }
            }
        }
    }

    #[test]
    fn every_query_is_known_to_the_listed_queries() {
        let all = all_listed();
        for query in QUERIES {
            assert!(all.lists(query.name), "{}", query.name);
            assert!(
                !ListedQueries::default().lists(query.name),
                "{}",
                query.name
            );
        }
    }

    /// The telnet query of each parameter is the one with the longest text
    /// that starts its event, so `PSIMAXAUD` is not taken for `PSIMAX`.
    /// Denon asks for auto lip sync with `OPALS ?` and reports `OPALSSET`.
    /// The state reads `SSALSSET` for both brands, so the lines use it.
    #[test]
    fn each_parameter_has_the_query_of_its_event() {
        use crate::Brand;
        for query in QUERIES {
            for rule in query.settings {
                if rule.telnet == "SSALSSET " {
                    assert_eq!(rule.query, TelnetQuery::AutoLipSync);
                    continue;
                }
                let event = rule.telnet.trim_end();
                let longest = TelnetQuery::ALL
                    .iter()
                    .flat_map(|query| {
                        [Brand::Denon, Brand::Marantz]
                            .into_iter()
                            .filter_map(move |brand| Some((*query, query.text(brand)?)))
                    })
                    .map(|(query, text)| (query, text.trim_end_matches('?').trim_end()))
                    .filter(|(_, prefix)| event.starts_with(prefix))
                    .max_by_key(|(_, prefix)| prefix.len())
                    .map(|(query, _)| query);
                assert_eq!(longest, Some(rule.query), "{}", rule.name);
                let groups = [Brand::Denon, Brand::Marantz].map(|brand| rule.query.group(brand));
                assert!(
                    groups[0].is_none() || groups[1].is_none() || groups[0] == groups[1],
                    "{}",
                    rule.name
                );
            }
        }
    }

    #[test]
    fn commands_give_the_parts_of_their_settings() {
        use crate::sound_settings::SoundSettingValue;
        use crate::telnet::Command;
        use crate::{Brand, Zone};

        for (line, part) in [
            ("PSCINEMA EQ.OFF\r", &[SettingsPart::Fast][..]),
            ("PSDRC AUTO\r", &[SettingsPart::Fast][..]),
            ("Z2PSBAS 47\r", &[SettingsPart::Fast][..]),
            ("Z3CVFR 46\r", &[SettingsPart::Fast][..]),
            ("MSMOVIE\r", &[SettingsPart::Fast][..]),
            ("SIGAME\r", &[SettingsPart::Fast, SettingsPart::Slow][..]),
            ("MSQUICK1\r", &[SettingsPart::Fast, SettingsPart::Slow][..]),
            ("MSSMART2\r", &[SettingsPart::Fast, SettingsPart::Slow][..]),
            ("MSQUICK1 MEMORY\r", &[][..]),
            ("Z2GAME\r", &[][..]),
            ("PSDEL 030\r", &[SettingsPart::Fast][..]),
            ("PSDELAY UP\r", &[SettingsPart::Slow][..]),
            ("PSRSTR HI\r", &[SettingsPart::Slow][..]),
            ("PWON\r", &[][..]),
            ("PSBAS 52\r", &[][..]),
            ("MV50\r", &[][..]),
        ] {
            assert_eq!(settings_parts_of_command(line), part, "{line}");
        }
        // The commands as the library writes them, for both brands.
        for (command, part) in [
            (Command::AutoLipSync(false), SettingsPart::Slow),
            (
                Command::SetSoundSetting(SoundSettingValue::CinemaEq(false)),
                SettingsPart::Fast,
            ),
            (
                Command::SetSoundSetting(SoundSettingValue::AudioDelay(100)),
                SettingsPart::Slow,
            ),
            (Command::SelectSoundMode("MOVIE"), SettingsPart::Fast),
        ] {
            for brand in [Brand::Denon, Brand::Marantz] {
                let mut line = String::new();
                command.write_line(brand, Zone::Main, &mut line).unwrap();
                assert_eq!(settings_parts_of_command(&line), [part], "{line}");
            }
        }
    }

    #[test]
    fn lfe_is_skipped_with_the_external_input() {
        let body = br#"<?xml version="1.0" encoding="utf-8" ?>
<rx>
<cmd><name>GetSurroundParameter</name><list>
<param name="lfe" control="2">0</param>
<param name="cinemaeq" control="2">1</param>
</list></cmd>
<cmd><name>GetInputSelect</name><list>
<param name="inputmode" control="2">5</param>
</list></cmd>
</rx>"#;
        assert_eq!(lines(body), ["PSCINEMA EQ.ON", "SD7.1IN"]);
    }

    #[test]
    fn lfe_is_skipped_without_the_input_mode() {
        let body = br#"<?xml version="1.0" encoding="utf-8" ?>
<rx>
<cmd><name>GetSurroundParameter</name><list>
<param name="lfe" control="2">-5</param>
</list></cmd>
</rx>"#;
        assert_eq!(lines(body), [] as [&str; 0]);
        let body = br#"<?xml version="1.0" encoding="utf-8" ?>
<rx>
<cmd><name>GetSurroundParameter</name><list>
<param name="lfe" control="2">-5</param>
</list></cmd>
<cmd><name>GetInputSelect</name><list>
<param name="inputmode" control="2">0</param>
</list></cmd>
</rx>"#;
        assert_eq!(lines(body), ["PSLFE 05", "SDNO"]);
        // An unknown input mode does not tell that the input is not 7.1CH IN.
        let body = br#"<?xml version="1.0" encoding="utf-8" ?>
<rx>
<cmd><name>GetSurroundParameter</name><list>
<param name="lfe" control="2">-5</param>
</list></cmd>
<cmd><name>GetInputSelect</name><list>
<param name="inputmode" control="2">9</param>
</list></cmd>
</rx>"#;
        assert_eq!(lines(body), [] as [&str; 0]);
    }

    #[test]
    fn names_are_unique_in_each_query() {
        for query in QUERIES {
            for (index, setting) in query.settings.iter().enumerate() {
                assert!(
                    !query.settings[index + 1..]
                        .iter()
                        .any(|other| other.name == setting.name),
                    "{}",
                    setting.name
                );
            }
        }
    }

    fn lists() -> impl Iterator<Item = (&'static [&'static str], &'static [Code])> {
        QUERIES
            .iter()
            .flat_map(|query| query.settings)
            .filter_map(|setting| match setting.form {
                Form::Codes(codes) => Some((setting.list, codes)),
                _ => None,
            })
    }

    /// Every list in the `Deviceinfo.xml` files of the tests has the same
    /// numbers and names as the tables.
    #[test]
    fn codes_match_deviceinfo_lists() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/xml");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().contains("Deviceinfo") {
                continue;
            }
            let body = std::fs::read(&path).unwrap();
            let Some(setup) = crate::xml::Node::new(&body)
                .children()
                .next()
                .and_then(|root| root.node().child("DeviceCapabilities"))
                .and_then(|child| child.node().child("Setup"))
            else {
                continue;
            };
            for (list, codes) in lists() {
                let mut node = setup.node();
                let mut found = true;
                for name in list {
                    match node.child(name) {
                        Some(child) => node = child.node(),
                        None => found = false,
                    }
                    if !found {
                        break;
                    }
                }
                let Some(values) = node.child("List").filter(|_| found) else {
                    continue;
                };
                for value in values.node().children().filter(|child| child.is("Value")) {
                    let text = |name| {
                        value
                            .node()
                            .child(name)
                            .and_then(|child| child.node().text())
                            .map(|text| text.trim().to_owned())
                    };
                    let (Some(number), Some(name)) = (text("CmdNo"), text("DispName")) else {
                        continue;
                    };
                    let code = codes.iter().find(|code| code.number == number);
                    assert!(
                        code.is_some_and(|code| code.name.eq_ignore_ascii_case(&name)),
                        "{}: {list:?} {number} {name}",
                        path.display()
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "{checked}");
    }
}
