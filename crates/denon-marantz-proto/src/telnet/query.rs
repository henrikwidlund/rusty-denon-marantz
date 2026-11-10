//! Queries that ask a receiver for its state.

use QueryGroup::{Core, Device, Names, Picture, Signal, Sound, Speakers, Toggles, Zones};

use crate::brand::Brand;

/// Asks for the power state of the main zone.
pub const MAIN_ZONE_POWER_QUERY: &str = "ZM?";

/// Asks for the power state of the whole device.
pub const DEVICE_POWER_QUERY: &str = "PW?";

/// Brands that a query is for.
#[derive(Debug, Clone, Copy)]
enum Brands {
    Both,
    Denon,
    Marantz,
}

#[derive(Debug, Clone, Copy)]
struct Query {
    text: &'static str,
    brands: Brands,
    group: QueryGroup,
}

impl Query {
    const fn is_for(&self, brand: Brand) -> bool {
        matches!(
            (self.brands, brand),
            (Brands::Both, _) | (Brands::Denon, Brand::Denon) | (Brands::Marantz, Brand::Marantz)
        )
    }
}

const fn both(group: QueryGroup, text: &'static str) -> Query {
    Query {
        text,
        brands: Brands::Both,
        group,
    }
}

const fn denon(group: QueryGroup, text: &'static str) -> Query {
    Query {
        text,
        brands: Brands::Denon,
        group,
    }
}

const fn marantz(group: QueryGroup, text: &'static str) -> Query {
    Query {
        text,
        brands: Brands::Marantz,
        group,
    }
}

labeled_enum! {
    /// A group of queries that ask for related values. A client chooses which
    /// groups the connection fetches and in which order, and can fetch a group
    /// later.
    QueryGroup {
        Core = "Core";
        Names = "Names";
        Signal = "Signal";
        Toggles = "Toggles";
        Device = "Device";
        Sound = "Sound";
        Zones = "Zones";
        Picture = "Picture";
        Speakers = "Speakers";
    }
}

impl QueryGroup {
    /// All groups, the most important first. Fetch them all with
    /// `TelnetConfig::query_groups` or one by one when they are needed.
    ///
    /// 1. [`Core`](Self::Core): power, source, volume and mute of the zones,
    ///    and the sound mode.
    /// 2. [`Names`](Self::Names): source names, sound mode list, friendly name
    ///    and Dirac slots.
    /// 3. [`Toggles`](Self::Toggles): on and off settings that clients toggle.
    ///    A toggle needs the current value.
    /// 4. [`Signal`](Self::Signal): video and audio signal information, which
    ///    clients show.
    /// 5. [`Device`](Self::Device): dimmer, eco, sleep, standby, video and
    ///    picture settings.
    /// 6. [`Sound`](Self::Sound): the sound settings and levels.
    /// 7. [`Zones`](Self::Zones): the volume settings and levels of the zones.
    /// 8. [`Picture`](Self::Picture): the picture mode and the picture
    ///    adjustments. Only older receivers answer these.
    pub const DEFAULT_ORDER: &'static [Self] = &[
        Self::Core,
        Self::Names,
        Self::Toggles,
        Self::Signal,
        Self::Device,
        Self::Sound,
        Self::Zones,
        Self::Picture,
        Self::Speakers,
    ];

    /// The groups that a connection fetches by default: the ones that most
    /// clients show or toggle at once. The others stay empty until the
    /// receiver reports a change, or until a client fetches them.
    pub const START_ORDER: &'static [Self] =
        &[Self::Core, Self::Names, Self::Toggles, Self::Signal];
}

/// Queries of the chosen groups, in the order of `groups`. A group that is
/// listed twice counts once. The receiver answers each query with the same
/// events as for changes.
pub fn queries(
    brand: Brand,
    groups: &'static [QueryGroup],
) -> impl Iterator<Item = &'static str> + Clone {
    groups
        .iter()
        .enumerate()
        .filter(|(index, group)| !groups[..*index].contains(group))
        .flat_map(move |(_, group)| group_queries(brand, *group))
}

/// Queries of one group, in the order that they are sent.
pub fn group_queries(
    brand: Brand,
    group: QueryGroup,
) -> impl Iterator<Item = &'static str> + Clone {
    QUERIES
        .iter()
        .filter(move |query| query.group == group && query.is_for(brand))
        .map(|query| query.text)
}

