use itertools::Itertools;

use crate::io_graph::{RecvBytes, Worker};

#[derive(Debug, thiserror::Error)]
pub enum VerifierError {
    #[error("{0}")]
    Ver(#[from] VerificationFailure),
    #[error("Error receiving from expected buffer: {0}")]
    ExpRx(std::io::Error),
    #[error("Error receiving from compare buffer: {0}")]
    CmpRx(std::io::Error),
}

#[derive(Debug, thiserror::Error, Clone)]
#[error("Verification failure at index {index}: {kind}")]
pub struct VerificationFailure {
    pub index: u64,
    pub kind: MismatchKind,
}

impl VerificationFailure {
    pub fn new(index: u64, kind: MismatchKind) -> Self {
        Self { index, kind }
    }
}

#[derive(Debug, thiserror::Error, Clone, Copy)]
pub enum MismatchKind {
    #[error("Comparison ran out of bytes")]
    Eof,
    #[error("Bytes were mismatched")]
    Mismatch,
}

impl MismatchKind {
    pub fn with_index(self, index: u64) -> VerificationFailure {
        VerificationFailure { index, kind: self }
    }
}

pub struct Verifier {
    _private: (),
}

impl Verifier {
    pub fn new() -> Box<Self> {
        Box::new(Self { _private: () })
    }
}

impl<I, Rx> Worker<(Rx, I)> for Verifier
where
    I: IntoIterator<Item = Rx>,
    Rx: RecvBytes,
{
    type Error = VerifierError;
    type Output = ();

    fn run(
        self: Box<Self>,
        _context: &crate::io_graph::GraphContext,
        (mut expected, compare): (Rx, I),
    ) -> Result<Self::Output, Self::Error> {
        let mut compare = compare.into_iter().collect_vec();
        let mut index = 0;

        while let Some(exp) = expected.recv().map_err(VerifierError::ExpRx)? {
            for v in compare.iter_mut() {
                let cmp = v
                    .recv()
                    .map_err(VerifierError::CmpRx)?
                    .ok_or(MismatchKind::Eof.with_index(index))?;

                if cmp != exp {
                    Err(MismatchKind::Mismatch.with_index(index))?
                }
            }

            index += exp.len() as u64;
        }

        Ok(())
    }
}
