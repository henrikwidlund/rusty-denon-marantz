//! Events that a receiver sends on telnet.

use crate::source::is_telnet_source;
use crate::zone::Zone;

code_enum! {
    /// Prefix of a telnet event. The code is the prefix text.
    pub enum Event {
        Z2 = "Z2", "Z2";
        Z3 = "Z3", "Z3";
        Sv = "SV", "SV";
        Ns = "NS", "NS";
        Pw = "PW", "PW";
        Stby = "STBY", "STBY";
        Mn = "MN", "MN";
        Ms = "MS", "MS";
        Tp = "TP", "TP";
        Bt = "BT", "BT";
        Ug = "UG", "UG";
        Hd = "HD", "HD";
        Dim = "DIM", "DIM";
        Pv = "PV", "PV";
        Zm = "ZM", "ZM";
        Tr = "TR", "TR";
        Op = "OP", "OP";
        Sd = "SD", "SD";
        Dc = "DC", "DC";
        Vs = "VS", "VS";
        Sr = "SR", "SR";
        Rm = "RM", "RM";
        Ps = "PS", "PS";
        Nse = "NSE", "NSE";
        Ss = "SS", "SS";
        Mv = "MV", "MV";
        Nsa = "NSA", "NSA";
        Cv = "CV", "CV";
        Si = "SI", "SI";
        Ilb = "ILB", "ILB";
        Tf = "TF", "TF";
        Sp = "SP", "SP";
        Eco = "ECO", "ECO";
        Slp = "SLP", "SLP";
        Sy = "SY", "SY";
        Tm = "TM", "TM";
        Mu = "MU", "MU";
    }
}

impl Event {
    /// Splits a line into the event and the text after it.
    ///
    /// The longest matching prefix wins, so `NSE1` is `Nse` and not `Ns`.
    #[must_use]
    pub fn split_prefix(line: &str) -> Option<(Self, &str)> {
        [4, 3, 2].into_iter().find_map(|len| {
            let event = Self::from_code(line.get(..len)?)?;
            Some((event, &line[len..]))
        })
    }

    /// Returns true if the event reports one setting of the whole receiver that
    /// the state keeps apart: triggers, Bluetooth, picture, digital input,
    /// illumination, speaker preset and tuner.
    #[must_use]
    pub const fn is_device_report(self) -> bool {
        matches!(
            self,
            Self::Tr
                | Self::Bt
                | Self::Pv
                | Self::Dc
                | Self::Ilb
                | Self::Sp
                | Self::Tf
                | Self::Tp
                | Self::Tm
        )
    }

    /// Returns true if the event is about the whole receiver, not one zone.
    #[must_use]
    pub const fn is_all_zone(self) -> bool {
        matches!(
            self,
            Self::Ug
                | Self::Ilb
                | Self::Tf
                | Self::Hd
                | Self::Ns
                | Self::Pw
                | Self::Rm
                | Self::Dim
                | Self::Sy
                | Self::Mn
                | Self::Tr
                | Self::Nse
                | Self::Tm
                | Self::Nsa
                | Self::Tp
        )
    }
}

/// Zone that an event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventZone {
    /// Main zone.
    Main,
    /// Zone 2.
    Zone2,
    /// Zone 3.
    Zone3,
    /// All zones.
    All,
}

impl EventZone {
    /// The zone. `None` for events about all zones.
    #[must_use]
    pub const fn zone(self) -> Option<Zone> {
        match self {
            Self::Main => Some(Zone::Main),
            Self::Zone2 => Some(Zone::Zone2),
            Self::Zone3 => Some(Zone::Zone3),
            Self::All => None,
        }
    }

    /// Returns true if an event with this zone is relevant for `zone`.
    #[must_use]
    pub const fn applies_to(self, zone: Zone) -> bool {
        match self.zone() {
            Some(own) => own.index() == zone.index(),
            None => true,
        }
    }
}

/// A telnet line split into zone, event and parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Message<'a> {
    /// Zone of the event.
    pub zone: EventZone,
    /// Event.
    pub event: Event,
    /// Text after the event.
    pub parameter: &'a str,
}

