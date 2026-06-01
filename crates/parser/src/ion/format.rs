use crate::ion::{IonError, IonResult};

pub const HEADER_SIZE: usize = 1024;
pub const FILE_SIGNATURE: [u8; 8] = *b"START\0\0\0";
pub const FILE_TRAILER: [u8; 8] = *b"END\0\0\0\0\0";

pub use crate::ion::version_generated::{
    CURRENT_VERSION, MAX_SUPPORTED_VERSION, MIN_SUPPORTED_VERSION,
};

const _VERSION_POLICY_INVARIANTS: () = {
    assert!(
        MIN_SUPPORTED_VERSION > 0,
        "version 0 is reserved and must never be supported"
    );
    assert!(
        MIN_SUPPORTED_VERSION <= CURRENT_VERSION,
        "CURRENT_VERSION must be supported"
    );
    assert!(
        CURRENT_VERSION <= MAX_SUPPORTED_VERSION,
        "CURRENT_VERSION must be supported"
    );
};

#[inline]
pub fn is_supported(version: u16) -> bool {
    version >= MIN_SUPPORTED_VERSION && version <= MAX_SUPPORTED_VERSION
}

#[inline]
pub fn allow_version(version: u16) -> IonResult<()> {
    if is_supported(version) {
        Ok(())
    } else {
        Err(IonError::UnsupportedFormatVersion(version))
    }
}

pub const CODEC_NONE: u8 = 0;
pub const CODEC_ZSTD: u8 = 1;
pub const ZSTD_LEVEL_MIN: u8 = 1;
pub const ZSTD_LEVEL_MAX: u8 = 22;

#[inline]
pub fn allow_compression(codec: u8, level: u8) -> IonResult<()> {
    match codec {
        CODEC_NONE if level == 0 => Ok(()),
        CODEC_ZSTD if level >= ZSTD_LEVEL_MIN && level <= ZSTD_LEVEL_MAX => Ok(()),
        CODEC_NONE | CODEC_ZSTD => Err(IonError::from(format!(
            "compression level {level} does not match codec {codec}"
        ))),
        other => Err(IonError::UnsupportedCodec(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_supported() {
        assert!(is_supported(CURRENT_VERSION));
    }

    #[test]
    fn allow_decision_matches_is_supported_for_every_u16() {
        for version in 0u16..=u16::MAX {
            match allow_version(version) {
                Ok(()) => assert!(
                    is_supported(version),
                    "allow_version accepted {version} but is_supported says no"
                ),
                Err(IonError::UnsupportedFormatVersion(rejected)) => {
                    assert_eq!(rejected, version, "error must carry the rejected version");
                    assert!(
                        !is_supported(version),
                        "allow_version rejected {version} but is_supported says yes"
                    );
                }
                Err(other) => {
                    panic!("allow_version returned wrong error variant for {version}: {other:?}")
                }
            }
        }
    }

    #[test]
    fn signature_and_trailer_are_eight_bytes() {
        assert_eq!(FILE_SIGNATURE.len(), 8);
        assert_eq!(FILE_TRAILER.len(), 8);
    }

    #[test]
    fn allow_compression_accepts_matching_codec_and_level() {
        assert!(allow_compression(CODEC_NONE, 0).is_ok());
        assert!(allow_compression(CODEC_ZSTD, ZSTD_LEVEL_MIN).is_ok());
        assert!(allow_compression(CODEC_ZSTD, ZSTD_LEVEL_MAX).is_ok());
    }

    #[test]
    fn allow_compression_rejects_level_that_does_not_match_codec() {
        assert!(allow_compression(CODEC_NONE, 1).is_err());
        assert!(allow_compression(CODEC_ZSTD, 0).is_err());
        assert!(allow_compression(CODEC_ZSTD, ZSTD_LEVEL_MAX + 1).is_err());
    }

    #[test]
    fn allow_compression_rejects_unknown_codec() {
        match allow_compression(2, 0) {
            Err(IonError::UnsupportedCodec(2)) => {}
            other => panic!("expected UnsupportedCodec(2), got {other:?}"),
        }
    }
}
