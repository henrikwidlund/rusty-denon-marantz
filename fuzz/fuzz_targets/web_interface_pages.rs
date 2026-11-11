//! The pages of the web interface, parsed and applied to the state.
#![no_main]

use denon_marantz_proto::{
    AudioChannelsPage, BassSettings, GeneralPage, GroupAvailability, InputAssignment, Playback2Ch,
    SettingsGroup, SourceLevels, SourceNames, SpeakerTable, State, SystemSettings, VideoMenuPage,
    VideoSignalPage, XlrAssign,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut state = State::new(false);
    if let Ok(table) = SpeakerTable::parse_distances(data) {
        let _ = state.apply_speaker_distances(table);
    }
    if let Ok(table) = SpeakerTable::parse_crossovers(data) {
        let _ = state.apply_speaker_crossovers(&table);
    }
    if let Ok(table) = SpeakerTable::parse_channel_levels(data) {
        let _ = state.apply_channel_level_table(&table);
    }
    if let Ok(bass) = BassSettings::parse(data) {
        let _ = state.apply_bass_settings(bass);
    }
    if let Ok(list) = InputAssignment::parse(data) {
        let _ = state.apply_input_assignments(&list);
    }
    if let Ok(names) = SourceNames::parse(data) {
        let _ = state.apply_web_source_names(&names);
    }
    for group in SettingsGroup::ALL.into_iter().chain(SettingsGroup::DETAILS) {
        if let Ok(group) = GroupAvailability::parse(group, data) {
            let _ = state.apply_settings_availability(group);
        }
    }
    if let Ok(settings) = SystemSettings::parse(data) {
        let _ = state.apply_system_settings(settings);
    }
    if let Ok(page) = VideoSignalPage::parse(data) {
        let _ = state.apply_video_signal_page(&page);
    }
    if let Ok(page) = AudioChannelsPage::parse(data) {
        let _ = state.apply_audio_channels_page(&page);
    }
    for page in VideoMenuPage::ALL {
        if let Ok(part) = page.parse(data) {
            let _ = state.apply_video_menu(part);
        }
    }
    for page in GeneralPage::ALL {
        if let Ok(part) = page.parse(data) {
            let _ = state.apply_general_settings(part);
        }
    }
    if let Ok(page) = Playback2Ch::parse(data) {
        let _ = state.apply_playback_2ch(page);
    }
    if let Ok(page) = XlrAssign::parse(data) {
        let _ = state.apply_xlr_assign(page);
    }
    if let Ok(levels) = SourceLevels::parse(data) {
        let _ = state.apply_source_levels(levels);
    }
});
