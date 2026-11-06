//! Queries that ask a receiver for its state.

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
}

impl Query {
    const fn is_for(&self, brand: Brand) -> bool {
        matches!(
            (self.brands, brand),
            (Brands::Both, _) | (Brands::Denon, Brand::Denon) | (Brands::Marantz, Brand::Marantz)
        )
    }
}

const fn both(text: &'static str) -> Query {
    Query {
        text,
        brands: Brands::Both,
    }
}

const fn denon(text: &'static str) -> Query {
    Query {
        text,
        brands: Brands::Denon,
    }
}

const fn marantz(text: &'static str) -> Query {
    Query {
        text,
        brands: Brands::Marantz,
    }
}

/// Queries to send after connect, after a power query.
///
/// The receiver answers each query with the same events as for changes.
/// Brand-specific queries are in the place where they are sent.
pub fn state_queries(brand: Brand) -> impl Iterator<Item = &'static str> + Clone {
    QUERIES
        .iter()
        .filter(move |query| query.is_for(brand))
        .map(|query| query.text)
}

static QUERIES: [Query; 93] = [
    both("SI?"),                // Select INPUT source
    both("MV?"),                // MASTER VOLUME
    both("MU?"),                // Mute
    both("Z2?"),                // Z2 Zone Power
    both("Z2MU?"),              // Z2 Mute
    both("Z3?"),                // Z3 Zone Power
    both("Z3MU?"),              // Z3 Mute
    both("MS?"),                // Surround mode
    both("MNMEN?"),             // Menu
    marantz("MSSMART ?"),       // SMART Select
    denon("MSQUICK ?"),         // Quick Select
    both("TR?"),                // Trigger
    marantz("PSMDAX ?"),        // MDAX
    marantz("PSDACFIL ?"),      // DAC Filter
    marantz("ILB ?"),           // Illumination
    marantz("SSHOS ?"),         // Auto Lip Sync on a Marantz receiver, not tested
    both("PSTONE CTRL ?"),      // TONE
    both("PSDYNEQ ?"),          // Dynamic EQ
    both("PSLFC ?"),            // Audyssey LFC
    both("PSNEURAL ?"),         // Neural:X
    both("PSIMAXAUD ?"),        // IMAX Audio Settings Auto/Manual
    both("PSIMAXSWM ?"),        // IMAX Subwoofer Mode
    both("PSSWR ?"),            // Subwoofer
    both("SSTTR ?"),            // Tactile Transducer
    both("VSAUDIO ?"),          // HDMI Audio Decode
    both("PSCES ?"),            // CENTER Spread
    both("PSLOM ?"),            // Loudness Management
    both("PSCINEMA EQ. ?"),     // CINEMA EQ
    both("BTTX ?"),             // Bluetooth Transmitter
    both("PSSPV ?"),            // Speaker Virtualizer
    both("PSGEQ ?"),            // Graphic EQ
    both("PSHEQ ?"),            // Headphone EQ
    both("DIM ?"),              // Dimmer
    both("ECO?"),               // ECO
    both("SLP?"),               // Sleep
    both("VSMONI ?"),           // HDMI Output
    both("PSDELAY ?"),          // Audio Delay
    both("SSINFSIGRES ?"),      // Video Signal
    both("SSINFAISFSV ?"),      // Audio Sampling Rate
    both("SYSDA ?"),            // Audio Signal
    both("SYSMI ?"),            // Audio Sound
    both("OPINFASP ?"),         // Output Channels
    both("OPINFINS ?"),         // Input Channels
    both("SYSDVIN ?"),          // Video Signal Input
    both("SYSDVOUT ?"),         // Video Signal Output
    both("SYHDMIDIAGMAXRES ?"), // HDMI Max Resolution
    both("SSINFSIGHDR ?"),      // Video HDR Format
    both("SSINFSIGPIX ?"),      // Video Pixel Depth
    both("SSINFSIGFRL ?"),      // Video Max FRL
    both("SSINFSIGCOS ?"),      // Signal Color Space
    both("SSINFSIGECI ?"),      // Signal Extended Colorimetry
    both("PSDIRAC ?"),          // Dirac Live Filter
    both("OPDLS ?"),            // Dirac Live slots: name, filter and kind
    both("SSFUN ?"),            // Source names
    both("SSSOD ?"),            // Sources in use or hidden
    both("OPSMLALL ?"),         // Sound modes that the receiver offers
    both("NSFRN?"),             // Friendly name
    both("OPALS ?"),            // Auto Lip Sync on a Denon receiver
    denon("SSQS ?"),            // Quick select names
    denon("SSLEV ?"),           // Speaker channel levels
    both("PSREFLEV ?"),         // Reference Level
    both("PSDYNVOL ?"),         // Dynamic Vol.
    both("SPPR ?"),             // Speaker Preset
    both("PV?"),                // Picture Mode
    both("DC?"),                // Digital Codec
    both("PSBAS ?"),            // BASS
    both("PSTRE ?"),            // TREBLE
    both("PSCNTAMT ?"),         // Containment Amount
    both("PSMULTEQ: ?"),        // MultEQ
    both("CV?"),                // Channel Volume
    both("PSIMAX ?"),           // IMAX
    both("PSIMAXHPF ?"),        // IMAX High Pass Filter
    both("PSIMAXLPF ?"),        // IMAX Low Pass Filter
    both("PSIMAXSWO ?"),        // Subwoofer Output LFE+Main/LFE
    both("PSSWL ?"),            // Subwoofer Level
    both("STBY?"),              // Auto Standby
    both("Z2STBY?"),            // Auto Standby
    both("Z3STBY?"),            // Auto Standby
    both("VSVPM ?"),            // Video Process
    both("PSLFE ?"),            // LFE Level
    both("PSBSC ?"),            // Bass Sync
    both("PSDEH ?"),            // Dialog Enhancer
    both("PSAUROPR ?"),         // Auro-Matic Preset
    both("PSAUROST ?"),         // Auro-Matic Strength
    both("PSAUROMODE ?"),       // AURO-3D Mode
    both("PSRSZ ?"),            // ROOM SIZE
    both("PSDIC ?"),            // Dialog Control
    both("PSSP: ?"),            // Effect Speaker selection
    both("PSDRC ?"),            // DRC
    both("PSDEL ?"),            // DELAY TIME
    both("PSRSTR ?"),           // AUDIO RESTORER
    both("PSCLV ?"),            // Center level
    both("PSPAN ?"),            // Panorama
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
    fn brand_specific_queries() {
        let denon: Vec<_> = state_queries(Brand::Denon).collect();
        let marantz: Vec<_> = state_queries(Brand::Marantz).collect();
        assert!(denon.contains(&"MSQUICK ?"));
        assert!(!denon.contains(&"MSSMART ?"));
        assert!(marantz.contains(&"MSSMART ?"));
        for query in ["PSMDAX ?", "PSDACFIL ?", "ILB ?", "SSHOS ?"] {
            assert!(marantz.contains(&query) && !denon.contains(&query));
        }
        assert_eq!(denon.len(), 88);
        assert_eq!(marantz.len(), 90);
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
    fn both_brands_ask_for_the_sources_sound_modes_name_and_lip_sync() {
        for brand in BRANDS {
            for wanted in ["SSFUN ?", "SSSOD ?", "OPSMLALL ?", "NSFRN?", "OPALS ?"] {
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
    fn queries_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for query in &QUERIES {
            assert!(seen.insert(query.text), "{}", query.text);
        }
    }
}