/// Which read answers a query of the [`Signal`](QueryGroup::Signal) group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SignalPart {
    /// The video signal. The web interface page
    /// [`VIDEO_SIGNAL_PATH`](crate::VIDEO_SIGNAL_PATH) answers it.
    Video,
    /// The channels. The web interface page
    /// [`AUDIO_CHANNELS_PATH`](crate::AUDIO_CHANNELS_PATH) answers it.
    Channels,
    /// The audio signal, the sampling rate and the HDMI limit. Only telnet
    /// answers it.
    Audio,
}

impl SignalPart {
    fn of(text: &str) -> Self {
        match text {
            "SSINFSIGRES ?" | "SYSDVIN ?" | "SYSDVOUT ?" | "SSINFSIGHDR ?" | "SSINFSIGPIX ?"
            | "SSINFSIGFRL ?" | "SSINFSIGCOS ?" => Self::Video,
            "OPINFASP ?" | "OPINFINS ?" => Self::Channels,
            _ => Self::Audio,
        }
    }
}

/// The queries of the signal group that belong to one part, in the order of
/// the group.
pub fn signal_queries(
    brand: Brand,
    part: SignalPart,
) -> impl Iterator<Item = &'static str> + Clone {
    group_queries(brand, Signal).filter(move |text| SignalPart::of(text) == part)
}

/// The queries to send first when the state was read with `AppCommand`.
///
/// `AppCommand` does not answer these, and clients need them soon: the quick
/// select (or the SMART select), the toggles, the sound modes that the
/// receiver offers and the Dirac Live slots.
pub fn after_appcommand_queries(brand: Brand) -> impl Iterator<Item = &'static str> + Clone {
    let only = |texts: &'static [&'static str]| {
        QUERIES
            .iter()
            .filter(move |query| query.is_for(brand) && texts.contains(&query.text))
            .map(|query| query.text)
    };
    only(&["MSSMART ?", "MSQUICK ?"])
        .chain(group_queries(brand, Toggles))
        .chain(only(&["OPSMLALL ?", "OPDLS ?"]))
}

/// Queries to send after connect with the default order of the groups.
///
/// Brand-specific queries are in the place where they are sent.
pub fn state_queries(brand: Brand) -> impl Iterator<Item = &'static str> + Clone {
    queries(brand, QueryGroup::DEFAULT_ORDER)
}

