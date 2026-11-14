//! Access to small XML documents without building a tree.
//!
//! Receiver XML is read in place. Names and text borrow from the input. Text
//! only allocates if it has entities, for example `&amp;`.
//!
//! [`check_well_formed`] checks a whole document once. The other functions
//! assume a checked document and stop at the first problem they find.

use std::borrow::Cow;

use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use crate::error::Error;

/// Content of an element, without its start and end tag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Node<'a> {
    inner: &'a [u8],
}

/// A child element and its content.
#[derive(Debug)]
pub(crate) struct Child<'a> {
    start: BytesStart<'a>,
    node: Node<'a>,
}

/// Iterator over the child elements of a [`Node`].
pub(crate) struct Children<'a> {
    reader: Reader<&'a [u8]>,
    source: &'a [u8],
}

/// Iterator over the elements with one name at any depth of a [`Node`].
pub(crate) struct Descendants<'a, 'n> {
    reader: Reader<&'a [u8]>,
    source: &'a [u8],
    name: &'n str,
}

impl<'a> Node<'a> {
    pub(crate) const fn new(inner: &'a [u8]) -> Self {
        Self { inner }
    }

    pub(crate) fn children(self) -> Children<'a> {
        Children {
            reader: Reader::from_reader(self.inner),
            source: self.inner,
        }
    }

    /// The elements with this name at any depth, in document order. The
    /// content of a match is not searched.
    pub(crate) fn descendants<'n>(self, name: &'n str) -> Descendants<'a, 'n> {
        Descendants {
            reader: Reader::from_reader(self.inner),
            source: self.inner,
            name,
        }
    }

    /// First child with the given name.
    pub(crate) fn child(self, name: &str) -> Option<Child<'a>> {
        self.children().find(|child| child.is(name))
    }

    /// Text before the first child element. Empty if there is none.
    ///
    /// Returns `None` if the text is not valid.
    pub(crate) fn text(self) -> Option<Cow<'a, str>> {
        let mut reader = Reader::from_reader(self.inner);
        let mut text: Option<Cow<'a, str>> = None;
        loop {
            let piece: Cow<'a, str> = match reader.read_event().ok()? {
                Event::Text(t) => t.xml10_content(),
                Event::CData(c) => c.xml10_content(),
                Event::GeneralRef(r) => match r.resolve_char_ref().ok()? {
                    Some(ch) => Cow::Owned(ch.to_string()),
                    None => Cow::Borrowed(resolve_predefined_entity(&r)?),
                },
                Event::Comment(_) | Event::PI(_) | Event::Decl(_) => continue,
                Event::Start(_) | Event::Empty(_) | Event::End(_) | Event::Eof => break,
                Event::DocType(_) => return None,
            };
            text = Some(match text {
                None => piece,
                Some(mut so_far) => {
                    so_far.to_mut().push_str(&piece);
                    so_far
                }
            });
        }
        Some(text.unwrap_or(Cow::Borrowed("")))
    }

    /// Returns true if an element with this name is anywhere inside.
    pub(crate) fn contains_descendant(self, name: &str) -> bool {
        self.descendants(name).next().is_some()
    }
}

impl<'a> Child<'a> {
    pub(crate) fn is(&self, name: &str) -> bool {
        self.start.local_name().as_ref() == name
    }

    /// Name of the element without a namespace prefix.
    pub(crate) fn name(&self) -> String {
        self.start.local_name().as_ref().to_owned()
    }

    pub(crate) const fn node(&self) -> Node<'a> {
        self.node
    }

    /// Value of an attribute. `None` if it is missing or not valid.
    pub(crate) fn attribute(&self, name: &str) -> Option<Cow<'_, str>> {
        let attribute = self.start.try_get_attribute(name).ok()??;
        attribute.normalized_value(XmlVersion::Implicit1_0).ok()
    }
}

impl<'a> Iterator for Children<'a> {
    type Item = Child<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.reader.read_event().ok()? {
                Event::Start(start) => return element(&mut self.reader, self.source, start),
                Event::Empty(start) => return Some(empty_element(start)),
                Event::Eof => return None,
                _ => {}
            }
        }
    }
}

impl<'a> Iterator for Descendants<'a, '_> {
    type Item = Child<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.reader.read_event().ok()? {
                Event::Start(start) if start.local_name().as_ref() == self.name => {
                    return element(&mut self.reader, self.source, start);
                }
                Event::Empty(start) if start.local_name().as_ref() == self.name => {
                    return Some(empty_element(start));
                }
                Event::Eof => return None,
                _ => {}
            }
        }
    }
}

/// The element that `start` opens, with its content. The reader moves past
/// its end tag.
fn element<'a>(
    reader: &mut Reader<&'a [u8]>,
    source: &'a [u8],
    start: BytesStart<'a>,
) -> Option<Child<'a>> {
    let span = reader.read_to_end(start.name()).ok()?;
    let range = usize::try_from(span.start).ok()?..usize::try_from(span.end).ok()?;
    Some(Child {
        start,
        node: Node::new(source.get(range)?),
    })
}

