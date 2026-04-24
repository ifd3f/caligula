use crate::mux::{MuxReader, MuxWriter, initialize_mux};

pub struct MuxTestHarness {
    pub aw: MuxWriter<tokio_pipe::PipeWrite>,
    pub ar: MuxReader<tokio_pipe::PipeRead>,
    pub bw: MuxWriter<tokio_pipe::PipeWrite>,
    pub br: MuxReader<tokio_pipe::PipeRead>,
}

pub async fn setup_mux_layer_test() -> MuxTestHarness {
    let (ar, bw) = tokio_pipe::pipe().unwrap();
    let (br, aw) = tokio_pipe::pipe().unwrap();
    let (a, b) = tokio::join!(initialize_mux(aw, ar), initialize_mux(bw, br));
    let ((aw, ar), (bw, br)) = (a.unwrap(), b.unwrap());
    MuxTestHarness { aw, ar, bw, br }
}