static QUERIES: [Query; 125] = [
    both(Core, "SI?"),                  // Select INPUT source
    both(Core, "MV?"),                  // MASTER VOLUME
    both(Core, "MU?"),                  // Mute
    both(Core, "Z2?"),                  // Z2 Zone Power
    both(Core, "Z2MU?"),                // Z2 Mute
    both(Core, "Z3?"),                  // Z3 Zone Power
    both(Core, "Z3MU?"),                // Z3 Mute
    both(Core, "MS?"),                  // Surround mode
    both(Toggles, "MNMEN?"),            // Menu
    marantz(Core, "MSSMART ?"),         // SMART Select
    denon(Core, "MSQUICK ?"),           // Quick Select
    both(Toggles, "TR?"),               // Trigger
    marantz(Toggles, "PSMDAX ?"),       // MDAX
    marantz(Toggles, "PSDACFIL ?"),     // DAC Filter
    marantz(Toggles, "ILB ?"),          // Illumination
    marantz(Toggles, "SSHOS ?"),        // Auto Lip Sync
    both(Toggles, "PSTONE CTRL ?"),     // TONE
    both(Toggles, "PSDYNEQ ?"),         // Dynamic EQ
    both(Toggles, "PSLFC ?"),           // Audyssey LFC
    both(Toggles, "PSNEURAL ?"),        // Neural:X
    both(Toggles, "PSIMAXAUD ?"),       // IMAX Audio Settings Auto/Manual
    both(Toggles, "PSIMAXSWM ?"),       // IMAX Subwoofer Mode
    both(Toggles, "PSSWR ?"),           // Subwoofer
    both(Toggles, "SSTTR ?"),           // Tactile Transducer
    both(Toggles, "VSAUDIO ?"),         // HDMI Audio Decode
    both(Toggles, "PSCES ?"),           // CENTER Spread
    both(Toggles, "PSLOM ?"),           // Loudness Management
    both(Toggles, "PSCINEMA EQ. ?"),    // CINEMA EQ
    both(Toggles, "BTTX ?"),            // Bluetooth Transmitter
    both(Toggles, "PSSPV ?"),           // Speaker Virtualizer
    both(Toggles, "PSGEQ ?"),           // Graphic EQ
    both(Toggles, "PSHEQ ?"),           // Headphone EQ
    both(Device, "DIM ?"),              // Dimmer
    both(Device, "ECO?"),               // ECO
    both(Device, "SLP?"),               // Sleep
    both(Device, "Z2SLP?"),             // Z2 Sleep
    both(Device, "Z3SLP?"),             // Z3 Sleep
    denon(Device, "Z2QUICK ?"),         // Z2 Quick Select
    denon(Device, "Z3QUICK ?"),         // Z3 Quick Select
    marantz(Device, "Z2SMART ?"),       // Z2 SMART Select
    marantz(Device, "Z3SMART ?"),       // Z3 SMART Select
    both(Device, "VSMONI ?"),           // HDMI Output
    both(Sound, "PSDELAY ?"),           // Audio Delay
    both(Signal, "SSINFSIGRES ?"),      // Video Signal
    both(Signal, "SSINFAISFSV ?"),      // Audio Sampling Rate
    both(Signal, "SYSDA ?"),            // Audio Signal
    both(Signal, "SYSMI ?"),            // Audio Sound
    both(Signal, "OPINFASP ?"),         // Output Channels
    both(Signal, "OPINFINS ?"),         // Input Channels
    both(Signal, "SYSDVIN ?"),          // Video Signal Input
    both(Signal, "SYSDVOUT ?"),         // Video Signal Output
    both(Signal, "SYHDMIDIAGMAXRES ?"), // HDMI Max Resolution
    both(Signal, "SSINFSIGHDR ?"),      // Video HDR Format
    both(Signal, "SSINFSIGPIX ?"),      // Video Pixel Depth
    both(Signal, "SSINFSIGFRL ?"),      // Video Max FRL
    both(Signal, "SSINFSIGCOS ?"),      // Signal Color Space
    both(Signal, "SSINFSIGECI ?"),      // Signal Extended Colorimetry
    both(Sound, "PSDIRAC ?"),           // Dirac Live Filter
    both(Names, "OPDLS ?"),             // Dirac Live slots: name, filter and kind
    both(Names, "SSFUN ?"),             // Source names
    both(Names, "SSSOD ?"),             // Sources in use or hidden
    both(Names, "OPSMLALL ?"),          // Sound modes that the receiver offers
    both(Names, "NSFRN?"),              // Friendly name
    denon(Toggles, "OPALS ?"),          // Auto Lip Sync
    denon(Names, "SSQS ?"),             // Quick select names
    denon(Sound, "SSLEV ?"),            // Speaker channel levels
    both(Sound, "PSREFLEV ?"),          // Reference Level
    both(Sound, "PSDYNVOL ?"),          // Dynamic Vol.
    both(Device, "SPPR ?"),             // Speaker Preset
    both(Picture, "PV?"),               // Picture Mode
    both(Device, "DC?"),                // Digital Codec
    both(Sound, "PSBAS ?"),             // BASS
    both(Sound, "PSTRE ?"),             // TREBLE
    both(Sound, "PSCNTAMT ?"),          // Containment Amount
    both(Sound, "PSMULTEQ: ?"),         // MultEQ
    both(Sound, "PSIMAX ?"),            // IMAX
    both(Sound, "PSIMAXHPF ?"),         // IMAX High Pass Filter
    both(Sound, "PSIMAXLPF ?"),         // IMAX Low Pass Filter
    both(Sound, "PSIMAXSWO ?"),         // Subwoofer Output LFE+Main/LFE
    both(Sound, "PSSWL ?"),             // Subwoofer Level
    both(Device, "STBY?"),              // Auto Standby
    both(Device, "Z2STBY?"),            // Auto Standby
    both(Device, "Z3STBY?"),            // Auto Standby
    both(Device, "VSVPM ?"),            // Video Process
    both(Device, "PSFRONT?"),           // Front Speaker A/B
    both(Device, "VSASP ?"),            // Aspect Ratio
    both(Device, "VSVST ?"),            // Vertical Stretch
    both(Device, "VSSC ?"),             // Analog to HDMI Resolution
    both(Device, "VSSCH ?"),            // HDMI Upscaler
    both(Sound, "PSLFE ?"),             // LFE Level
    both(Sound, "PSBSC ?"),             // Bass Sync
    both(Sound, "PSDEH ?"),             // Dialog Enhancer
    both(Sound, "PSAUROPR ?"),          // Auro-Matic Preset
    both(Sound, "PSAUROST ?"),          // Auro-Matic Strength
    both(Sound, "PSAUROMODE ?"),        // AURO-3D Mode
    both(Sound, "PSRSZ ?"),             // ROOM SIZE
    both(Sound, "PSDIC ?"),             // Dialog Control
    both(Sound, "PSSP: ?"),             // Effect Speaker selection
    both(Sound, "PSDRC ?"),             // DRC
    both(Sound, "PSDEL ?"),             // DELAY TIME
    both(Sound, "PSRSTR ?"),            // AUDIO RESTORER
    both(Sound, "PSCLV ?"),             // Center level
    both(Sound, "PSPAN ?"),             // Panorama
    both(Sound, "PSSURLEV ?"),          // Surround level compensation
    both(Sound, "PSDIM ?"),             // Dimension
    both(Sound, "PSCEN ?"),             // Center width
    both(Sound, "PSCEW ?"),             // Center width, current receivers
    both(Sound, "PSEFF ?"),             // Effect
    both(Sound, "SD?"),                 // Audio input mode
    both(Sound, "PSCEI ?"),             // Center image
    marantz(Sound, "PSCEG ?"),          // Center gain
    // The A10H and its app never get an answer to this query. It pushes the list after an input change.
    both(Sound, "CV?"),            // Channel Volume
    denon(Speakers, "SSSPC ?"),    // Speaker configuration
    denon(Speakers, "SSCFR ?"),    // Speaker crossovers
    denon(Speakers, "SSAEQLRS ?"), // Graphic EQ of each speaker group
    both(Zones, "SSVCTZMA ?"),     // Volume settings of the main zone
    both(Zones, "SSVCTZ2S ?"),     // Volume settings of zone 2
    both(Zones, "Z2CV?"),          // Channel levels of zone 2
    both(Zones, "Z3CV?"),          // Channel levels of zone 3
    both(Zones, "SSVCTZ3S ?"),     // Volume settings of zone 3
    both(Picture, "PVCN ?"),       // Contrast
    both(Picture, "PVBR ?"),       // Brightness
    both(Picture, "PVST ?"),       // Saturation
    both(Picture, "PVDNR ?"),      // Noise Reduction
    both(Picture, "PVENH ?"),      // Enhancer
];

