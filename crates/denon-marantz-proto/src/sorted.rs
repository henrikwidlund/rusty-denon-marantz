//! Lists that are sorted at compile time, for a binary search by name.

use core::cmp::Ordering;

/// A value and the two names that find it. The second name is empty when
/// one name is enough.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Keyed<T> {
    pub(crate) key: (&'static str, &'static str),
    pub(crate) value: T,
}

/// Sorts a list by its keys, at compile time. Insertion sort: the lists are
/// short.
pub(crate) const fn sort<T: Copy, const N: usize>(mut list: [Keyed<T>; N]) -> [Keyed<T>; N] {
    let mut i = 1;
    while i < N {
        let mut j = i;
        while j > 0 && is_less(list[j].key, list[j - 1].key) {
            list.swap(j, j - 1);
            j -= 1;
        }
        i += 1;
    }
    list
}

/// The value with this key in a list that [`sort`] sorted.
pub(crate) fn find<T: Copy>(list: &[Keyed<T>], key: (&str, &str)) -> Option<T> {
    list.binary_search_by(|entry| compare(entry.key, key))
        .ok()
        .map(|index| list[index].value)
}

/// Returns true if the list holds a key with this first name and a second
/// name that is not empty.
pub(crate) fn has_second<T>(list: &[Keyed<T>], first: &str) -> bool {
    let index = list.partition_point(|entry| compare(entry.key, (first, "")).is_le());
    list.get(index).is_some_and(|entry| entry.key.0 == first)
}

/// Returns true if each key comes after the one before it, so that no key is
/// listed twice.
#[cfg(test)]
pub(crate) fn is_strictly_sorted<T>(list: &[Keyed<T>]) -> bool {
    list.windows(2)
        .all(|pair| is_less(pair[0].key, pair[1].key))
}

fn compare(key: (&str, &str), other: (&str, &str)) -> Ordering {
    key.cmp(&other)
}

/// Compares two keys as [`compare`] does.
const fn is_less(a: (&str, &str), b: (&str, &str)) -> bool {
    match bytes_order(a.0.as_bytes(), b.0.as_bytes()) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => matches!(bytes_order(a.1.as_bytes(), b.1.as_bytes()), Ordering::Less),
    }
}

/// Compares two byte strings as [`Ord`] does for [`str`].
const fn bytes_order(a: &[u8], b: &[u8]) -> Ordering {
    let mut i = 0;
    while i < a.len() && i < b.len() {
        if a[i] != b[i] {
            return if a[i] < b[i] {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        i += 1;
    }
    if a.len() < b.len() {
        Ordering::Less
    } else if a.len() > b.len() {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn keyed(first: &'static str, second: &'static str, value: u8) -> Keyed<u8> {
        Keyed {
            key: (first, second),
            value,
        }
    }

    static LIST: [Keyed<u8>; 5] = sort([
        keyed("b", "", 1),
        keyed("a", "z", 2),
        keyed("a", "", 3),
        keyed("ab", "", 4),
        keyed("a", "b", 5),
    ]);

    #[test]
    fn sorted_lists_are_searched() {
        assert!(is_strictly_sorted(&LIST));
        let values: Vec<_> = LIST.iter().map(|entry| entry.value).collect();
        assert_eq!(values, [3, 5, 2, 4, 1]);
        assert_eq!(find(&LIST, ("a", "b")), Some(5));
        assert_eq!(find(&LIST, ("ab", "")), Some(4));
        assert_eq!(find(&LIST, ("a", "c")), None);
        assert_eq!(find(&LIST, ("", "")), None);
        assert!(has_second(&LIST, "a"));
        assert!(!has_second(&LIST, "ab"));
        assert!(!has_second(&LIST, "b"));
        assert!(!has_second(&LIST, "c"));
    }
}
