//! Splits fuzz input into chunks of changing size.

/// Reads the chunk sizes from the start of `data`. The low two bits of the
/// first byte give the number of size bytes that follow, from 1 to 4. Returns
/// the sizes and the rest, or `None` if `data` is too short.
pub fn split_sizes(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let (&first, rest) = data.split_first()?;
    let count = usize::from(first & 3) + 1;
    rest.split_at_checked(count)
}

/// Splits `data` into chunks. Each byte of `sizes` gives the size of one
/// chunk, from 1 to 64 bytes. The sizes are used one after the other, and
/// again from the first. `sizes` must not be empty.
pub fn chunks<'a>(sizes: &'a [u8], mut data: &'a [u8]) -> impl Iterator<Item = &'a [u8]> {
    let mut sizes = sizes.iter().cycle();
    std::iter::from_fn(move || {
        if data.is_empty() {
            return None;
        }
        let size = usize::from(sizes.next()? % 64) + 1;
        let (chunk, rest) = data.split_at(size.min(data.len()));
        data = rest;
        Some(chunk)
    })
}
