//! Answers from the AppCommand endpoints.

use std::borrow::Cow;

use super::pattern::Definition;
use crate::error::Error;
use crate::xml::{Child, Children, Node, check_well_formed};
use crate::zone::Zone;

/// A checked answer. It borrows the body.
#[derive(Debug, Clone, Copy)]
pub struct Response<'a> {
    root: Node<'a>,
    count: usize,
}

/// One `cmd` or `error` element of an answer.
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    node: Node<'a>,
    is_error: bool,
}

/// Iterator over the items of a [`Response`].
pub struct Items<'a> {
    children: Children<'a>,
}

impl<'a> Response<'a> {
    /// Checks an answer to a request with `expected` commands.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not XML, is an HTML page, has an element
    /// other than `cmd` or `error`, or does not have `expected` items.
    pub fn parse(body: &'a [u8], expected: usize) -> Result<Self, Error> {
        let response = Self::check(body)?;
        if response.count != expected {
            return Err(Error::UnexpectedItemCount {
                expected,
                actual: response.count,
            });
        }
        Ok(response)
    }

    /// Like [`Response::parse`], but the answer can have fewer items than
    /// the `most` commands of the request. A busy receiver can leave some
    /// out. Use [`Response::matched`] to find the command of each item.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not XML, is an HTML page, has an element
    /// other than `cmd` or `error`, or has more than `most` items.
    pub fn parse_at_most(body: &'a [u8], most: usize) -> Result<Self, Error> {
        let response = Self::check(body)?;
        if response.count > most {
            return Err(Error::UnexpectedItemCount {
                expected: most,
                actual: response.count,
            });
        }
        Ok(response)
    }

    fn check(body: &'a [u8]) -> Result<Self, Error> {
        check_well_formed(body)?;
        let root = Node::new(body)
            .children()
            .next()
            .ok_or(Error::HtmlResponse)?;
        if root.is("html") {
            return Err(Error::HtmlResponse);
        }
        let mut count = 0;
        for (index, child) in root.node().children().enumerate() {
            if !child.is("cmd") && !child.is("error") {
                return Err(Error::UnexpectedElement { index });
            }
            count += 1;
        }
        Ok(Self {
            root: root.node(),
            count,
        })
    }

    /// Each item with the index of its command in `definitions`, the
    /// commands of the request in their order.
    ///
    /// If the answer has an item for each command, the order tells the
    /// command, and items after the last command are skipped. If it has
    /// fewer, the `name` element of the item tells it. Then
    /// an item with no name, or with a name that no command has, is skipped.
    /// An `error` element has no name.
    pub fn matched<'d>(
        &self,
        definitions: &'d [Definition],
    ) -> impl Iterator<Item = (usize, Item<'a>)> + use<'a, 'd> {
        let complete = self.count >= definitions.len();
        self.items().enumerate().filter_map(move |(index, item)| {
            if complete {
                return (index < definitions.len()).then_some((index, item));
            }
            let name = item.name()?;
            definitions
                .iter()
                .position(|definition| definition.cmd.name == Some(name.trim()))
                .map(|index| (index, item))
        })
    }

    /// The items in the order of the commands.
    #[must_use]
    pub fn items(&self) -> Items<'a> {
        Items {
            children: self.root.children(),
        }
    }

    /// Calls `on_value` for each value found for the commands.
    ///
    /// `definitions` must be the commands of the request, or the first of
    /// them, in the same order. The items are matched to them as
    /// [`Response::matched`] does, so a short answer is matched by name. An
    /// item with an error is skipped. A value that is not in the answer is
    /// skipped. An empty value gives `None`.
    pub fn extract(
        &self,
        definitions: &[Definition],
        zone: Zone,
        mut on_value: impl FnMut(super::pattern::Field, Option<&str>),
    ) {
        for (index, item) in self.matched(definitions) {
            definitions[index].extract(&item, zone, &mut on_value);
        }
    }
}