impl<'a> Message<'a> {
    /// Parses a line from the receiver.
    ///
    /// Returns `None` for lines that carry no event or that are not useful.
    /// Zone 2 and zone 3 lines are rewritten: `Z2CD` is a source change, `Z243`
    /// is a volume change and `Z2MUON` is a mute change in zone 2.
    #[must_use]
    pub fn parse(line: &'a str) -> Option<Self> {
        if line.len() < 3 {
            return None;
        }
        let (mut event, mut parameter) = Event::split_prefix(line)?;

        // `MVMAX` is not sent when the maximum volume changes. The limit comes
        // from the `SS` event instead.
        if event == Event::Mv && parameter.starts_with("MAX") {
            return None;
        }

        let mut zone = EventZone::Main;
        if event.is_all_zone() {
            zone = EventZone::All;
        } else if matches!(event, Event::Z2 | Event::Z3) {
            zone = if event == Event::Z2 {
                EventZone::Zone2
            } else {
                EventZone::Zone3
            };
            if is_telnet_source(parameter) {
                event = Event::Si;
            } else if !parameter.is_empty() && parameter.bytes().all(|b| b.is_ascii_digit()) {
                event = Event::Mv;
            } else if let Some((nested, rest)) = Event::split_prefix(parameter) {
                event = nested;
                parameter = rest;
            }
        }

        if parameter.starts_with("SDV END") || parameter.starts_with("HDMIDIAG END") {
            return None;
        }
        Some(Self {
            zone,
            event,
            parameter,
        })
    }
}

/// Parameter prefixes of events that the receiver repeats when it is queried.
const DUPLICATE_PREFIXES: [&str; 25] = [
    "INFSIGRES I",
    "INFSIGRES O",
    "SDVIN ",
    "SDVOUT ",
    "INFSIGHDR I",
    "INFSIGHDR O",
    "INFSIGPIX",
    "INFSIGFRL I",
    "INFSIGFRL O",
    "INFSIGCOS",
    "INFSIGECI",
    "HDMIDIAGMAXRES",
    "HDMIDIAGERR",
    "HDMIDIAGHDCP23",
    "HDMIDIAGHDR",
    "HDMIDIAGDCR",
    "HDMIDIAGDVN",
    "HDMIDIAGDTSX",
    "HDMIDIAGATMS",
    "HDMIDIAGPCM2",
    "INFASP",
    "INFINS",
    "INFAISFSV",
    "SDA",
    "SMI",
];

/// Finds events that repeat the last value of a group of events.
///
/// A query makes the receiver send some events again, also when nothing
/// changed. This filter keeps the last parameter of each group.
#[derive(Debug)]
pub struct DuplicateFilter {
    last: [String; DUPLICATE_PREFIXES.len()],
}

