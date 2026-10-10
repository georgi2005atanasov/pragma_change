use sha2::{Digest, Sha256};

pub(crate) fn hash(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes))
}