#[cfg(test)]
mod tests {
    use super::*;

    const BRANDS: [Brand; 2] = [Brand::Denon, Brand::Marantz];

    #[test]
    fn every_query_asks_for_a_value() {
        for brand in BRANDS {
            for query in state_queries(brand) {
                assert!(query.ends_with('?'), "{query}");
            }
        }
    }

    #[test]
    fn speaker_queries_are_last_and_for_denon() {
        let denon: Vec<_> = state_queries(Brand::Denon).collect();
        let marantz: Vec<_> = state_queries(Brand::Marantz).collect();
        let tail: Vec<_> = denon[denon.len() - 3..].to_vec();
        assert_eq!(tail, ["SSSPC ?", "SSCFR ?", "SSAEQLRS ?"]);
        assert!(!marantz.contains(&"SSAEQLRS ?"));
        assert!(denon.contains(&"SD?") && marantz.contains(&"SD?"));
        assert_eq!(QueryGroup::DEFAULT_ORDER.last(), Some(&Speakers));
    }

    #[test]
    fn brand_specific_queries() {
        let denon: Vec<_> = state_queries(Brand::Denon).collect();
        let marantz: Vec<_> = state_queries(Brand::Marantz).collect();
        assert!(denon.contains(&"MSQUICK ?"));
        assert!(!denon.contains(&"MSSMART ?"));
        assert!(marantz.contains(&"MSSMART ?"));
        for query in ["PSMDAX ?", "PSDACFIL ?", "ILB ?", "SSHOS ?"] {
            assert!(marantz.contains(&query) && !denon.contains(&query));
        }
        assert!(denon.contains(&"OPALS ?") && !marantz.contains(&"OPALS ?"));
        assert_eq!(denon.len(), 117);
        assert_eq!(marantz.len(), 116);
    }

    #[test]
    fn both_brands_ask_for_the_dirac_slots() {
        for brand in BRANDS {
            assert!(state_queries(brand).any(|query| query == "OPDLS ?"));
        }
    }

    #[test]
    fn a_denon_receiver_asks_for_the_quick_selects_and_levels() {
        for wanted in ["SSQS ?", "SSLEV ?"] {
            assert!(state_queries(Brand::Denon).any(|query| query == wanted));
            assert!(!state_queries(Brand::Marantz).any(|query| query == wanted));
        }
    }

    #[test]
    fn both_brands_ask_for_the_sources_sound_modes_and_name() {
        for brand in BRANDS {
            for wanted in ["SSFUN ?", "SSSOD ?", "OPSMLALL ?", "NSFRN?"] {
                assert!(state_queries(brand).any(|query| query == wanted));
            }
        }
    }

