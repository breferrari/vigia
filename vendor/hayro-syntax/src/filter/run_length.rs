use crate::reader::Reader;
use alloc::vec;
use alloc::vec::Vec;

pub(crate) fn decode(data: &[u8]) -> Option<Vec<u8>> {
    let mut reader = Reader::new(data);
    let mut decoded = vec![];

    loop {
        let length = reader.read_byte()?;

        match length {
            128 => break,
            0..=127 => {
                // PDFBOX-3990, just abort early if stream is invalid.
                let Some(bytes) = reader.read_bytes(length as usize + 1) else {
                    break;
                };

                if !crate::limit::room(&mut decoded, bytes.len()) {
                    return None;
                }
                decoded.extend(bytes);
            }
            _ => {
                let length = 257 - length as usize;
                let byte = reader.read_byte()?;
                if !crate::limit::room(&mut decoded, length) {
                    return None;
                }
                decoded.extend(core::iter::repeat_n(byte, length));
            }
        }
    }

    Some(decoded)
}

#[cfg(test)]
mod tests {
    use crate::filter::run_length::decode;

    #[test]
    fn run_length() {
        let input = vec![4, 10, 11, 12, 13, 14, 253, 3, 128];
        assert_eq!(
            decode(&input).unwrap(),
            vec![10, 11, 12, 13, 14, 3, 3, 3, 3]
        );
    }
}
