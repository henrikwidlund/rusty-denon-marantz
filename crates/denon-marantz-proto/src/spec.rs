//! What the protocol specs of a receiver generation have.

use crate::brand::Brand;
use crate::network_audio::MAX_NETWORK_PRESET;
use crate::telnet::{Query, query_for_command, query_for_text};

/// The protocol specs that a receiver follows, from `CommApiVers` in
/// `Deviceinfo.xml` and the brand.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpecGeneration {
    /// Denon 0210: the specs of 2012 and 2013.
    Denon2013,
    /// Marantz 0210: the spec of 2013.
    Marantz2013,
    /// Denon 0250 and 0300: the spec of 2014 and 2015.
    Denon2015,
    /// 0301 and higher: the current specs. These receivers have HEOS.
    Current,
    /// No spec is known for the version, or there is no version. Marantz
    /// 0250 and 0300 have no known spec.
    Unknown,
}

/// A command group that only the specs of some generations have.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpecFeature {
    /// `PSMODE`: the mode of Dolby `PLII` and DTS Neo.
    ProLogicMode,
    /// `PSSB`: the surround back mode.
    SurroundBackMode,
    /// `PSFH`: the front height output.
    FrontHeight,
    /// `PSDCO`: dynamic compression. Not the same as
    /// [`DeviceFeature::DynamicCompression`](crate::DeviceFeature::DynamicCompression),
    /// which is `PSDRC`.
    DynamicCompression,
    /// `PSAFD`: auto flag detect. Only a spec of 2011 has it.
    AutoFlagDetect,
    /// `PSDSX`, `PSSTW` and `PSSTH`: Audyssey DSX with stage width and
    /// height.
    AudysseyDsx,
    /// `PSPHG`: the height gain of `PLIIz`.
    HeightGain,
    /// `PSSP:OFF`: no effect speakers.
    EffectSpeakersOff,
    /// `PSDIL`: Dialog Level Adjust.
    DialogLevel,
    /// `MNPRV`: `InstaPrevue`.
    InstaPrevue,
    /// `MNSRC`: the source menu.
    SourceMenu,
    /// `NS` commands of network audio before HEOS.
    NetworkAudio,
    /// `SD7.1IN` and `PSLFL`: the 7.1 channel input and its LFE level.
    SevenOneIn,
}

/// Bits of [`SpecFeature::generations`].
const D13: u8 = 1 << 0;
const M13: u8 = 1 << 1;
const D15: u8 = 1 << 2;
const CURRENT: u8 = 1 << 3;

impl SpecFeature {
    /// All values.
    pub const ALL: &'static [Self] = &[
        Self::ProLogicMode,
        Self::SurroundBackMode,
        Self::FrontHeight,
        Self::DynamicCompression,
        Self::AutoFlagDetect,
        Self::AudysseyDsx,
        Self::HeightGain,
        Self::EffectSpeakersOff,
        Self::DialogLevel,
        Self::InstaPrevue,
        Self::SourceMenu,
        Self::NetworkAudio,
        Self::SevenOneIn,
    ];

    /// The generations whose specs list the feature for at least one model.
    /// A command that a spec lists for no model does not count.
    const fn generations(self) -> u8 {
        match self {
            Self::ProLogicMode | Self::AudysseyDsx | Self::HeightGain | Self::NetworkAudio => {
                D13 | M13 | D15
            }
            Self::InstaPrevue | Self::FrontHeight | Self::EffectSpeakersOff => D13 | M13,
            Self::SurroundBackMode => D13,
            Self::DynamicCompression | Self::SourceMenu => M13,
            Self::AutoFlagDetect => 0,
            Self::DialogLevel => D15,
            Self::SevenOneIn => M13 | D15 | CURRENT,
        }
    }
}

/// Command starts of a [`SpecFeature`] that no query has: values of a
/// command and commands without a query.
const LINE_FEATURES: [(&str, SpecFeature); 4] = [
    ("PSSP:OFF", SpecFeature::EffectSpeakersOff),
    ("MNSRC", SpecFeature::SourceMenu),
    ("SD7.1IN", SpecFeature::SevenOneIn),
    ("PSLFL", SpecFeature::SevenOneIn),
];

impl SpecGeneration {
    /// The generation for the text of `CommApiVers`, for example `0301`.
    #[must_use]
    pub fn of(comm_api_version: Option<&str>, brand: Brand) -> Self {
        Self::of_version(
            comm_api_version.and_then(|version| version.trim().parse().ok()),
            brand,
        )
    }

    /// The generation for `CommApiVers` as a number, for example `301`.
    #[must_use]
    pub const fn of_version(comm_api_version: Option<u16>, brand: Brand) -> Self {
        match (comm_api_version, brand) {
            (Some(210), Brand::Denon) => Self::Denon2013,
            (Some(210), Brand::Marantz) => Self::Marantz2013,
            (Some(250 | 300), Brand::Denon) => Self::Denon2015,
            (Some(301..), _) => Self::Current,
            _ => Self::Unknown,
        }
    }