    #[test]
    fn shared_queries_keep_the_same_order() {
        let shared = |brand| -> Vec<&'static str> {
            state_queries(brand)
                .filter(|query| {
                    QUERIES
                        .iter()
                        .any(|q| q.text == *query && matches!(q.brands, Brands::Both))
                })
                .collect()
        };
        assert_eq!(shared(Brand::Denon), shared(Brand::Marantz));
    }

    #[test]
    fn both_brands_ask_for_extended_colorimetry() {
        for brand in BRANDS {
            assert!(state_queries(brand).any(|query| query == "SSINFSIGECI ?"));
        }
    }

    #[test]
    fn the_groups_hold_every_query_once() {
        for brand in BRANDS {
            let mut from_groups: Vec<_> = QueryGroup::ALL
                .iter()
                .flat_map(|group| group_queries(brand, *group))
                .collect();
            let mut all: Vec<_> = state_queries(brand).collect();
            from_groups.sort_unstable();
            all.sort_unstable();
            assert_eq!(from_groups, all);
        }
        assert_eq!(QueryGroup::DEFAULT_ORDER.len(), QueryGroup::ALL.len());
    }

    #[test]
    fn the_default_order_starts_with_the_core_and_the_toggles_come_before_the_signal() {
        for brand in BRANDS {
            let all: Vec<_> = state_queries(brand).collect();
            let position = |wanted| all.iter().position(|query| *query == wanted).unwrap();
            assert_eq!(all[0], "SI?");
            assert!(position("MS?") < position("SSFUN ?"));
            assert!(position("SSFUN ?") < position("PSTONE CTRL ?"));
            assert!(position("PSTONE CTRL ?") < position("SSINFSIGRES ?"));
            assert!(position("SSINFSIGRES ?") < position("PSBAS ?"));
            assert!(position("PSBAS ?") < position("SSVCTZMA ?"));
        }
    }

    #[test]
    fn a_client_chooses_and_orders_the_groups() {
        static CHOSEN: [QueryGroup; 3] = [Signal, Core, Signal];
        let chosen: Vec<_> = queries(Brand::Denon, &CHOSEN).collect();
        let signal = group_queries(Brand::Denon, Signal).count();
        let core = group_queries(Brand::Denon, Core).count();
        // A group that is listed twice counts once.
        assert_eq!(chosen.len(), signal + core);
        assert_eq!(chosen[0], "SSINFSIGRES ?");
        assert_eq!(chosen[signal], "SI?");
        assert!(queries(Brand::Denon, &[]).next().is_none());
    }

    #[test]
    fn brand_queries_stay_in_their_group() {
        assert!(group_queries(Brand::Denon, Core).any(|q| q == "MSQUICK ?"));
        assert!(group_queries(Brand::Marantz, Core).any(|q| q == "MSSMART ?"));
        assert!(!group_queries(Brand::Denon, Core).any(|q| q == "MSSMART ?"));
    }

    #[test]
    fn the_parts_of_the_signal_group_cover_it() {
        for brand in [Brand::Denon, Brand::Marantz] {
            let all = group_queries(brand, Signal).count();
            let parts: usize = [SignalPart::Video, SignalPart::Channels, SignalPart::Audio]
                .into_iter()
                .map(|part| signal_queries(brand, part).count())
                .sum();
            assert_eq!(all, parts);
            assert_eq!(signal_queries(brand, SignalPart::Channels).count(), 2);
            assert!(signal_queries(brand, SignalPart::Audio).any(|q| q == "SYSDA ?"));
            assert!(!signal_queries(brand, SignalPart::Video).any(|q| q == "SYSDA ?"));
        }
    }

    #[test]
    fn queries_after_appcommand_are_not_what_it_answers() {
        let denon: Vec<_> = after_appcommand_queries(Brand::Denon).collect();
        let marantz: Vec<_> = after_appcommand_queries(Brand::Marantz).collect();
        assert_eq!(denon[0], "MSQUICK ?");
        assert_eq!(marantz[0], "MSSMART ?");
        assert!(denon.ends_with(&["OPDLS ?", "OPSMLALL ?"]));
        for list in [&denon, &marantz] {
            // AppCommand answers these.
            for answered in ["MV?", "MU?", "SI?", "MS?", "Z2?"] {
                assert!(!list.contains(&answered), "{answered}");
            }
            assert!(list.contains(&"PSDYNEQ ?"));
        }
    }

    #[test]
    fn the_start_groups_are_the_first_of_the_default_order() {
        assert!(QueryGroup::DEFAULT_ORDER.starts_with(QueryGroup::START_ORDER));
    }

    #[test]
    fn queries_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for query in &QUERIES {
            assert!(seen.insert(query.text), "{}", query.text);
        }
    }
}
