//! Output shared by the examples.

use denon_marantz_heos::{Change, Heos};

/// Prints each player with its state.
pub(crate) fn print_players(heos: &Heos) {
    for state in heos.players() {
        println!(
            "player  {} {}: {:?} {:?} {:?}",
            state.pid(),
            state.player.name,
            state.play_state,
            state.play_mode(),
            state.current_media()
        );
    }
}

/// Prints a change with the new value from the state.
pub(crate) fn print_change(heos: &Heos, change: &Change) {
    let state = match change {
        Change::PlayState(pid)
        | Change::NowPlaying(pid)
        | Change::Progress(pid)
        | Change::PlayMode(pid) => heos.player(*pid),
        _ => None,
    };
    match (change, state) {
        (Change::PlayState(_), Some(state)) => {
            println!("state   {:?} {:?}", state.play_state, state.current_media());
        }
        (Change::NowPlaying(_), Some(state)) => {
            println!("media   {:?} {:?}", state.current_media(), state.controls());
        }
        (Change::Progress(_), Some(state)) => println!("time    {:?}", state.progress),
        (Change::PlayMode(_), Some(state)) => println!("mode    {:?}", state.play_mode()),
        _ => println!("change  {change:?}"),
    }
}
