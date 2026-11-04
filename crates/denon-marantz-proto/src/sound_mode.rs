//! Sound mode groups and the raw modes reported by receivers.

code_enum! {
    /// Sound mode group. The code is the name sent to select the mode.
    pub enum SoundModeGroup {
        Music = "MUSIC", "MUSIC";
        Movie = "MOVIE", "MOVIE";
        Game = "GAME", "GAME";
        Auto = "AUTO", "AUTO";
        Standard = "STANDARD", "STANDARD";
        Virtual = "VIRTUAL", "VIRTUAL";
        Matrix = "MATRIX", "MATRIX";
        RockArena = "ROCK ARENA", "ROCK ARENA";
        JazzClub = "JAZZ CLUB", "JAZZ CLUB";
        VideoGame = "VIDEO GAME", "VIDEO GAME";
        MonoMovie = "MONO MOVIE", "MONO MOVIE";
        SuperStadium = "SUPER STADIUM", "SUPER STADIUM";
        ClassicConcert = "CLASSIC CONCERT", "CLASSIC CONCERT";
        WideScreen = "WIDE SCREEN", "WIDE SCREEN";
        Direct = "DIRECT", "DIRECT";
        PureDirect = "PURE DIRECT", "PURE DIRECT";
        DolbyDigital = "DOLBY DIGITAL", "DOLBY DIGITAL";
        DtsSurround = "DTS SURROUND", "DTS SURROUND";
        Auro3d = "AURO3D", "AURO3D";
        Auro2dSurround = "AURO2DSURR", "AURO2DSURR";
        McHStereo = "MCH STEREO", "MCH STEREO";
        Stereo = "STEREO", "STEREO";
        AllZoneStereo = "ALL ZONE STEREO", "ALL ZONE STEREO";
    }
}

