use stdiomux::frame::{
    Frame,
    sync::{self, FrameReader},
};

pub fn test_serialize_roundtrip_io_std<F: Frame>(fs: &[F]) {
    let mut buf = vec![];
    let mut w = sync::FrameWriter::new(&mut buf);

    for f in fs {
        w.write_frame(f).unwrap();
    }

    let buf = &buf[..];
    let mut r = FrameReader::new(buf);

    for (i, f) in fs.iter().enumerate() {
        let result: F = r.read_frame().unwrap();
        assert_eq!(f, &result, "mismatch at frame {i}")
    }
}

macro_rules! generate_roundtrip_tests {
    ($mod_name:ident, $frame:ty) => {
        mod $mod_name {
            #[test_strategy::proptest]
            fn io_std_roundtrip(fs: Vec<$frame>) {
                super::test_serialize_roundtrip_io_std(&fs);
            }
        }
    };
}

generate_roundtrip_tests!(simple, ::stdiomux::frame::simple::SimpleLengthFrame);