impl Default for DuplicateFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl DuplicateFilter {
    /// Creates a filter that has seen no events.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last: core::array::from_fn(|_| String::new()),
        }
    }

    /// Returns false if the event repeats the last value of its group.
    ///
    /// Events that are not in a group are always accepted.
    pub fn accept(&mut self, parameter: &str) -> bool {
        let Some(index) = DUPLICATE_PREFIXES
            .iter()
            .position(|prefix| parameter.starts_with(prefix))
        else {
            return true;
        };
        let last = &mut self.last[index];
        if last == parameter {
            return false;
        }
        last.clear();
        last.push_str(parameter);
        true
    }

    /// Forgets all values. Call this before queries that must be reported.
    pub fn clear(&mut self) {
        for last in &mut self.last {
            last.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Option<(EventZone, Event, &str)> {
        Message::parse(line).map(|m| (m.zone, m.event, m.parameter))
    }

    #[test]
    fn events_round_trip() {
        assert_eq!(Event::ALL.len(), 37);
        for &event in Event::ALL {
            assert_eq!(Event::from_code(event.code()), Some(event));
            let line = format!("{}X", event.code());
            assert_eq!(Event::split_prefix(&line), Some((event, "X")), "{line}");
        }
    }

    #[test]
    fn longest_prefix_wins() {
        assert_eq!(
            Event::split_prefix("NSE1Hello"),
            Some((Event::Nse, "1Hello"))
        );
        assert_eq!(
            Event::split_prefix("NSA0Hello"),
            Some((Event::Nsa, "0Hello"))
        );
        assert_eq!(Event::split_prefix("NS9A"), Some((Event::Ns, "9A")));
        assert_eq!(Event::split_prefix("STBYOFF"), Some((Event::Stby, "OFF")));
        assert_eq!(Event::split_prefix("SPPR 1"), Some((Event::Sp, "PR 1")));
        assert_eq!(Event::split_prefix("DIM OFF"), Some((Event::Dim, " OFF")));
        assert_eq!(Event::split_prefix("XX"), None);
        assert_eq!(Event::split_prefix(""), None);
        assert_eq!(Event::split_prefix("Ü"), None);
    }

    #[test]
    fn main_zone_events() {
        use Event::{Cv, Mu, Mv, Si, Zm};
        assert_eq!(parse("ZMON"), Some((EventZone::Main, Zm, "ON")));
        assert_eq!(parse("MV435"), Some((EventZone::Main, Mv, "435")));
        assert_eq!(parse("MUOFF"), Some((EventZone::Main, Mu, "OFF")));
        assert_eq!(parse("SIGAME"), Some((EventZone::Main, Si, "GAME")));
        assert_eq!(parse("CVFL 50"), Some((EventZone::Main, Cv, "FL 50")));
    }

    #[test]
    fn all_zone_events() {
        assert_eq!(parse("PWON"), Some((EventZone::All, Event::Pw, "ON")));
        assert_eq!(
            parse("NSE1Hello"),
            Some((EventZone::All, Event::Nse, "1Hello"))
        );
        assert_eq!(parse("DIM BRI"), Some((EventZone::All, Event::Dim, " BRI")));
        assert_eq!(
            parse("MNMEN ON"),
            Some((EventZone::All, Event::Mn, "MEN ON"))
        );
    }

    #[test]
    fn zone_two_and_three_are_rewritten() {
        use Event::{Mu, Mv, Si, Z2, Z3};
        assert_eq!(parse("Z2ON"), Some((EventZone::Zone2, Z2, "ON")));
        assert_eq!(parse("Z3OFF"), Some((EventZone::Zone3, Z3, "OFF")));
        assert_eq!(parse("Z2CD"), Some((EventZone::Zone2, Si, "CD")));
        assert_eq!(parse("Z3SAT/CBL"), Some((EventZone::Zone3, Si, "SAT/CBL")));
        assert_eq!(parse("Z243"), Some((EventZone::Zone2, Mv, "43")));
        assert_eq!(parse("Z3435"), Some((EventZone::Zone3, Mv, "435")));
        assert_eq!(parse("Z2MUON"), Some((EventZone::Zone2, Mu, "ON")));
        assert_eq!(
            parse("Z3STBY2H"),
            Some((EventZone::Zone3, Event::Stby, "2H"))
        );
    }

    #[test]
    fn ignored_lines() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("PW"), None);
        assert_eq!(parse("MVMAX 98"), None);
        assert_eq!(parse("XYZ"), None);
        assert_eq!(parse("SYSDV END"), None);
        assert_eq!(parse("SYHDMIDIAG END"), None);
        assert_eq!(parse("SSSDV END"), None);
    }

    #[test]
    fn max_volume_check_only_for_main_zone_events() {
        assert_eq!(parse("MVMAX"), None);
        assert_eq!(parse("MV"), None);
    }

    #[test]
    fn events_for_zones() {
        assert!(EventZone::All.applies_to(Zone::Zone3));
        assert!(EventZone::Main.applies_to(Zone::Main));
        assert!(!EventZone::Main.applies_to(Zone::Zone2));
        assert!(EventZone::Zone2.applies_to(Zone::Zone2));
        assert!(!EventZone::Zone2.applies_to(Zone::Zone3));
    }

    #[test]
    fn duplicate_filter_drops_repeated_values() {
        let mut filter = DuplicateFilter::new();
        assert!(filter.accept("INFSIGRES I 4K"));
        assert!(!filter.accept("INFSIGRES I 4K"));
        assert!(filter.accept("INFSIGRES I 1080p"));
        assert!(filter.accept("INFSIGRES I 4K"));
        // Groups are independent.
        assert!(filter.accept("INFSIGRES O 4K"));
        assert!(!filter.accept("INFSIGRES O 4K"));
    }

    #[test]
    fn duplicate_filter_knows_extended_colorimetry() {
        let mut filter = DuplicateFilter::new();
        assert!(filter.accept("INFSIGECI 55"));
        assert!(!filter.accept("INFSIGECI 55"));
        assert!(filter.accept("INFSIGECI 00"));
    }

    #[test]
    fn duplicate_filter_knows_the_audio_sampling_rate() {
        // A receiver sends this event several times in a row.
        let mut filter = DuplicateFilter::new();
        assert!(filter.accept("INFAISFSV NON"));
        for _ in 0..6 {
            assert!(!filter.accept("INFAISFSV NON"));
        }
        assert!(filter.accept("INFAISFSV 48K"));
    }

    #[test]
    fn duplicate_filter_accepts_other_events() {
        let mut filter = DuplicateFilter::default();
        assert!(filter.accept("ON"));
        assert!(filter.accept("ON"));
        assert!(filter.accept(""));
    }

    #[test]
    fn duplicate_filter_clear() {
        let mut filter = DuplicateFilter::new();
        assert!(filter.accept("SDA HDMI"));
        assert!(!filter.accept("SDA HDMI"));
        filter.clear();
        assert!(filter.accept("SDA HDMI"));
    }

    #[test]
    fn duplicate_prefixes_are_unique() {
        for (index, prefix) in DUPLICATE_PREFIXES.iter().enumerate() {
            assert!(!DUPLICATE_PREFIXES[..index].contains(prefix), "{prefix}");
        }
    }

    proptest::proptest! {
        #[test]
        fn parse_never_panics(line in "\\PC{0,40}") {
            if let Some(message) = Message::parse(&line) {
                proptest::prop_assert!(line.ends_with(message.parameter));
            }
        }

        #[test]
        fn parse_ascii_never_panics(line in "[A-Z0-9 /+.:?-]{0,24}") {
            let _ = Message::parse(&line);
        }
    }
}