    const fn bit(self) -> Option<u8> {
        match self {
            Self::Denon2013 => Some(D13),
            Self::Marantz2013 => Some(M13),
            Self::Denon2015 => Some(D15),
            Self::Current => Some(CURRENT),
            Self::Unknown => None,
        }
    }

    /// Returns true if a spec of this generation has the feature. True for
    /// [`SpecGeneration::Unknown`].
    #[must_use]
    pub const fn supports(self, feature: SpecFeature) -> bool {
        match self.bit() {
            Some(bit) => feature.generations() & bit != 0,
            None => true,
        }
    }

    /// Returns false if the query text asks for a [`SpecFeature`] that this
    /// generation does not have. True for other queries and for query texts
    /// that this library does not know.
    #[must_use]
    pub fn allows(self, query: &str) -> bool {
        query_for_text(query)
            .and_then(Query::spec_feature)
            .is_none_or(|feature| self.supports(feature))
    }

    /// Returns false if the command line needs a [`SpecFeature`] that this
    /// generation does not have. Raw `NS` lines are not checked: newer
    /// receivers have some `NS` commands too.
    #[must_use]
    pub fn allows_line(self, line: &str) -> bool {
        spec_feature_of_line(line).is_none_or(|feature| self.supports(feature))
    }

    /// The number of network audio presets (favorites): 36 on
    /// [`SpecGeneration::Denon2015`], else 56. `None` on
    /// [`SpecGeneration::Current`]: its specs have no preset commands.
    #[must_use]
    pub const fn network_presets(self) -> Option<u8> {
        match self {
            Self::Denon2015 => Some(36),
            Self::Current => None,
            _ => Some(MAX_NETWORK_PRESET),
        }
    }
}

/// The [`SpecFeature`] that a command line needs.
#[must_use]
pub fn spec_feature_of_line(line: &str) -> Option<SpecFeature> {
    LINE_FEATURES
        .iter()
        .find(|(start, _)| line.starts_with(start))
        .map(|(_, feature)| *feature)
        .or_else(|| query_for_command(line)?.spec_feature())
}