/// An element without content, for example `<a/>`.
const fn empty_element(start: BytesStart<'_>) -> Child<'_> {
    Child {
        start,
        node: Node::new(&[]),
    }
}

/// Checks that the document is well-formed and has no DTD.
///
/// A DTD can define entities. Receivers do not send one, so it is refused.
pub(crate) fn check_well_formed(document: &[u8]) -> Result<(), Error> {
    let mut reader = Reader::from_reader(document);
    let mut depth = 0_usize;
    let mut elements = 0_usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {
                depth += 1;
                elements += 1;
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Empty(_)) => elements += 1,
            Ok(Event::DocType(_)) => {
                return Err(invalid(document, reader.buffer_position(), "DTD"));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(invalid(document, reader.buffer_position(), "XML syntax")),
        }
    }
    if depth != 0 || elements == 0 {
        return Err(invalid(
            document,
            document.len() as u64,
            "unclosed or missing element",
        ));
    }
    Ok(())
}

fn invalid(document: &[u8], position: u64, what: &'static str) -> Error {
    use core::fmt::Write as _;
    let mut start = String::with_capacity(32);
    for byte in document.iter().take(16) {
        let _ = write!(start, "{byte:02x}");
    }
    Error::InvalidXml {
        what,
        position: usize::try_from(position)
            .unwrap_or(usize::MAX)
            .min(document.len()),
        start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(text: &str) -> Child<'_> {
        check_well_formed(text.as_bytes()).unwrap();
        Node::new(text.as_bytes()).children().next().unwrap()
    }

    #[test]
    fn children_text_and_attributes() {
        let rx = root(
            "<?xml version=\"1.0\"?>\n<rx>\n<cmd>\n<a>1</a><b/><c x='y'>t&amp;u</c></cmd><error>2</error></rx>",
        );
        assert!(rx.is("rx"));
        let cmd = rx.node().child("cmd").unwrap().node();
        assert_eq!(cmd.children().count(), 3);
        assert_eq!(cmd.child("a").unwrap().node().text().unwrap(), "1");
        assert_eq!(cmd.child("b").unwrap().node().text().unwrap(), "");
        assert_eq!(cmd.child("c").unwrap().node().text().unwrap(), "t&u");
        assert_eq!(cmd.child("c").unwrap().attribute("x").unwrap(), "y");
        assert_eq!(cmd.child("c").unwrap().attribute("z"), None);
        assert_eq!(
            rx.node().child("error").unwrap().node().text().unwrap(),
            "2"
        );
    }

    #[test]
    fn text_is_borrowed_unless_it_has_entities() {
        let node = root("<r><a>plain</a><b>a&lt;b&#65;</b><c><![CDATA[<raw>]]></c></r>").node();
        assert!(matches!(
            node.child("a").unwrap().node().text().unwrap(),
            Cow::Borrowed("plain")
        ));
        assert_eq!(node.child("b").unwrap().node().text().unwrap(), "a<bA");
        assert_eq!(node.child("c").unwrap().node().text().unwrap(), "<raw>");
    }

    #[test]
    fn descendants_are_found_at_any_depth() {
        let node = root("<r><a><x>1</x></a><x>2<x>inner</x></x><b><c><x/></c></b></r>").node();
        let found: Vec<String> = node
            .descendants("x")
            .map(|x| x.node().text().unwrap().into_owned())
            .collect();
        assert_eq!(found, ["1", "2", ""]);
        assert_eq!(node.descendants("y").count(), 0);
    }

    #[test]
    fn text_stops_at_the_first_child() {
        let node = root("<r><a>one<b/>two</a></r>").node();
        assert_eq!(node.child("a").unwrap().node().text().unwrap(), "one");
    }

    #[test]
    fn nested_elements_with_the_same_name() {
        let node = root("<r><a><a>in</a>out</a><a>second</a></r>").node();
        let items: Vec<_> = node.children().collect();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].node().child("a").unwrap().node().text().unwrap(),
            "in"
        );
        assert_eq!(items[1].node().text().unwrap(), "second");
    }

    #[test]
    fn namespace_prefixes_are_ignored() {
        let rx = root("<s:r><s:a>1</s:a></s:r>");
        assert!(rx.is("r"));
        assert!(rx.node().child("a").is_some());
        assert!(rx.node().contains_descendant("a"));
        assert!(!rx.node().contains_descendant("r"));
    }

    #[test]
    fn rejects_bad_documents() {
        let bad: [&[u8]; 8] = [
            b"",
            b"text",
            b"<a>",
            b"<a></b>",
            b"<a><b></a></b>",
            b"</a>",
            b"<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>",
            b"<a>\xff</a>",
        ];
        for document in bad {
            assert!(
                matches!(check_well_formed(document), Err(Error::InvalidXml { .. })),
                "{:?}",
                String::from_utf8_lossy(document)
            );
        }
    }

    #[test]
    fn error_has_position_and_start() {
        let Err(Error::InvalidXml { what, start, .. }) = check_well_formed(b"<a></b>") else {
            panic!("expected an error");
        };
        assert_eq!(what, "XML syntax");
        assert_eq!(start, "3c613e3c2f623e");
    }
}
