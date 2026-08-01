use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, LOGICAL_PROCESSOR_RELATIONSHIP,
};

pub(crate) fn query_records<T>(
    requested: LOGICAL_PROCESSOR_RELATIONSHIP,
    expected: LOGICAL_PROCESSOR_RELATIONSHIP,
    mut parse: impl FnMut(&[u8]) -> std::io::Result<T>,
) -> std::io::Result<Vec<T>> {
    let mut length = 0_u32;
    let first =
        unsafe { GetLogicalProcessorInformationEx(requested, std::ptr::null_mut(), &mut length) };
    let first_error = std::io::Error::last_os_error();
    if first != 0 || first_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
        return Err(first_error);
    }
    if length < 8 {
        return Err(malformed(format!(
            "topology relationship {requested} reported {length} bytes"
        )));
    }

    let bytes = length as usize;
    let mut storage = vec![0_usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let mut written = length;
    if unsafe {
        GetLogicalProcessorInformationEx(requested, storage.as_mut_ptr().cast(), &mut written)
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let capacity = storage.len() * std::mem::size_of::<usize>();
    if written as usize > capacity {
        return Err(malformed(
            "topology query wrote beyond the requested buffer length",
        ));
    }
    let bytes = unsafe { std::slice::from_raw_parts(storage.as_ptr().cast(), written as usize) };
    parse_records(bytes, expected, &mut parse)
}

fn parse_records<T>(
    bytes: &[u8],
    expected: LOGICAL_PROCESSOR_RELATIONSHIP,
    parse: &mut impl FnMut(&[u8]) -> std::io::Result<T>,
) -> std::io::Result<Vec<T>> {
    let mut offset = 0_usize;
    let mut records = Vec::new();
    while offset < bytes.len() {
        if bytes.len() - offset < 8 {
            return Err(malformed("truncated topology record header"));
        }
        let relationship = i32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let size = u32::from_ne_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if relationship != expected {
            return Err(malformed(
                "topology record relationship changed inside the buffer",
            ));
        }
        if size < 8 || size > bytes.len() - offset {
            return Err(malformed("topology record has an invalid size"));
        }
        records.push(parse(&bytes[offset..offset + size])?);
        offset += size;
    }
    if records.is_empty() {
        return Err(malformed("topology query returned no records"));
    }
    Ok(records)
}

fn malformed(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::SystemInformation::{
        RelationProcessorCore, RelationProcessorPackage,
    };

    fn record(relationship: LOGICAL_PROCESSOR_RELATIONSHIP, size: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; size as usize];
        bytes[..4].copy_from_slice(&relationship.to_ne_bytes());
        bytes[4..8].copy_from_slice(&size.to_ne_bytes());
        bytes
    }

    #[test]
    fn record_parser_validates_relationship_size_and_truncation() {
        let valid = record(RelationProcessorCore, 8);
        assert_eq!(
            parse_records(&valid, RelationProcessorCore, &mut |_| Ok(7)).unwrap(),
            vec![7]
        );

        let wrong = record(RelationProcessorPackage, 8);
        let truncated = [0_u8; 7];
        let oversized = record(RelationProcessorCore, 16);
        for invalid in [&wrong[..], &truncated[..], &oversized[..8]] {
            assert_eq!(
                parse_records(invalid, RelationProcessorCore, &mut |_| Ok(()))
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::InvalidData
            );
        }
    }
}
