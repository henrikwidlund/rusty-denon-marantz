//! Commands for the network audio of older receivers.

use core::fmt::Write as _;

use crate::error::Error;

/// Highest preset number of network audio.
pub(crate) const MAX_NETWORK_PRESET: u8 = 56;

/// Repeat mode of network audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RepeatMode {
    /// Repeats the title.
    One,
    /// Repeats all titles.
    All,
    /// Does not repeat.
    Off,
}

/// A command for the network audio of older receivers.
///
/// Network audio is for example internet radio, a media server or USB.
/// They are for receivers without HEOS, with a `CommApiVers` below 0301 in
/// `Deviceinfo.xml`. Receivers with HEOS may ignore them. Play, pause and
/// stop are [`Command::Play`](crate::telnet::Command::Play) and the commands after
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NetworkAudioCommand {
    /// Goes to the next title.
    Next,
    /// Goes to the previous title.
    Previous,
    /// Sets the repeat mode.
    Repeat(RepeatMode),
    /// Turns random play on (`true`) or off (`false`).
    Random(bool),
    /// Switches to the next repeat mode.
    RepeatToggle,
    /// Turns random play on or off.
    RandomToggle,
    /// Moves the cursor up.
    CursorUp,
    /// Moves the cursor down.
    CursorDown,
    /// Moves the cursor left.
    CursorLeft,
    /// Moves the cursor right.
    CursorRight,
    /// Confirms. Plays or pauses on some sources.
    CursorEnter,
    /// Shows the next page.
    PageNext,
    /// Shows the previous page.
    PagePrevious,
    /// Goes to a preset, from 1 to 56. Denon receivers of 2014 and 2015
    /// have 36 presets.
    Preset(u8),
    /// Stores what plays in a preset, from 1 to 56. Denon receivers of 2014
    /// and 2015 have 36 presets.
    StorePreset(u8),
    /// Asks for the names of all presets. The answers fill
    /// [`DeviceState::network_preset_names`](crate::DeviceState::network_preset_names).
    PresetNames,
}

impl NetworkAudioCommand {
    /// Appends the command text, without the line end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ValueOutOfRange`] for a preset that is not from 1 to
    /// 56. `out` is not changed then.
    pub(crate) fn write(self, out: &mut String) -> Result<(), Error> {
        let text = match self {
            Self::Next => "NS9D",
            Self::Previous => "NS9E",
            Self::Repeat(RepeatMode::One) => "NS9H",
            Self::Repeat(RepeatMode::All) => "NS9I",
            Self::Repeat(RepeatMode::Off) => "NS9J",
            Self::Random(true) => "NS9K",
            Self::Random(false) => "NS9M",
            Self::RepeatToggle => "NSRPT",
            Self::RandomToggle => "NSRND",
            Self::CursorUp => "NS90",
            Self::CursorDown => "NS91",
            Self::CursorLeft => "NS92",
            Self::CursorRight => "NS93",
            Self::CursorEnter => "NS94",
            Self::PageNext => "NS9X",
            Self::PagePrevious => "NS9Y",
            Self::PresetNames => "NSH",
            Self::Preset(number) => return write_preset(out, "NSB", number),
            Self::StorePreset(number) => return write_preset(out, "NSC", number),
        };
        out.push_str(text);
        Ok(())
    }
}

/// The receiver numbers the presets from 00, so preset 1 is `00`.
fn write_preset(out: &mut String, start: &str, number: u8) -> Result<(), Error> {
    if !(1..=MAX_NETWORK_PRESET).contains(&number) {
        return Err(Error::ValueOutOfRange {
            what: "network audio preset",
        });
    }
    out.push_str(start);
    // Writing to a `String` cannot fail.
    let _ = write!(out, "{:02}", number - 1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(command: NetworkAudioCommand) -> Result<String, Error> {
        let mut out = String::new();
        command.write(&mut out).map(|()| out)
    }

    #[test]
    fn texts() {
        use NetworkAudioCommand as N;
        let cases = [
            (N::Next, "NS9D"),
            (N::Previous, "NS9E"),
            (N::Repeat(RepeatMode::One), "NS9H"),
            (N::Repeat(RepeatMode::All), "NS9I"),
            (N::Repeat(RepeatMode::Off), "NS9J"),
            (N::Random(true), "NS9K"),
            (N::Random(false), "NS9M"),
            (N::RepeatToggle, "NSRPT"),
            (N::RandomToggle, "NSRND"),
            (N::CursorUp, "NS90"),
            (N::CursorDown, "NS91"),
            (N::CursorLeft, "NS92"),
            (N::CursorRight, "NS93"),
            (N::CursorEnter, "NS94"),
            (N::PageNext, "NS9X"),
            (N::PagePrevious, "NS9Y"),
            (N::PresetNames, "NSH"),
            (N::Preset(1), "NSB00"),
            (N::Preset(56), "NSB55"),
            (N::StorePreset(1), "NSC00"),
            (N::StorePreset(56), "NSC55"),
        ];
        for (command, expected) in cases {
            assert_eq!(text(command).as_deref(), Ok(expected), "{command:?}");
        }
    }

    #[test]
    fn presets_are_from_1_to_56() {
        for number in [0, 57, 255] {
            for command in [
                NetworkAudioCommand::Preset(number),
                NetworkAudioCommand::StorePreset(number),
            ] {
                let mut out = String::from("x");
                assert_eq!(
                    command.write(&mut out),
                    Err(Error::ValueOutOfRange {
                        what: "network audio preset"
                    })
                );
                assert_eq!(out, "x");
            }
        }
    }
}
