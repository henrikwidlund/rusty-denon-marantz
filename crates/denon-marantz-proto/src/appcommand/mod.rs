//! `AppCommand.xml` and `AppCommand0300.xml`: XML over HTTP POST.
//!
//! A request has up to five commands for each `tx` element ([`write_body`]).
//! The answer has one `cmd` or `error` element for each command, in the same
//! order ([`Response`]). The answer does not name the command, so a command
//! [`Definition`] describes where its values are ([`Pattern`]).

mod commands;
mod pattern;
mod request;
mod response;
mod settings;

pub use commands::*;
pub(crate) use pattern::find;
pub use pattern::{Definition, Field, Leaf, Pattern, Step};
pub use request::{Cmd, Endpoint, Param, write_body};
pub use response::{Item, Response};
pub use settings::{
    SettingsPart, settings_lines, settings_parts_of_command, write_settings_request,
};
