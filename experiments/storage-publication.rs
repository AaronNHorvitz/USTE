#![forbid(unsafe_code)]

//! Executable R0 state-machine experiment for Decision 0004.
//! The checksum is intentionally non-cryptographic: this experiment tests publication and
//! recovery ordering only. Production storage must use Decision 0005 authentication.

#[derive(Debug, Eq, PartialEq)]
enum Recovery {
    Frontier(u64),
    IntegrityFailure,
}

fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn group(revision: u64, payload: &[u8]) -> Vec<u8> {
    let mut encoded = vec![b'G'];
    encoded.extend_from_slice(&revision.to_le_bytes());
    encoded.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    encoded.extend_from_slice(payload);
    let checksum = digest(&encoded);
    encoded.extend_from_slice(&checksum.to_le_bytes());
    encoded
}

fn certificate(revision: u64, group: &[u8], previous: u64) -> Vec<u8> {
    let mut encoded = vec![b'C'];
    encoded.extend_from_slice(&revision.to_le_bytes());
    encoded.extend_from_slice(&(group.len() as u64).to_le_bytes());
    encoded.extend_from_slice(&digest(group).to_le_bytes());
    encoded.extend_from_slice(&previous.to_le_bytes());
    let checksum = digest(&encoded);
    encoded.extend_from_slice(&checksum.to_le_bytes());
    encoded
}

fn read_u64(bytes: &[u8], start: usize) -> u64 {
    u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap())
}

fn recover(group_bytes: &[u8], certificate_bytes: &[u8]) -> Recovery {
    const CERTIFICATE_LEN: usize = 41;
    if certificate_bytes.len() < CERTIFICATE_LEN {
        return Recovery::Frontier(0);
    }
    if certificate_bytes.len() != CERTIFICATE_LEN || certificate_bytes[0] != b'C' {
        return Recovery::IntegrityFailure;
    }
    let certificate_checksum = read_u64(certificate_bytes, 33);
    if digest(&certificate_bytes[..33]) != certificate_checksum {
        return Recovery::IntegrityFailure;
    }

    let revision = read_u64(certificate_bytes, 1);
    let named_group_len = read_u64(certificate_bytes, 9) as usize;
    let named_group_digest = read_u64(certificate_bytes, 17);
    let previous = read_u64(certificate_bytes, 25);
    if revision != 1 || previous != 0 || group_bytes.len() != named_group_len {
        return Recovery::IntegrityFailure;
    }
    if group_bytes.len() < 21 || group_bytes[0] != b'G' || digest(group_bytes) != named_group_digest
    {
        return Recovery::IntegrityFailure;
    }

    let group_revision = read_u64(group_bytes, 1);
    let payload_len = u32::from_le_bytes(group_bytes[9..13].try_into().unwrap()) as usize;
    if group_revision != revision || group_bytes.len() != 13 + payload_len + 8 {
        return Recovery::IntegrityFailure;
    }
    let group_checksum = read_u64(group_bytes, 13 + payload_len);
    if digest(&group_bytes[..13 + payload_len]) != group_checksum {
        return Recovery::IntegrityFailure;
    }
    Recovery::Frontier(revision)
}

fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pre_certificate_crash_recovers_the_previous_frontier() {
        let complete_group = group(1, b"bounded transaction");
        for cut in 0..complete_group.len() {
            assert_eq!(recover(&complete_group[..cut], &[]), Recovery::Frontier(0));
        }
        assert_eq!(recover(&complete_group, &[]), Recovery::Frontier(0));
    }

    #[test]
    fn every_certificate_tail_cut_recovers_previous_and_complete_recovers_new() {
        let complete_group = group(1, b"bounded transaction");
        let complete_certificate = certificate(1, &complete_group, 0);
        for cut in 0..complete_certificate.len() {
            assert_eq!(
                recover(&complete_group, &complete_certificate[..cut]),
                Recovery::Frontier(0),
                "certificate cut {cut}"
            );
        }
        assert_eq!(
            recover(&complete_group, &complete_certificate),
            Recovery::Frontier(1)
        );
    }

    #[test]
    fn corruption_named_by_a_complete_certificate_fails_closed() {
        let complete_group = group(1, b"bounded transaction");
        let complete_certificate = certificate(1, &complete_group, 0);

        for index in 0..complete_group.len() {
            let mut corrupt = complete_group.clone();
            corrupt[index] ^= 0x01;
            assert_eq!(
                recover(&corrupt, &complete_certificate),
                Recovery::IntegrityFailure,
                "group byte {index}"
            );
        }
        for index in 0..complete_certificate.len() {
            let mut corrupt = complete_certificate.clone();
            corrupt[index] ^= 0x01;
            assert_eq!(
                recover(&complete_group, &corrupt),
                Recovery::IntegrityFailure,
                "certificate byte {index}"
            );
        }
    }

    #[test]
    fn a_complete_certificate_never_falls_back_for_missing_group_bytes() {
        let complete_group = group(1, b"bounded transaction");
        let complete_certificate = certificate(1, &complete_group, 0);
        for cut in 0..complete_group.len() {
            assert_eq!(
                recover(&complete_group[..cut], &complete_certificate),
                Recovery::IntegrityFailure,
                "group cut {cut}"
            );
        }
    }
}
