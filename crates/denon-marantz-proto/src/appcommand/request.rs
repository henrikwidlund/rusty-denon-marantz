//! Requests to the AppCommand endpoints.

use crate::error::Error;

/// A receiver does not accept more commands than this in one `tx` element.
const COMMANDS_PER_ROOT: usize = 5;

/// HTTP endpoint of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Endpoint {
    /// `/goform/AppCommand.xml`, command id 1.
    AppCommand,
    /// `/goform/AppCommand0300.xml`, command id 3.
    AppCommand0300,
}

impl Endpoint {
    /// Command id in the XML.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::AppCommand => "1",
            Self::AppCommand0300 => "3",
        }
    }

    /// URL path.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::AppCommand => "/goform/AppCommand.xml",
            Self::AppCommand0300 => "/goform/AppCommand0300.xml",
        }
    }
}

/// A `param` element of a command, or the element of a set command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param<'a> {
    /// Name. It is an attribute of a `param` element, or the element name of a set command.
    pub name: &'a str,
    /// Text. Can be empty.
    pub text: &'a str,
}

/// A command in a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cmd<'a> {
    /// Endpoint. All commands of one request must have the same endpoint.
    pub endpoint: Endpoint,
    /// Text of the `cmd` element.
    pub text: Option<&'a str>,
    /// Text of the `name` element.
    pub name: Option<&'a str>,
    /// Parameters in a `list` element.
    pub params: Option<&'a [Param<'a>]>,
    /// Element after the `cmd` element, used to set a value.
    pub set: Option<Param<'a>>,
}

impl<'a> Cmd<'a> {
    /// A query for `AppCommand.xml`, for example `GetAllZonePowerStatus`.
    #[must_use]
    pub const fn get(text: &'a str) -> Self {
        Self {
            endpoint: Endpoint::AppCommand,
            text: Some(text),
            name: None,
            params: None,
            set: None,
        }
    }

    /// Sets one tone control value. The name is for example `bassvalue`.
    #[must_use]
    pub const fn set_tone_control(parameter: Param<'a>) -> Self {
        Self {
            endpoint: Endpoint::AppCommand,
            text: Some("SetToneControl"),
            name: None,
            params: None,
            set: Some(parameter),
        }
    }

    /// Sets Audyssey values.
    #[must_use]
    pub const fn set_audyssey(params: &'a [Param<'a>]) -> Self {
        Self {
            endpoint: Endpoint::AppCommand0300,
            text: None,
            name: Some("SetAudyssey"),
            params: Some(params),
            set: None,
        }
    }
}

