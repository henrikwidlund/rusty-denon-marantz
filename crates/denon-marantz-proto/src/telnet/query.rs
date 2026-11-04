//! Queries that ask a receiver for its state.

use crate::brand::Brand;

/// Asks for the power state of the main zone.
pub const MAIN_ZONE_POWER_QUERY: &str = "ZM?";

/// Asks for the power state of the whole device.
pub const DEVICE_POWER_QUERY: &str = "PW?";

/// Queries to send after connect, after a power query.
///
/// The receiver answers each query with the same events as for changes.
#[must_use]
pub fn state_queries(brand: Brand) -> &'static [&'static str] {
    match brand {
        Brand::Denon => &DENON,
        Brand::Marantz => &MARANTZ,
    }
}

/// Queries for Denon receivers.
static DENON: [&str; 77] = [
    "SI?",                // Select INPUT source
    "MV?",                // MASTER VOLUME
    "MU?",                // Mute
    "Z2?",                // Z2 Zone Power
    "Z2MU?",              // Z2 Mute
    "Z3?",                // Z3 Zone Power
    "Z3MU?",              // Z3 Mute
    "MS?",                // Surround mode
    "MNMEN?",             // Menu
    "MSQUICK ?",          // Quick Select
    "TR?",                // Trigger
    "PSTONE CTRL ?",      // TONE
    "PSDYNEQ ?",          // Dynamic EQ
    "PSLFC ?",            // Audyssey LFC
    "PSNEURAL ?",         // Neural:X
    "PSIMAXAUD ?",        // IMAX Audio Settings Auto/Manual
    "PSIMAXSWM ?",        // IMAX Subwoofer Mode
    "PSSWR ?",            // Subwoofer
    "SSTTR ?",            // Tactile Transducer
    "VSAUDIO ?",          // HDMI Audio Decode
    "PSCES ?",            // CENTER Spread
    "PSLOM ?",            // Loudness Management
    "PSCINEMA EQ. ?",     // CINEMA EQ
    "BTTX ?",             // Bluetooth Transmitter
    "PSSPV ?",            // Speaker Virtualizer
    "PSGEQ ?",            // Graphic EQ
    "PSHEQ ?",            // Headphone EQ
    "DIM ?",              // Dimmer
    "ECO?",               // ECO
    "SLP?",               // Sleep
    "VSMONI ?",           // HDMI Output
    "PSDELAY ?",          // Audio Delay
    "SSINFSIGRES ?",      // Video Signal
    "SSINFAISFSV ?",      // Audio Sampling Rate
    "SYSDA ?",            // Audio Signal
    "SYSMI ?",            // Audio Sound
    "OPINFASP ?",         // Output Channels
    "OPINFINS ?",         // Input Channels
    "SYSDVIN ?",          // Video Signal Input
    "SYSDVOUT ?",         // Video Signal Output
    "SYHDMIDIAGMAXRES ?", // HDMI Max Resolution
    "SSINFSIGHDR ?",      // Video HDR Format
    "SSINFSIGPIX ?",      // Video Pixel Depth
    "SSINFSIGFRL ?",      // Video Max FRL
    "SSINFSIGCOS ?",      // Signal Color Space
    "PSDIRAC ?",          // Dirac Live Filter
    "PSREFLEV ?",         // Reference Level
    "PSDYNVOL ?",         // Dynamic Vol.
    "SPPR ?",             // Speaker Preset
    "PV?",                // Picture Mode
    "DC?",                // Digital Codec
    "PSBAS ?",            // BASS
    "PSTRE ?",            // TREBLE
    "PSCNTAMT ?",         // Containment Amount
    "PSMULTEQ: ?",        // MultEQ
    "CV?",                // Channel Volume
    "PSIMAX ?",           // IMAX
    "PSIMAXHPF ?",        // IMAX High Pass Filter
    "PSIMAXLPF ?",        // IMAX Low Pass Filter
    "PSIMAXSWO ?",        // Subwoofer Output LFE+Main/LFE
    "PSSWL ?",            // Subwoofer Level
    "STBY?",              // Auto Standby
    "Z2STBY?",            // Auto Standby
    "Z3STBY?",            // Auto Standby
    "VSVPM ?",            // Video Process
    "PSLFE ?",            // LFE Level
    "PSBSC ?",            // Bass Sync
    "PSDEH ?",            // Dialog Enhancer
    "PSAUROPR ?",         // Auro-Matic Preset
    "PSAUROST ?",         // Auro-Matic Strength
    "PSAUROMODE ?",       // AURO-3D Mode
    "PSRSZ ?",            // ROOM SIZE
    "PSDIC ?",            // Dialog Control
    "PSSP: ?",            // Effect Speaker selection
    "PSDRC ?",            // DRC
    "PSDEL ?",            // DELAY TIME
    "PSRSTR ?",           // AUDIO RESTORER
];

