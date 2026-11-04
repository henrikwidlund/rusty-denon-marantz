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
        check_well_formed(body)?;
        let root = Node::new(body)
            .children()
            .next()
            .ok_or(Error::HtmlResponse)?;
        if root.is("html") {
            return Err(Error::HtmlResponse);
        }
        let response = Self { root: root.node() };
        let mut count = 0;
        for (index, child) in root.node().children().enumerate() {
            if !child.is("cmd") && !child.is("error") {
                return Err(Error::UnexpectedElement { index });
            }
            count += 1;
        }
        if count != expected {
            return Err(Error::UnexpectedItemCount {
                expected,
                actual: count,
            });
        }
        Ok(response)
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
    /// `definitions` must be the commands of the request, in the same order.
    /// An item with an error is skipped. A value that is not in the answer is
    /// skipped. An empty value gives `None`.
    pub fn extract(
        &self,
        definitions: &[Definition],
        zone: Zone,
        mut on_value: impl FnMut(super::pattern::Field, Option<&str>),
    ) {
        for (definition, item) in definitions.iter().zip(self.items()) {
            definition.extract(&item, zone, &mut on_value);
        }
    }
}

impl<'a> Item<'a> {
    /// Returns true for an `error` element.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        self.is_error
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
    use crate::appcommand::{GET_ALL_ZONE_POWER_STATUS, GET_DIMMER};

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