/// Appends the body of a POST request to `out`.
///
/// Each group of five commands is a `tx` element. The receiver accepts several
/// root elements in one body, but not more than five commands in one.
/// `out` is not changed if this returns an error.
///
/// # Errors
///
/// Returns [`Error::InvalidCommandText`] if text has a control character, or
/// [`Error::InvalidName`] if the name of a set command is not an XML name.
pub fn write_body(commands: &[Cmd<'_>], out: &mut Vec<u8>) -> Result<(), Error> {
    let start = out.len();
    let result = write_all(commands, out);
    if result.is_err() {
        out.truncate(start);
    }
    result
}

fn write_all(commands: &[Cmd<'_>], out: &mut Vec<u8>) -> Result<(), Error> {
    for (index, chunk) in commands.chunks(COMMANDS_PER_ROOT).enumerate() {
        if index == 0 {
            out.extend_from_slice(b"<?xml version='1.0' encoding='utf-8'?>\n");
        }
        out.extend_from_slice(b"<tx>");
        for cmd in chunk {
            write_cmd(cmd, out)?;
        }
        out.extend_from_slice(b"</tx>");
    }
    Ok(())
}

fn write_cmd(cmd: &Cmd<'_>, out: &mut Vec<u8>) -> Result<(), Error> {
    out.extend_from_slice(b"<cmd id=\"");
    out.extend_from_slice(cmd.endpoint.id().as_bytes());
    out.extend_from_slice(b"\">");
    if let Some(text) = cmd.text {
        escape(text, out)?;
    }
    if let Some(name) = cmd.name {
        out.extend_from_slice(b"<name>");
        escape(name, out)?;
        out.extend_from_slice(b"</name>");
    }
    if let Some(params) = cmd.params {
        if params.is_empty() {
            out.extend_from_slice(b"<list />");
        } else {
            out.extend_from_slice(b"<list>");
            for param in params {
                out.extend_from_slice(b"<param name=\"");
                escape(param.name, out)?;
                if param.text.is_empty() {
                    out.extend_from_slice(b"\" />");
                } else {
                    out.extend_from_slice(b"\">");
                    escape(param.text, out)?;
                    out.extend_from_slice(b"</param>");
                }
            }
            out.extend_from_slice(b"</list>");
        }
    }
    out.extend_from_slice(b"</cmd>");
    if let Some(set) = cmd.set {
        check_name(set.name)?;
        out.push(b'<');
        out.extend_from_slice(set.name.as_bytes());
        if set.text.is_empty() {
            out.extend_from_slice(b" />");
        } else {
            out.push(b'>');
            escape(set.text, out)?;
            out.extend_from_slice(b"</");
            out.extend_from_slice(set.name.as_bytes());
            out.push(b'>');
        }
    }
    Ok(())
}

/// Appends text with the characters `& < > "` replaced.
fn escape(text: &str, out: &mut Vec<u8>) -> Result<(), Error> {
    if text.chars().any(char::is_control) {
        return Err(Error::InvalidCommandText);
    }
    for byte in text.bytes() {
        match byte {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            _ => out.push(byte),
        }
    }
    Ok(())
}

/// Accepts the ASCII subset of XML names that receivers use.
fn check_name(name: &str) -> Result<(), Error> {
    let mut bytes = name.bytes();
    let valid = bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidName)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(commands: &[Cmd<'_>]) -> String {
        let mut out = Vec::new();
        write_body(commands, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn single_query() {
        assert_eq!(
            body(&[Cmd::get("GetAllZonePowerStatus")]),
            "<?xml version='1.0' encoding='utf-8'?>\n<tx><cmd id=\"1\">GetAllZonePowerStatus</cmd></tx>"
        );
    }

    #[test]
    fn empty_request_is_empty() {
        assert_eq!(body(&[]), "");
    }

    #[test]
    fn five_commands_per_root_and_one_declaration() {
        let commands = [Cmd::get("A"); 11];
        let xml = body(&commands);
        assert_eq!(xml.matches("<?xml").count(), 1);
        assert_eq!(xml.matches("<tx>").count(), 3);
        assert_eq!(xml.matches("</tx>").count(), 3);
        assert_eq!(xml.matches("<cmd id=\"1\">A</cmd>").count(), 11);
        assert!(xml.contains("</tx><tx>"));
    }

    #[test]
    fn audyssey_query_with_empty_params() {
        let params = [
            Param {
                name: "dynamiceq",
                text: "",
            },
            Param {
                name: "multeq",
                text: "",
            },
        ];
        let cmd = Cmd {
            endpoint: Endpoint::AppCommand0300,
            text: None,
            name: Some("GetAudyssey"),
            params: Some(&params),
            set: None,
        };
        assert_eq!(
            body(&[cmd]),
            "<?xml version='1.0' encoding='utf-8'?>\n<tx><cmd id=\"3\"><name>GetAudyssey</name>\
             <list><param name=\"dynamiceq\" /><param name=\"multeq\" /></list></cmd></tx>"
        );
    }

    #[test]
    fn set_commands() {
        let params = [Param {
            name: "dynamiceq",
            text: "1",
        }];
        assert_eq!(
            body(&[Cmd::set_audyssey(&params)]),
            "<?xml version='1.0' encoding='utf-8'?>\n<tx><cmd id=\"3\"><name>SetAudyssey</name>\
             <list><param name=\"dynamiceq\">1</param></list></cmd></tx>"
        );
        assert_eq!(
            body(&[Cmd::set_tone_control(Param {
                name: "bassvalue",
                text: "45"
            })]),
            "<?xml version='1.0' encoding='utf-8'?>\n<tx><cmd id=\"1\">SetToneControl</cmd>\
             <bassvalue>45</bassvalue></tx>"
        );
    }

    #[test]
    fn escapes_text() {
        let cmd = Cmd::get("a<b>&\"c");
        assert!(body(&[cmd]).contains("a&lt;b&gt;&amp;&quot;c"));
    }

    #[test]
    fn rejects_control_characters_and_bad_names() {
        let mut out = b"KEEP".to_vec();
        assert_eq!(
            write_body(&[Cmd::get("A\nB")], &mut out),
            Err(Error::InvalidCommandText)
        );
        assert_eq!(out, b"KEEP");
        for name in ["", "1a", "a b", "a>b", "a/b", "ä"] {
            let cmd = Cmd::set_tone_control(Param { name, text: "1" });
            assert_eq!(
                write_body(&[cmd], &mut out),
                Err(Error::InvalidName),
                "{name:?}"
            );
            assert_eq!(out, b"KEEP");
        }
    }

    #[test]
    fn endpoints() {
        assert_eq!(Endpoint::AppCommand.path(), "/goform/AppCommand.xml");
        assert_eq!(
            Endpoint::AppCommand0300.path(),
            "/goform/AppCommand0300.xml"
        );
        assert_eq!(Endpoint::AppCommand0300.id(), "3");
    }
}