/// Queries for Marantz receivers.
static MARANTZ: [&str; 81] = [
    "SI?",                // Select INPUT source
    "MV?",                // MASTER VOLUME
    "MU?",                // Mute
    "Z2?",                // Z2 Zone Power
    "Z2MU?",              // Z2 Mute
    "Z3?",                // Z3 Zone Power
    "Z3MU?",              // Z3 Mute
    "MS?",                // Surround mode
    "MNMEN?",             // Menu
    "MSSMART ?",          // SMART Select
    "TR?",                // Trigger
    "PSMDAX ?",           // MDAX
    "PSDACFIL ?",         // DAC Filter
    "ILB ?",              // Illumination
    "SSHOS ?",            // Auto Lip Sync
    "PSTONE CTRL ?",      // TONE
    "PSDYNEQ ?",          // Dynamic EQ
    "PSLFC ?",            // Audyssey LFC
    "PSNEURAL ?",         // Neural:X
    "PSIMAXAUD ?",        // IMAX Audio Settings Auto/Manual
    "PSIMAXSWM ?",        // IMAX Subwoofer Mode
    "PSSWR ?",            // Subwoofer
    "SSTTR ?",            // Tactile Transducer
    "VSAUDIO ?",          // HDMI Audio Decode
    "PSCES ?",            // CENTER Spread
    "PSLOM ?",            // Loudness Management
    "PSCINEMA EQ. ?",     // CINEMA EQ
    "BTTX ?",             // Bluetooth Transmitter
    "PSSPV ?",            // Speaker Virtualizer
    "PSGEQ ?",            // Graphic EQ
    "PSHEQ ?",            // Headphone EQ
    "DIM ?",              // Dimmer
    "ECO?",               // ECO
    "SLP?",               // Sleep
    "VSMONI ?",           // HDMI Output
    "PSDELAY ?",          // Audio Delay
    "SSINFSIGRES ?",      // Video Signal
    "SSINFAISFSV ?",      // Audio Sampling Rate
    "SYSDA ?",            // Audio Signal
    "SYSMI ?",            // Audio Sound
    "OPINFASP ?",         // Output Channels
    "OPINFINS ?",         // Input Channels
    "SYSDVIN ?",          // Video Signal Input
    "SYSDVOUT ?",         // Video Signal Output
    "SYHDMIDIAGMAXRES ?", // HDMI Max Resolution
    "SSINFSIGHDR ?",      // Video HDR Format
    "SSINFSIGPIX ?",      // Video Pixel Depth
    "SSINFSIGFRL ?",      // Video Max FRL
    "SSINFSIGCOS ?",      // Signal Color Space
    "PSDIRAC ?",          // Dirac Live Filter
    "PSREFLEV ?",         // Reference Level
    "PSDYNVOL ?",         // Dynamic Vol.
    "SPPR ?",             // Speaker Preset
    "PV?",                // Picture Mode
    "DC?",                // Digital Codec
    "PSBAS ?",            // BASS
    "PSTRE ?",            // TREBLE
    "PSCNTAMT ?",         // Containment Amount
    "PSMULTEQ: ?",        // MultEQ
    "CV?",                // Channel Volume
    "PSIMAX ?",           // IMAX
    "PSIMAXHPF ?",        // IMAX High Pass Filter
    "PSIMAXLPF ?",        // IMAX Low Pass Filter
    "PSIMAXSWO ?",        // Subwoofer Output LFE+Main/LFE
    "PSSWL ?",            // Subwoofer Level
    "STBY?",              // Auto Standby
    "Z2STBY?",            // Auto Standby
    "Z3STBY?",            // Auto Standby
    "VSVPM ?",            // Video Process
    "PSLFE ?",            // LFE Level
    "PSBSC ?",            // Bass Sync
    "PSDEH ?",            // Dialog Enhancer
    "PSAUROPR ?",         // Auro-Matic Preset
    "PSAUROST ?",         // Auro-Matic Strength
    "PSAUROMODE ?",       // AURO-3D Mode
    "PSRSZ ?",            // ROOM SIZE
    "PSDIC ?",            // Dialog Control
    "PSSP: ?",            // Effect Speaker selection
    "PSDRC ?",            // DRC
    "PSDEL ?",            // DELAY TIME
    "PSRSTR ?",           // AUDIO RESTORER
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_query_asks_for_a_value() {
        for brand in [Brand::Denon, Brand::Marantz] {
            for query in state_queries(brand) {
                assert!(query.ends_with('?'), "{query}");
            }
        }
    }

    #[test]
    fn brand_specific_queries() {
        let denon = state_queries(Brand::Denon);
        let marantz = state_queries(Brand::Marantz);
        assert!(denon.contains(&"MSQUICK ?"));
        assert!(!denon.contains(&"MSSMART ?"));
        assert!(marantz.contains(&"MSSMART ?"));
        for query in ["PSMDAX ?", "PSDACFIL ?", "ILB ?", "SSHOS ?"] {
            assert!(marantz.contains(&query) && !denon.contains(&query));
        }
        assert_eq!(denon.len(), 77);
        assert_eq!(marantz.len(), 81);
    }

    #[test]
    fn queries_are_unique() {
        for brand in [Brand::Denon, Brand::Marantz] {
            let mut seen = std::collections::HashSet::new();
            for query in state_queries(brand) {
                assert!(seen.insert(*query), "{query}");
            }
        }
    }
}