impl<'a> Item<'a> {
    /// Returns true for an `error` element.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        self.is_error
    }

    /// Text of the `name` element, for example `GetVideoInfo`, not trimmed.
    /// Only the answers of `AppCommand0300.xml` have it.
    #[must_use]
    pub fn name(&self) -> Option<Cow<'a, str>> {
        self.node.child("name")?.node().text()
    }

    /// Text of an `error` element, for example `2`.
    #[must_use]
    pub fn error_code(&self) -> Option<Cow<'a, str>> {
        if self.is_error {
            self.node.text()
        } else {
            None
        }
    }

    pub(super) const fn node(&self) -> Node<'a> {
        self.node
    }

    /// Calls `on_entry` with the name and the new name of each source in the
    /// answer to `GetRenameSource`. The new name is `None` if it is empty.
    /// Nothing is called for an item with an error.
    pub fn renamed_sources(&self, on_entry: impl FnMut(&str, Option<&str>)) {
        self.source_entries("functionrename", "rename", on_entry);
    }

    /// Calls `on_entry` with the name and the `use` value of each source in
    /// the answer to `GetDeletedSource`. The value is `None` if it is empty.
    /// Nothing is called for an item with an error.
    pub fn source_use(&self, on_entry: impl FnMut(&str, Option<&str>)) {
        self.source_entries("functiondelete", "use", on_entry);
    }

    /// Calls `on_param` with the name, the `control` attribute and the text of
    /// each `param` element in the `list` of the answer. `control` is `None`
    /// if it is missing or not a number. Nothing is called for an item with
    /// an error.
    pub fn params(&self, mut on_param: impl FnMut(&str, Option<u8>, &str)) {
        if self.is_error {
            return;
        }
        let Some(list) = self.node.child("list") else {
            return;
        };
        for param in list.node().children().filter(|child| child.is("param")) {
            let Some(name) = param.attribute("name") else {
                continue;
            };
            let control = param
                .attribute("control")
                .and_then(|control| control.trim().parse().ok());
            let text = param.node().text();
            on_param(&name, control, text.as_deref().map_or("", str::trim));
        }
    }

    fn source_entries(
        &self,
        wrapper: &str,
        value: &str,
        mut on_entry: impl FnMut(&str, Option<&str>),
    ) {
        if self.is_error {
            return;
        }
        let Some(wrapper) = self.node.child(wrapper) else {
            return;
        };
        for entry in wrapper.node().children().filter(|child| child.is("list")) {
            let text = |name: &str| {
                let child = entry.node().child(name)?;
                let text = child.node().text()?;
                Some(text.trim().to_owned()).filter(|text| !text.is_empty())
            };
            if let Some(name) = text("name") {
                on_entry(&name, text(value).as_deref());
            }
        }
    }
}