/// Raw sound modes in upper case with single spaces.
static RAW_MODES: [(&str, SoundModeGroup); 206] = [
    ("AAC+NEO:X M", SoundModeGroup::Music),
    ("AAC+PL2X M", SoundModeGroup::Music),
    ("DOLBY D +NEO:X M", SoundModeGroup::Music),
    ("DOLBY D+ +NEO:X M", SoundModeGroup::Music),
    ("DOLBY D+ +PL2X M", SoundModeGroup::Music),
    ("DOLBY D+NEO:X M", SoundModeGroup::Music),
    ("DOLBY D+PL2X M", SoundModeGroup::Music),
    ("DOLBY HD+NEO:X M", SoundModeGroup::Music),
    ("DOLBY HD+PL2X M", SoundModeGroup::Music),
    ("DOLBY PL2 M", SoundModeGroup::Music),
    ("DOLBY PL2 MUSIC", SoundModeGroup::Music),
    ("DOLBY PL2 X MUSIC", SoundModeGroup::Music),
    ("DOLBY PL2X M", SoundModeGroup::Music),
    ("DOLBY PLII MS", SoundModeGroup::Music),
    ("DTS HD+NEO:X M", SoundModeGroup::Music),
    ("DTS HD+PL2X M", SoundModeGroup::Music),
    ("DTS NEO:6 M", SoundModeGroup::Music),
    ("DTS NEO:6 MUSIC", SoundModeGroup::Music),
    ("DTS NEO:X M", SoundModeGroup::Music),
    ("DTS NEO:X MUSIC", SoundModeGroup::Music),
    ("DTS+NEO:X M", SoundModeGroup::Music),
    ("DTS+PL2X M", SoundModeGroup::Music),
    ("M CH IN+NEO:X M", SoundModeGroup::Music),
    ("M CH IN+PL2X M", SoundModeGroup::Music),
    ("PLII MUSIC", SoundModeGroup::Music),
    ("PLIIX MUSIC", SoundModeGroup::Music),
    ("AAC+NEO:X C", SoundModeGroup::Movie),
    ("AAC+PL2X C", SoundModeGroup::Movie),
    ("DOLBY D +NEO:X C", SoundModeGroup::Movie),
    ("DOLBY D+ +NEO:X C", SoundModeGroup::Movie),
    ("DOLBY D+ +PL2X C", SoundModeGroup::Movie),
    ("DOLBY D+NEO:X C", SoundModeGroup::Movie),
    ("DOLBY D+PL2X C", SoundModeGroup::Movie),
    ("DOLBY HD+NEO:X C", SoundModeGroup::Movie),
    ("DOLBY HD+PL2X C", SoundModeGroup::Movie),
    ("DOLBY PL2 C", SoundModeGroup::Movie),
    ("DOLBY PL2 CINEMA", SoundModeGroup::Movie),
    ("DOLBY PL2 MOVIE", SoundModeGroup::Movie),
    ("DOLBY PL2 X MOVIE", SoundModeGroup::Movie),
    ("DOLBY PL2X C", SoundModeGroup::Movie),
    ("DOLBY PLII MOVIE", SoundModeGroup::Movie),
    ("DOLBY PLII MV", SoundModeGroup::Movie),
    ("DTS HD+NEO:X C", SoundModeGroup::Movie),
    ("DTS HD+PL2X C", SoundModeGroup::Movie),
    ("DTS NEO:6 C", SoundModeGroup::Movie),
    ("DTS NEO:6 CINEMA", SoundModeGroup::Movie),
    ("DTS NEO:X C", SoundModeGroup::Movie),
    ("DTS NEO:X CINEMA", SoundModeGroup::Movie),
    ("DTS+NEO:X C", SoundModeGroup::Movie),
    ("DTS+PL2X C", SoundModeGroup::Movie),
    ("M CH IN+NEO:X C", SoundModeGroup::Movie),
    ("M CH IN+PL2X C", SoundModeGroup::Movie),
    ("MULTI IN + VIRTUAL:X", SoundModeGroup::Movie),
    ("PL2 C DSX", SoundModeGroup::Movie),
    ("PLII CINEMA", SoundModeGroup::Movie),
    ("PLII MOVIE", SoundModeGroup::Movie),
    ("PLIIX CINEMA", SoundModeGroup::Movie),
    ("AAC+NEO:X G", SoundModeGroup::Game),
    ("DOLBY D +NEO:X G", SoundModeGroup::Game),
    ("DOLBY D+ +NEO:X G", SoundModeGroup::Game),
    ("DOLBY D+NEO:X G", SoundModeGroup::Game),
    ("DOLBY HD+NEO:X G", SoundModeGroup::Game),
    ("DOLBY PL2 G", SoundModeGroup::Game),
    ("DOLBY PL2 GAME", SoundModeGroup::Game),
    ("DOLBY PL2 X GAME", SoundModeGroup::Game),
    ("DOLBY PL2X G", SoundModeGroup::Game),
    ("DOLBY PLII GAME", SoundModeGroup::Game),
    ("DOLBY PLII GM", SoundModeGroup::Game),
    ("DTS HD+NEO:X G", SoundModeGroup::Game),
    ("DTS NEO:X G", SoundModeGroup::Game),
    ("DTS+NEO:X G", SoundModeGroup::Game),
    ("M CH IN+NEO:X G", SoundModeGroup::Game),
    ("PLII GAME", SoundModeGroup::Game),
    ("NONE", SoundModeGroup::Auto),
    ("NONE2", SoundModeGroup::Standard),
    ("VIRTUAL", SoundModeGroup::Virtual),
    ("MATRIX", SoundModeGroup::Matrix),
    ("ROCK ARENA", SoundModeGroup::RockArena),
    ("JAZZ CLUB", SoundModeGroup::JazzClub),
    ("VIDEO GAME", SoundModeGroup::VideoGame),
    ("MONO MOVIE", SoundModeGroup::MonoMovie),
    ("SUPER STADIUM", SoundModeGroup::SuperStadium),
    ("CLASSIC CONCERT", SoundModeGroup::ClassicConcert),
    ("WIDE SCREEN", SoundModeGroup::WideScreen),
    ("DIRECT", SoundModeGroup::Direct),
    ("PURE_DIRECT", SoundModeGroup::PureDirect),
    ("PURE DIRECT", SoundModeGroup::PureDirect),
    ("AAC+DOLBY EX", SoundModeGroup::DolbyDigital),
    ("AAC+DSUR", SoundModeGroup::DolbyDigital),
    ("AAC+PL2Z H", SoundModeGroup::DolbyDigital),
    ("DOLBY ATMOS", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO - DD + DSUR", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO - DD + NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO - DD+ + DSUR", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO - DD+ + NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO - DOLBY DIGITAL", SoundModeGroup::DolbyDigital),
    (
        "DOLBY AUDIO - DOLBY DIGITAL PLUS",
        SoundModeGroup::DolbyDigital,
    ),
    ("DOLBY AUDIO - DOLBY SURROUND", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO - DOLBY TRUEHD", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO - TRUEHD + DSUR", SoundModeGroup::DolbyDigital),
    (
        "DOLBY AUDIO - TRUEHD + NEURAL:X",
        SoundModeGroup::DolbyDigital,
    ),
    ("DOLBY AUDIO-DD", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-DD+", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-DD+ +DSUR", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-DD+ +NERUAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-DD+ +NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-DD+DSUR", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-DD+NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-DSUR", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-TRUEHD", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-TRUEHD+DSUR", SoundModeGroup::DolbyDigital),
    ("DOLBY AUDIO-TRUEHD+NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY D + +DOLBY SURROUND", SoundModeGroup::DolbyDigital),
    ("DOLBY D + DOLBY SURROUND", SoundModeGroup::DolbyDigital),
    ("DOLBY D + NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY D EX", SoundModeGroup::DolbyDigital),
    ("DOLBY D+", SoundModeGroup::DolbyDigital),
    ("DOLBY D+ +DS", SoundModeGroup::DolbyDigital),
    ("DOLBY D+ +EX", SoundModeGroup::DolbyDigital),
    ("DOLBY D+ +PL2Z H", SoundModeGroup::DolbyDigital),
    ("DOLBY D+DS", SoundModeGroup::DolbyDigital),
    ("DOLBY D+NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY D+PL2Z H", SoundModeGroup::DolbyDigital),
    ("DOLBY DIGITAL", SoundModeGroup::DolbyDigital),
    ("DOLBY DIGITAL +", SoundModeGroup::DolbyDigital),
    ("DOLBY DIGITAL + + NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY DIGITAL + NEURAL:X", SoundModeGroup::DolbyDigital),
    ("DOLBY HD", SoundModeGroup::DolbyDigital),
    ("DOLBY HD + DOLBY SURROUND", SoundModeGroup::DolbyDigital),
    ("DOLBY HD+EX", SoundModeGroup::DolbyDigital),
    ("DOLBY HD+PL2Z H", SoundModeGroup::DolbyDigital),
    ("DOLBY PL2Z H", SoundModeGroup::DolbyDigital),
    ("DOLBY PRO LOGIC", SoundModeGroup::DolbyDigital),
    ("DOLBY SURROUND", SoundModeGroup::DolbyDigital),
    ("DOLBY TRUEHD", SoundModeGroup::DolbyDigital),
    ("M CH IN+DOLBY EX", SoundModeGroup::DolbyDigital),
    ("M CH IN+DSUR", SoundModeGroup::DolbyDigital),
    ("M CH IN+PL2Z H", SoundModeGroup::DolbyDigital),
    ("MPEG-H", SoundModeGroup::DolbyDigital),
    ("MPEG2 AAC", SoundModeGroup::DolbyDigital),
    ("MPEG4 AAC", SoundModeGroup::DolbyDigital),
    ("MULTI IN + DOLBY SURROUND", SoundModeGroup::DolbyDigital),
    ("MULTI IN + DSUR", SoundModeGroup::DolbyDigital),
    ("MULTI IN + NEURAL:X", SoundModeGroup::DolbyDigital),
    ("NEURAL", SoundModeGroup::DolbyDigital),
    ("STANDARD(DOLBY)", SoundModeGroup::DolbyDigital),
    ("AAC+NEURAL:X", SoundModeGroup::DtsSurround),
    ("AAC+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("DTS + DOLBY SURROUND", SoundModeGroup::DtsSurround),
    ("DTS + NEURAL:X", SoundModeGroup::DtsSurround),
    ("DTS + VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("DTS ES 8CH DSCRT", SoundModeGroup::DtsSurround),
    ("DTS ES DSCRT+NEURAL:X", SoundModeGroup::DtsSurround),
    ("DTS ES DSCRT6.1", SoundModeGroup::DtsSurround),
    ("DTS ES MTRX+NEURAL:X", SoundModeGroup::DtsSurround),
    ("DTS ES MTRX6.1", SoundModeGroup::DtsSurround),
    ("DTS EXPRESS", SoundModeGroup::DtsSurround),
    ("DTS HD", SoundModeGroup::DtsSurround),
    ("DTS HD MSTR", SoundModeGroup::DtsSurround),
    ("DTS HD+DSUR", SoundModeGroup::DtsSurround),
    ("DTS HD+NEO:6", SoundModeGroup::DtsSurround),
    ("DTS HD+NEURAL:X", SoundModeGroup::DtsSurround),
    ("DTS HD+PL2Z H", SoundModeGroup::DtsSurround),
    ("DTS HD+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("DTS NEURAL:X", SoundModeGroup::DtsSurround),
    ("DTS SURROUND", SoundModeGroup::DtsSurround),
    ("DTS VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("DTS-HD", SoundModeGroup::DtsSurround),
    ("DTS-HD + DOLBY SURROUND", SoundModeGroup::DtsSurround),
    ("DTS-HD + DSUR", SoundModeGroup::DtsSurround),
    ("DTS-HD + NEURAL:X", SoundModeGroup::DtsSurround),
    ("DTS-HD MSTR", SoundModeGroup::DtsSurround),
    ("DTS:X", SoundModeGroup::DtsSurround),
    ("DTS:X MSTR", SoundModeGroup::DtsSurround),
    ("DTS:X+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("DTS+DSUR", SoundModeGroup::DtsSurround),
    ("DTS+NEO:6", SoundModeGroup::DtsSurround),
    ("DTS+NEUR", SoundModeGroup::DtsSurround),
    ("DTS+NEURAL:X", SoundModeGroup::DtsSurround),
    ("DTS+PL2Z H", SoundModeGroup::DtsSurround),
    ("DTS+VIRTU", SoundModeGroup::DtsSurround),
    ("DTS+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("DTS96 ES MTRX", SoundModeGroup::DtsSurround),
    ("DTS96/24", SoundModeGroup::DtsSurround),
    ("IMAX DTS", SoundModeGroup::DtsSurround),
    ("IMAX DTS:X", SoundModeGroup::DtsSurround),
    ("IMAX DTS:X+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("IMAX DTS+NEURAL:X", SoundModeGroup::DtsSurround),
    ("IMAX DTS+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("M CH IN+NEURAL:X", SoundModeGroup::DtsSurround),
    ("M CH IN+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("MAX DTS+VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("MULTI CH IN", SoundModeGroup::DtsSurround),
    ("MULTI CH IN 7.1", SoundModeGroup::DtsSurround),
    ("NEURAL:X", SoundModeGroup::DtsSurround),
    ("STANDARD(DTS)", SoundModeGroup::DtsSurround),
    ("VIRTUAL:X", SoundModeGroup::DtsSurround),
    ("AURO-3D", SoundModeGroup::Auro3d),
    ("AURO3D", SoundModeGroup::Auro3d),
    ("AURO-2D SURROUND", SoundModeGroup::Auro2dSurround),
    ("AURO2DSURR", SoundModeGroup::Auro2dSurround),
    ("MCH STEREO", SoundModeGroup::McHStereo),
    ("MULTI CH STEREO", SoundModeGroup::McHStereo),
    ("MULTI_CH_STEREO", SoundModeGroup::McHStereo),
    ("STEREO", SoundModeGroup::Stereo),
    ("ALL ZONE STEREO", SoundModeGroup::AllZoneStereo),
];

impl SoundModeGroup {
    /// Finds the group of a raw sound mode reported by the receiver.
    #[must_use]
    pub fn from_raw(raw: &str) -> Option<Self> {
        RAW_MODES
            .iter()
            .find(|(mode, _)| words_eq_ignore_case(mode, raw))
            .map(|&(_, group)| group)
    }

    /// Raw sound modes known for this group.
    pub fn raw_modes(self) -> impl Iterator<Item = &'static str> {
        RAW_MODES
            .iter()
            .filter(move |&&(_, group)| group == self)
            .map(|&(mode, _)| mode)
    }

    /// Guesses the group of an unknown raw sound mode from words in its name.
    #[must_use]
    pub fn guess_from_raw(raw: &str) -> Option<Self> {
        let has = |word: &str| {
            raw.as_bytes()
                .windows(word.len())
                .any(|w| w.eq_ignore_ascii_case(word.as_bytes()))
        };
        if has("DTS") {
            Some(Self::DtsSurround)
        } else if has("DOLBY") {
            Some(Self::DolbyDigital)
        } else if has("MUSIC") {
            Some(Self::Music)
        } else if has("AURO") {
            Some(Self::Auro3d)
        } else if has("MOVIE") || has("CINEMA") {
            Some(Self::Movie)
        } else {
            None
        }
    }
}

/// Compares ignoring ASCII case and treating runs of white space as one space.
fn words_eq_ignore_case(expected: &str, actual: &str) -> bool {
    let mut expected = expected.split(' ');
    let mut actual = actual.split_ascii_whitespace();
    loop {
        match (expected.next(), actual.next()) {
            (Some(e), Some(a)) if e.eq_ignore_ascii_case(a) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_round_trip() {
        for &group in SoundModeGroup::ALL {
            assert_eq!(SoundModeGroup::from_code(group.code()), Some(group));
        }
        assert_eq!(SoundModeGroup::ALL.len(), 23);
    }

    #[test]
    fn every_raw_mode_finds_its_group() {
        for &(mode, group) in &RAW_MODES {
            assert_eq!(SoundModeGroup::from_raw(mode), Some(group), "{mode}");
            assert!(group.raw_modes().any(|m| m == mode));
        }
    }

    #[test]
    fn raw_mode_known_answers() {
        use SoundModeGroup as G;
        assert_eq!(G::from_raw("DOLBY ATMOS"), Some(G::DolbyDigital));
        assert_eq!(G::from_raw("DTS:X MSTR"), Some(G::DtsSurround));
        assert_eq!(G::from_raw("PURE_DIRECT"), Some(G::PureDirect));
        assert_eq!(G::from_raw("MULTI_CH_STEREO"), Some(G::McHStereo));
        assert_eq!(G::from_raw("PL2 C DSX"), Some(G::Movie));
        assert_eq!(G::from_raw("STEREO"), Some(G::Stereo));
        assert_eq!(G::from_raw("AAC+NEURAL:X"), Some(G::DtsSurround));
        assert_eq!(G::from_raw("MULTI CH IN"), Some(G::DtsSurround));
    }

    #[test]
    fn raw_mode_matching_ignores_case_and_extra_spaces() {
        use SoundModeGroup as G;
        assert_eq!(
            G::from_raw("dolby audio - dd+   + dsur"),
            Some(G::DolbyDigital)
        );
        assert_eq!(G::from_raw("  Jazz  Club "), Some(G::JazzClub));
        assert_eq!(G::from_raw("None"), Some(G::Auto));
        assert_eq!(G::from_raw("NONE2"), Some(G::Standard));
        assert_eq!(G::from_raw(""), None);
        assert_eq!(G::from_raw("JAZZ"), None);
        assert_eq!(G::from_raw("JAZZ CLUB X"), None);
    }

    #[test]
    fn guess_uses_the_same_order_as_python() {
        use SoundModeGroup as G;
        assert_eq!(G::guess_from_raw("NEW DTS DOLBY"), Some(G::DtsSurround));
        assert_eq!(G::guess_from_raw("new dolby thing"), Some(G::DolbyDigital));
        assert_eq!(G::guess_from_raw("MY MUSIC MOVIE"), Some(G::Music));
        assert_eq!(G::guess_from_raw("AURO X"), Some(G::Auro3d));
        assert_eq!(G::guess_from_raw("NEW CINEMA"), Some(G::Movie));
        assert_eq!(G::guess_from_raw("SOMETHING"), None);
        assert_eq!(G::guess_from_raw(""), None);
    }
}
