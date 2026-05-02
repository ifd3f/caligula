use stdiomux::frame::*;

pub fn test_serialize_roundtrip_io_std<F: Frame>(fs: Vec<F>) {
    let mut buf = vec![];
    let mut w = sync::FrameWriter::new(&mut buf);

    for f in fs.clone() {
        w.write_frame(f).unwrap();
    }

    let buf = &buf[..];
    let mut r = sync::FrameReader::new(buf);

    for (i, f) in fs.iter().enumerate() {
        let result: F = r.read_frame().unwrap();
        assert_eq!(f, &result, "mismatch at frame {i}")
    }
}

pub async fn test_serialize_roundtrip_io_tokio<F: Frame>(fs: Vec<F>) {
    let mut buf = vec![];
    let mut w = tokio::FrameWriter::new(&mut buf);

    for f in fs.clone() {
        w.write_frame(f).await.unwrap();
    }

    let buf = &buf[..];
    let mut r = tokio::FrameReader::new(buf);

    for (i, f) in fs.iter().enumerate() {
        let result: F = r.read_frame().await.unwrap();
        assert_eq!(f, &result, "mismatch at frame {i}")
    }
}

macro_rules! generate_roundtrip_tests {
    ($mod_name:ident, $frame:ty) => {
        mod $mod_name {
            #[test_strategy::proptest]
            fn roundtrip_sync(fs: Vec<$frame>) {
                super::test_serialize_roundtrip_io_std(fs);
            }

            #[test_strategy::proptest(async = "tokio")]
            async fn roundtrip_tokio(fs: Vec<$frame>) {
                super::test_serialize_roundtrip_io_tokio(fs).await;
            }
        }
    };
}

generate_roundtrip_tests!(simple, ::stdiomux::frame::simple::SimpleLengthFrame);
generate_roundtrip_tests!(simple_mux, ::stdiomux::frame::simple::SimpleMuxFrame);
