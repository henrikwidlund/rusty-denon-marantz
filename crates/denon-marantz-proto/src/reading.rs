//! A value of a web interface page, which the page can leave out.

/// A value of a page. A page can leave a value out, and a state keeps its
/// value then. That is not the same as a value that is empty or has no
/// signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading<T> {
    /// The page does not have the value.
    Absent,
    /// The page has the value, and it is empty or has no signal.
    Empty,
    /// The page has the value.
    Value(T),
}

// Not derived: a derive would need `T: Default`.
#[allow(clippy::derivable_impls)]
impl<T> Default for Reading<T> {
    fn default() -> Self {
        Self::Absent
    }
}

impl<T> Reading<T> {
    /// The value, `None` if the page has none.
    #[must_use]
    pub const fn as_value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Absent | Self::Empty => None,
        }
    }

    /// `Empty` for `None`, the value otherwise.
    pub(crate) fn from_option(value: Option<T>) -> Self {
        value.map_or(Self::Empty, Self::Value)
    }
}

impl<T: Copy + PartialEq> Reading<T> {
    /// Stores the value in `slot`. `Absent` keeps the old value, `Empty`
    /// clears it. Returns true if the slot changed.
    pub(crate) fn store(self, slot: &mut Option<T>) -> bool {
        let value = match self {
            Self::Absent => return false,
            Self::Empty => None,
            Self::Value(value) => Some(value),
        };
        if *slot == value {
            false
        } else {
            *slot = value;
            true
        }
    }
}

impl Reading<String> {
    /// Like [`Reading::store`], for a text. The buffer of the old text is
    /// reused.
    pub(crate) fn store_text(&self, slot: &mut Option<String>) -> bool {
        let value = match self {
            Self::Absent => return false,
            Self::Empty => None,
            Self::Value(value) => Some(value.as_str()),
        };
        match (slot.as_mut(), value) {
            (Some(current), Some(value)) if current == value => false,
            (Some(current), Some(value)) => {
                current.clear();
                current.push_str(value);
                true
            }
            (None, Some(value)) => {
                *slot = Some(value.to_owned());
                true
            }
            (Some(_), None) => {
                *slot = None;
                true
            }
            (None, None) => false,
        }
    }

    /// Reads the text as another value. A text that is not known is
    /// [`Reading::Empty`].
    pub(crate) fn convert<T>(self, from: fn(&str) -> Option<T>) -> Reading<T> {
        match self {
            Self::Value(text) => Reading::from_option(from(&text)),
            Self::Empty => Reading::Empty,
            Self::Absent => Reading::Absent,
        }
    }
}

/// A value of the page for the input and one for the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sides<T> {
    /// Value of the signal that the receiver gets.
    pub input: Reading<T>,
    /// Value of the signal that the receiver sends to the TV.
    pub output: Reading<T>,
}

impl<T> Default for Sides<T> {
    fn default() -> Self {
        Self {
            input: Reading::Absent,
            output: Reading::Absent,
        }
    }
}
