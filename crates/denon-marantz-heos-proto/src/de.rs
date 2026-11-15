//! Deserializers for values that the CLI sends as a number or as a string.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use serde::Deserializer;
use serde::de::{Error, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// Integer that can come from a JSON number or from text.
pub(crate) trait Number: Sized {
    fn from_i64(value: i64) -> Option<Self>;
    fn from_u64(value: u64) -> Option<Self>;
    /// Parses text with an integer. Spaces around it are ignored.
    fn from_text(text: &str) -> Option<Self>;
}

macro_rules! number_impls {
    ($($name:ty),*) => {$(
        impl Number for $name {
            fn from_i64(value: i64) -> Option<Self> {
                Self::try_from(value).ok()
            }

            fn from_u64(value: u64) -> Option<Self> {
                Self::try_from(value).ok()
            }

            fn from_text(text: &str) -> Option<Self> {
                text.trim().parse().ok()
            }
        }
    )*};
}

number_impls!(u8, u32, i64, u64);

/// Integer from a JSON number or from a string with a number.
pub(crate) fn number<'de, D: Deserializer<'de>, T: Number>(deserializer: D) -> Result<T, D::Error> {
    deserializer
        .deserialize_any(NumberVisitor(PhantomData))?
        .ok_or_else(|| D::Error::custom("expected an integer or a string with an integer"))
}

/// Optional integer, see [`number`]. A value that is missing, `null` or not
/// valid gives `None`.
pub(crate) fn lenient_number<'de, D: Deserializer<'de>, T: Number>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    deserializer.deserialize_any(NumberVisitor(PhantomData))
}

/// Gives `None` for any value that is not a valid integer.
struct NumberVisitor<T>(PhantomData<T>);

impl<'de, T: Number> Visitor<'de> for NumberVisitor<T> {
    type Value = Option<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an integer or a string with an integer")
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(T::from_i64(value))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(T::from_u64(value))
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(T::from_text(value))
    }

    fn visit_f64<E: Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_bool<E: Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }
}

/// Text from a JSON string or number. Borrows the string if it has no escapes.
pub(crate) fn text<'de, D>(deserializer: D) -> Result<Cow<'de, str>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(TextVisitor)
}

/// True for the text `yes`. Spaces around it are ignored. Any other text or
/// number gives false. A JSON `true` or `false` is an error.
pub(crate) fn yes<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(option_text(deserializer)?.is_some_and(|text| text.trim() == "yes"))
}

/// Optional text, see [`text`]. `null` gives `None`.
pub(crate) fn option_text<'de, D>(deserializer: D) -> Result<Option<Cow<'de, str>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_option(OptionVisitor(TextVisitor))
}

struct TextVisitor;

impl<'de> Visitor<'de> for TextVisitor {
    type Value = Cow<'de, str>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string or a number")
    }

    fn visit_borrowed_str<E: Error>(self, value: &'de str) -> Result<Self::Value, E> {
        Ok(Cow::Borrowed(value))
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Cow::Owned(value.to_owned()))
    }

    fn visit_string<E: Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(Cow::Owned(value))
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Cow::Owned(value.to_string()))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Cow::Owned(value.to_string()))
    }
}

/// Wraps a visitor so that `null` gives `None`.
struct OptionVisitor<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for OptionVisitor<V> {
    type Value = Option<V::Value>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self.0).map(Some)
    }
}