impl Query {
    /// The [`SpecFeature`] of the value. `None` if it does not depend on the
    /// generation.
    #[must_use]
    pub const fn spec_feature(self) -> Option<SpecFeature> {
        use SpecFeature as F;
        Some(match self {
            Self::ProLogicMode => F::ProLogicMode,
            Self::SurroundBackMode => F::SurroundBackMode,
            Self::FrontHeight => F::FrontHeight,
            Self::DynamicCompression => F::DynamicCompression,
            Self::AutoFlagDetect => F::AutoFlagDetect,
            Self::AudysseyDsx | Self::StageWidth | Self::StageHeight => F::AudysseyDsx,
            Self::HeightGain => F::HeightGain,
            Self::DialogLevel => F::DialogLevel,
            Self::InstaPrevue => F::InstaPrevue,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use SpecGeneration as G;

    const KNOWN: [SpecGeneration; 4] = [G::Denon2013, G::Marantz2013, G::Denon2015, G::Current];

    #[test]
    fn generation_follows_version_and_brand() {
        let of = |text, brand| G::of(text, brand);
        assert_eq!(of(Some("0210"), Brand::Denon), G::Denon2013);
        assert_eq!(of(Some(" 0210 "), Brand::Marantz), G::Marantz2013);
        assert_eq!(of(Some("0250"), Brand::Denon), G::Denon2015);
        assert_eq!(of(Some("0300"), Brand::Denon), G::Denon2015);
        assert_eq!(of(Some("0300"), Brand::Marantz), G::Unknown);
        assert_eq!(of(Some("0220"), Brand::Marantz), G::Unknown);
        assert_eq!(of(Some("0301"), Brand::Marantz), G::Current);
        assert_eq!(of(Some("0400"), Brand::Denon), G::Current);
        assert_eq!(of(Some("x"), Brand::Denon), G::Unknown);
        assert_eq!(of(None, Brand::Denon), G::Unknown);
        assert_eq!(G::of_version(Some(301), Brand::Denon), G::Current);
        assert_eq!(G::of_version(None, Brand::Marantz), G::Unknown);
    }

    #[test]
    fn all_has_each_value_once() {
        // A new value must be added here, then to `ALL`.
        let index = |feature| match feature {
            SpecFeature::ProLogicMode => 0,
            SpecFeature::SurroundBackMode => 1,
            SpecFeature::FrontHeight => 2,
            SpecFeature::DynamicCompression => 3,
            SpecFeature::AutoFlagDetect => 4,
            SpecFeature::AudysseyDsx => 5,
            SpecFeature::HeightGain => 6,
            SpecFeature::EffectSpeakersOff => 7,
            SpecFeature::DialogLevel => 8,
            SpecFeature::InstaPrevue => 9,
            SpecFeature::SourceMenu => 10,
            SpecFeature::NetworkAudio => 11,
            SpecFeature::SevenOneIn => 12,
        };
        assert_eq!(SpecFeature::ALL.len(), 13);
        for (i, feature) in SpecFeature::ALL.iter().enumerate() {
            assert_eq!(index(*feature), i, "{feature:?}");
        }
    }

    #[test]
    fn unknown_supports_all() {
        for feature in SpecFeature::ALL {
            assert!(G::Unknown.supports(*feature));
        }
    }

    #[test]
    fn matrix_follows_the_specs() {
        use SpecFeature as F;
        let rows: [(F, [bool; 4]); 13] = [
            (F::ProLogicMode, [true, true, true, false]),
            (F::AudysseyDsx, [true, true, true, false]),
            (F::HeightGain, [true, true, true, false]),
            (F::NetworkAudio, [true, true, true, false]),
            (F::InstaPrevue, [true, true, false, false]),
            (F::SurroundBackMode, [true, false, false, false]),
            (F::FrontHeight, [true, true, false, false]),
            (F::DynamicCompression, [false, true, false, false]),
            (F::SourceMenu, [false, true, false, false]),
            (F::AutoFlagDetect, [false; 4]),
            (F::EffectSpeakersOff, [true, true, false, false]),
            (F::DialogLevel, [false, false, true, false]),
            (F::SevenOneIn, [false, true, true, true]),
        ];
        assert_eq!(rows.len(), SpecFeature::ALL.len());
        for (feature, expected) in rows {
            for (generation, expected) in KNOWN.into_iter().zip(expected) {
                assert_eq!(
                    generation.supports(feature),
                    expected,
                    "{feature:?} {generation:?}"
                );
            }
        }
    }

    #[test]
    fn lines_and_queries_need_their_feature() {
        use SpecFeature as F;
        for (line, feature) in [
            ("PSMODE:MUSIC", Some(F::ProLogicMode)),
            ("PSSB:ON", Some(F::SurroundBackMode)),
            ("PSFH:ON", Some(F::FrontHeight)),
            ("PSSTW 10", Some(F::AudysseyDsx)),
            ("PSSP:OFF", Some(F::EffectSpeakersOff)),
            ("PSSP:FH", None),
            ("PSSPV ON", None),
            ("MNSRC ON", Some(F::SourceMenu)),
            ("MNPRV ON", Some(F::InstaPrevue)),
            ("SD7.1IN", Some(F::SevenOneIn)),
            ("SDHDMI", None),
            ("PSLFL 05", Some(F::SevenOneIn)),
            ("PSLFE 05", None),
            ("PSDIL ON", Some(F::DialogLevel)),
            ("NS9A", None),
        ] {
            assert_eq!(spec_feature_of_line(line), feature, "{line}");
        }
        assert!(!G::Current.allows("PSMODE: ?"));
        assert!(G::Denon2015.allows("PSMODE: ?"));
        assert!(G::Current.allows("MV?"));
        assert!(G::Current.allows("XX ?"));
        assert!(!G::Current.allows_line("PSDSX ONHW"));
        assert!(G::Current.allows_line("MV50"));
    }

    #[test]
    fn every_query_line_gives_the_spec_feature_of_its_query() {
        for query in Query::ALL {
            for brand in [Brand::Denon, Brand::Marantz] {
                let Some(text) = query.text(brand) else {
                    continue;
                };
                let start = text.trim_end_matches('?').trim_end();
                assert_eq!(query_for_command(start), Some(*query), "{text}");
                assert_eq!(spec_feature_of_line(start), query.spec_feature(), "{text}");
            }
        }
    }

    #[test]
    fn every_spec_feature_but_network_audio_has_a_line() {
        let lines = [
            "PSMODE:MUSIC",
            "PSSB:ON",
            "PSFH:ON",
            "PSDCO LOW",
            "PSAFD ON",
            "PSDSX ONH",
            "PSPHG LOW",
            "PSSP:OFF",
            "PSDIL ON",
            "MNPRV ON",
            "MNSRC ON",
            "SD7.1IN",
        ];
        for feature in SpecFeature::ALL {
            let found = lines
                .iter()
                .any(|line| spec_feature_of_line(line) == Some(*feature));
            assert_eq!(found, *feature != SpecFeature::NetworkAudio, "{feature:?}");
        }
    }

    #[test]
    fn presets_follow_the_generation() {
        assert_eq!(G::Denon2015.network_presets(), Some(36));
        assert_eq!(G::Denon2013.network_presets(), Some(56));
        assert_eq!(G::Unknown.network_presets(), Some(56));
        assert_eq!(G::Current.network_presets(), None);
    }
}
