//! Where the values of a command are in its answer.

use super::request::Cmd;
use super::response::Item;
use crate::xml::{Child, Node};
use crate::zone::Zone;

/// A value that an answer can set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Field {
    /// Name that the user gave the receiver.
    FriendlyName,
    /// Name of a zone.
    ZoneName,
    /// Mute state of a zone.
    Muted,
    /// Power state of a zone.
    Power,
    /// Selected source of a zone.
    InputFunc,
    /// Volume of a zone.
    Volume,
    /// Volume limit of a zone.
    MaxVolume,
    /// Auto standby time.
    AutoStandby,
    /// Display dimmer mode.
    Dimmer,
    /// Eco mode.
    EcoMode,
    /// Raw surround mode.
    SoundModeRaw,
    /// Tone control on or off.
    ToneControlStatus,
    /// Tone control can be adjusted.
    ToneControlAdjust,
    /// Bass level text.
    BassLevel,
    /// Bass value.
    Bass,
    /// Treble level text.
    TrebleLevel,
    /// Treble value.
    Treble,
    /// MultEQ mode.
    MultEq,
    /// MultEQ `control` attribute.
    MultEqControl,
    /// Dynamic EQ.
    DynamicEq,
    /// Dynamic EQ `control` attribute.
    DynamicEqControl,
    /// Reference level offset.
    RefLevOffset,
    /// Reference level offset `control` attribute.
    RefLevOffsetControl,
    /// Dynamic volume.
    DynamicVol,
    /// Dynamic volume `control` attribute.
    DynamicVolControl,
}

/// One step on the way to a value. A step selects a child element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Child with this name.
    Tag(&'static str),
    /// Child `zone1` for the main zone, `zone2` or `zone3` for the others.
    ZoneTag,
    /// Child `tag` that has a child `child` with the name of the zone as text.
    TagWithZoneChild {
        /// Name of the element.
        tag: &'static str,
        /// Name of the child with the zone name.
        child: &'static str,
    },
    /// Child `tag` that has an attribute `name` with this value.
    TagWithAttribute {
        /// Name of the element.
        tag: &'static str,
        /// Attribute name.
        name: &'static str,
        /// Attribute value.
        value: &'static str,
    },
}

/// What to read from the element at the end of the steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaf {
    /// Text of the element.
    Text,
    /// Value of an attribute.
    Attribute(&'static str),
}

/// A value to read from the item of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pattern {
    /// Value that this pattern sets.
    pub field: Field,
    /// Steps from the `cmd` element to the element with the value.
    pub steps: &'static [Step],
    /// What to read at the end.
    pub leaf: Leaf,
}

/// A command with the patterns for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition {
    /// Command to send.
    pub cmd: Cmd<'static>,
    /// Values in the answer.
    pub patterns: &'static [Pattern],
    /// What a blank value means. `true` means the value is not known. `false`
    /// means the value did not change, so the answer gives nothing for it.
    pub blank_is_unknown: bool,
}

impl Pattern {
    /// Text of the element at the end of `steps`.
    #[must_use]
    pub const fn text(field: Field, steps: &'static [Step]) -> Self {
        Self {
            field,
            steps,
            leaf: Leaf::Text,
        }
    }

    /// Attribute of the element at the end of `steps`.
    #[must_use]
    pub const fn attribute(field: Field, steps: &'static [Step], name: &'static str) -> Self {
        Self {
            field,
            steps,
            leaf: Leaf::Attribute(name),
        }
    }
}

impl Definition {
    /// Calls `on_value` for each value of this command found in `item`.
    ///
    /// The value is `None` for an empty element or a missing attribute, unless
    /// [`Definition::blank_is_unknown`] is false: then nothing is called.
    /// Nothing is called for an element that is not there.
    pub fn extract(
        &self,
        item: &Item<'_>,
        zone: Zone,
        on_value: &mut impl FnMut(Field, Option<&str>),
    ) {
        if item.is_error() {
            return;
        }
        for pattern in self.patterns {
            find(
                item.node(),
                pattern.steps,
                zone,
                pattern.leaf,
                &mut |value| {
                    if value.is_some() || self.blank_is_unknown {
                        on_value(pattern.field, value);
                    }
                },
            );
        }
    }
}

/// Follows `steps` from `node`. Calls `on_value` for the first match.
/// Returns false if there is no match.
pub(crate) fn find(
    node: Node<'_>,
    steps: &[Step],
    zone: Zone,
    leaf: Leaf,
    on_value: &mut impl FnMut(Option<&str>),
) -> bool {
    let Some((step, rest)) = steps.split_first() else {
        return false;
    };
    for child in node.children() {
        if !step_matches(*step, &child, zone) {
            continue;
        }
        if rest.is_empty() {
            on_value(read_leaf(&child, leaf).as_deref());
            return true;
        }
        if find(child.node(), rest, zone, leaf, on_value) {
            return true;
        }
    }
    false
}

fn read_leaf<'c>(child: &'c Child<'_>, leaf: Leaf) -> Option<std::borrow::Cow<'c, str>> {
    let value = match leaf {
        Leaf::Text => child.node().text()?,
        Leaf::Attribute(name) => child.attribute(name)?,
    };
    if value.is_empty() { None } else { Some(value) }
}

fn step_matches(step: Step, child: &Child<'_>, zone: Zone) -> bool {
    match step {
        Step::Tag(tag) => child.is(tag),
        Step::ZoneTag => child.is(zone_element(zone)),
        Step::TagWithZoneChild { tag, child: name } => {
            child.is(tag)
                && child
                    .node()
                    .children()
                    .any(|c| c.is(name) && c.node().text().is_some_and(|text| text == zone.name()))
        }
        Step::TagWithAttribute { tag, name, value } => {
            child.is(tag) && child.attribute(name).is_some_and(|v| v == value)
        }
    }
}

const fn zone_element(zone: Zone) -> &'static str {
    match zone {
        Zone::Main => "zone1",
        Zone::Zone2 => "zone2",
        Zone::Zone3 => "zone3",
    }
}