impl<'a> Iterator for Items<'a> {
    type Item = Item<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let child: Child<'a> = self.children.next()?;
        Some(Item {
            is_error: child.is("error"),
            node: child.node(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appcommand::{
        Field, GET_ALL_ZONE_POWER_STATUS, GET_AUDIO_INFO, GET_AUDYSSEY, GET_DIMMER,
        GET_TONE_CONTROL, GET_VIDEO_INFO,
    };

    #[test]
    fn a_short_answer_is_matched_by_name() {
        let definitions = [GET_AUDYSSEY, GET_VIDEO_INFO, GET_AUDIO_INFO];
        let body = b"<rx><cmd><name> GetAudioInfo </name></cmd><error>2</error></rx>";
        assert!(Response::parse(body, 3).is_err());
        let response = Response::parse_at_most(body, 3).unwrap();
        let matched: Vec<_> = response.matched(&definitions).map(|(i, _)| i).collect();
        assert_eq!(matched, [2]);
        // An answer with all items is matched by order, errors too.
        let body = b"<rx><error>2</error><cmd/><cmd><name>GetAudyssey</name></cmd></rx>";
        let response = Response::parse_at_most(body, 3).unwrap();
        let matched: Vec<_> = response.matched(&definitions).map(|(i, _)| i).collect();
        assert_eq!(matched, [0, 1, 2]);
        let matched: Vec<_> = response
            .matched(&definitions[..2])
            .map(|(i, _)| i)
            .collect();
        assert_eq!(matched, [0, 1]);
        assert_eq!(
            Response::parse_at_most(body, 2).unwrap_err(),
            Error::UnexpectedItemCount {
                expected: 2,
                actual: 3
            }
        );
        // `extract` matches in the same way.
        let body = b"<rx><cmd><name>GetVideoInfo</name><list>\
            <param name=\"hdmisigin\" control=\"1\">4K50</param></list></cmd></rx>";
        let response = Response::parse_at_most(body, 3).unwrap();
        let mut fields = Vec::new();
        response.extract(&definitions, Zone::Main, |field, value| {
            fields.push((field, value.map(str::to_owned)));
        });
        assert!(
            fields.contains(&(Field::VideoSignalIn, Some("4K50".to_owned()))),
            "{fields:?}"
        );
        assert!(
            fields
                .iter()
                .all(|(field, _)| !matches!(field, Field::DynamicEq))
        );
        // A name that no command has is skipped.
        let body = b"<rx><cmd><name>GetOther</name></cmd></rx>";
        let response = Response::parse_at_most(body, 3).unwrap();
        assert_eq!(response.matched(&definitions).count(), 0);
    }

    #[test]
    fn items_and_errors() {
        let body = b"<rx><cmd><zone1>ON</zone1></cmd><error>2</error></rx>";
        let response = Response::parse(body, 2).unwrap();
        let items: Vec<_> = response.items().collect();
        assert!(!items[0].is_error());
        assert_eq!(items[0].error_code(), None);
        assert!(items[1].is_error());
        assert_eq!(items[1].error_code().as_deref(), Some("2"));
    }

    #[test]
    fn extract_skips_errors_and_missing_values() {
        let body = b"<rx><cmd><zone1>ON</zone1><zone2>OFF</zone2></cmd><error>2</error></rx>";
        let response = Response::parse(body, 2).unwrap();
        let mut values = Vec::new();
        response.extract(
            &[GET_ALL_ZONE_POWER_STATUS, GET_DIMMER],
            Zone::Zone2,
            |field, value| {
                values.push((field, value.map(str::to_owned)));
            },
        );
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].1.as_deref(), Some("OFF"));
    }

    #[test]
    fn rejects_unexpected_elements_and_counts() {
        assert_eq!(
            Response::parse(b"<rx><cmd/><foo/></rx>", 2).unwrap_err(),
            Error::UnexpectedElement { index: 1 }
        );
        assert_eq!(
            Response::parse(b"<rx><cmd/></rx>", 2).unwrap_err(),
            Error::UnexpectedItemCount {
                expected: 2,
                actual: 1
            }
        );
        assert_eq!(Response::parse(b"<rx></rx>", 0).unwrap().items().count(), 0);
    }

    #[test]
    fn blank_values_depend_on_the_definition() {
        let body = b"<rx><cmd><status>1</status><basslevel/><bassvalue>45</bassvalue></cmd>\
                     <cmd><value/></cmd></rx>";
        let response = Response::parse(body, 2).unwrap();
        let mut values = Vec::new();
        response.extract(
            &[GET_TONE_CONTROL, GET_DIMMER],
            Zone::Main,
            |field, value| {
                values.push((field, value.map(str::to_owned)));
            },
        );
        // Tone control keeps its value when the answer is blank.
        assert_eq!(
            values,
            [
                (Field::ToneControlStatus, Some("1".to_owned())),
                (Field::Bass, Some("45".to_owned())),
                // The dimmer is unknown when the answer is blank.
                (Field::Dimmer, None),
            ]
        );
    }

    #[test]
    fn source_lists_are_read() {
        let body = b"<rx><cmd><functionrename>\
            <list><name>CBL/SAT</name><rename>TV </rename></list>\
            <list><name>AUX3</name><rename></rename></list>\
            <list><rename>No name</rename></list>\
            </functionrename></cmd><cmd><functiondelete>\
            <list><name>CBL/SAT</name><FuncName>CBL/SAT</FuncName><use>0</use></list>\
            <list><name>CD</name><use/></list>\
            </functiondelete></cmd><error>2</error></rx>";
        let response = Response::parse(body, 3).unwrap();
        let items: Vec<_> = response.items().collect();
        let mut renamed = Vec::new();
        items[0]
            .renamed_sources(|name, new| renamed.push((name.to_owned(), new.map(str::to_owned))));
        assert_eq!(
            renamed,
            [
                ("CBL/SAT".to_owned(), Some("TV".to_owned())),
                ("AUX3".to_owned(), None)
            ]
        );
        let mut used = Vec::new();
        items[1].source_use(|name, value| used.push((name.to_owned(), value.map(str::to_owned))));
        assert_eq!(
            used,
            [
                ("CBL/SAT".to_owned(), Some("0".to_owned())),
                ("CD".to_owned(), None)
            ]
        );
        // An item with an error, or another kind of answer, gives nothing.
        let mut count = 0;
        items[2].renamed_sources(|_, _| count += 1);
        items[0].source_use(|_, _| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn rejects_html_and_bad_xml() {
        assert_eq!(
            Response::parse(b"<html><body>Error</body></html>", 1).unwrap_err(),
            Error::HtmlResponse
        );
        assert!(matches!(
            Response::parse(b"<rx><cmd></rx>", 1),
            Err(Error::InvalidXml { .. })
        ));
        assert!(matches!(
            Response::parse(b"", 1),
            Err(Error::InvalidXml { .. })
        ));
    }

    proptest::proptest! {
        #[test]
        fn parse_never_panics(body in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..120)) {
            if let Ok(response) = Response::parse(&body, 1) {
                response.extract(&[GET_ALL_ZONE_POWER_STATUS], Zone::Main, |_, _| {});
            }
        }

        #[test]
        fn parse_xml_like_never_panics(body in "[<>/a-z0-9 =\"'&;!?-]{0,60}") {
            if let Ok(response) = Response::parse(body.as_bytes(), 1) {
                response.extract(&[GET_ALL_ZONE_POWER_STATUS], Zone::Main, |_, _| {});
            }
        }
    }
}
