use crate::hash::HashAlg;
use anyhow::anyhow;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

/// Common filenames of hash files.
const HASH_FILES: [(HashAlg, &str); 24] = [
    (HashAlg::Md5, "md5sum.txt"),
    (HashAlg::Md5, "md5sums.txt"),
    (HashAlg::Md5, "MD5SUM"),
    (HashAlg::Md5, "MD5SUMS"),
    (HashAlg::Sha1, "sha1sum.txt"),
    (HashAlg::Sha1, "sha1sums.txt"),
    (HashAlg::Sha1, "SHA1SUM"),
    (HashAlg::Sha1, "SHA1SUMS"),
    (HashAlg::Sha224, "sha224sum.txt"),
    (HashAlg::Sha224, "sha224sums.txt"),
    (HashAlg::Sha224, "SHA224SUM"),
    (HashAlg::Sha224, "SHA224SUMS"),
    (HashAlg::Sha256, "sha256sum.txt"),
    (HashAlg::Sha256, "sha256sums.txt"),
    (HashAlg::Sha256, "SHA256SUM"),
    (HashAlg::Sha256, "SHA256SUMS"),
    (HashAlg::Sha384, "sha384sum.txt"),
    (HashAlg::Sha384, "sha384sums.txt"),
    (HashAlg::Sha384, "SHA384SUM"),
    (HashAlg::Sha384, "SHA384SUMS"),
    (HashAlg::Sha512, "sha512sum.txt"),
    (HashAlg::Sha512, "sha512sums.txt"),
    (HashAlg::Sha512, "SHA512SUM"),
    (HashAlg::Sha512, "SHA512SUMS"),
];

pub fn find_hash(input: &PathBuf) -> Option<(HashAlg, &str, Vec<u8>)> {
    for (alg, hash_file) in HASH_FILES {
        let hash_filepath = input.parent()?.join(hash_file);
        match File::open(&hash_filepath) {
            Ok(file) => match parse_hashfile(BufReader::new(file), input.file_name()?.to_str()?) {
                Ok(Some(expected_hash)) => return Some((alg, hash_file, expected_hash)),
                Ok(None) => tracing::warn!("Hash not found in {}", hash_filepath.display()),
                Err(e) => tracing::warn!("{e}"),
            },
            Err(e) => tracing::warn!("{e}"),
        }
    }

    None
}

fn parse_hashfile(hash_file: impl BufRead, input_file: &str) -> anyhow::Result<Option<Vec<u8>>> {
    for line in hash_file.lines() {
        match line?.split_once(char::is_whitespace) {
            Some((hash, file)) if file.trim_start() == input_file => {
                match base16::decode(hash.as_bytes()) {
                    Ok(decoded) => return Ok(Some(decoded)),
                    Err(err) => {
                        eprintln!("Failed to decode hash");
                        return Err(err.into());
                    }
                }
            }
            None => return Err(anyhow!("Invalid hash file")),
            _ => continue,
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::parse_hashfile;
    use std::io::Cursor;

    #[test]
    fn parse_simple_hashfile() {
        let mut cursor = Cursor::new(
            "bceb3dded8935c1d3521c475a69ae557e082839b46d921c8b400524470b5c965  archlinux-2024.11.01-x86_64.iso"
        );

        assert_eq!(
            parse_hashfile(&mut cursor, "archlinux-2024.11.01-x86_64.iso").unwrap(),
            Some(
                base16::decode("bceb3dded8935c1d3521c475a69ae557e082839b46d921c8b400524470b5c965")
                    .unwrap()
            ),
        );
    }

    #[test]
    fn parse_complicated_hashfile() {
        let mut cursor = Cursor::new(
        "bceb3dded8935c1d3521c475a69ae557e082839b46d921c8b400524470b5c965  archlinux-2024.11.01-x86_64.iso\n\
        bceb3dded8935c1d3521c475a69ae557e082839b46d921c8b400524470b5c965  archlinux-x86_64.iso\n\
        c64745475da03a31f270b92e9abfbe7b6315596c7c97b17ef9a373433562a4a4  archlinux-bootstrap-2024.11.01-x86_64.tar.zst\n\
        c64745475da03a31f270b92e9abfbe7b6315596c7c97b17ef9a373433562a4a4  archlinux-bootstrap-x86_64.tar.zst",
        );

        for (filename, hash) in &[
            (
                "archlinux-2024.11.01-x86_64.iso",
                "bceb3dded8935c1d3521c475a69ae557e082839b46d921c8b400524470b5c965",
            ),
            (
                "archlinux-x86_64.iso",
                "bceb3dded8935c1d3521c475a69ae557e082839b46d921c8b400524470b5c965",
            ),
            (
                "archlinux-bootstrap-2024.11.01-x86_64.tar.zst",
                "c64745475da03a31f270b92e9abfbe7b6315596c7c97b17ef9a373433562a4a4",
            ),
            (
                "archlinux-bootstrap-x86_64.tar.zst",
                "c64745475da03a31f270b92e9abfbe7b6315596c7c97b17ef9a373433562a4a4",
            ),
        ] {
            assert_eq!(
                parse_hashfile(&mut cursor, filename).unwrap(),
                Some(base16::decode(hash).unwrap())
            );
        }
    }
}
