//! Exposes the [`CaligulaFacade`], which is a facade that orchestrates all
//! "high-level" work and tracks the state of worker tasks.

use std::path::PathBuf;

pub use self::{
    child::SpawnDaemonError,
    disks::DiskList,
    escalation::EscalationMethod,
    real::make_real_facade,
    workflow::{
        Orchestrator, OrchestratorExt,
        write_verify::{WVState, WriteVerifyWorkflow},
    },
};
use crate::facade::{analyze_input::FileAnalysis, workflow::hash::HashWorkflow};

mod analyze_input;
mod child;
mod disks;
mod escalation;
mod real;
pub mod watch;
pub mod workflow;

/// Helper to generate our facade definition based on a list of traits it
/// combines.
macro_rules! gen_facade {
    ($($traits:path,)*) => {
        /// Main facade for UI implementations to interact with the rest of the
        /// program's logic.
        ///
        /// This trait is split up into several subtraits, each representing a different
        /// kind of shared action the UI can take.
        pub trait CaligulaFacade:
            Sync + Send + 'static $(+ $traits)*
        {
        }


        impl<F> CaligulaFacade for F where
            F: Sync + Send + 'static $(+ $traits)*
        {
        }
    };
}

gen_facade!(
    DiskWatcher,
    FileAnalyzer,
    Escalator,
    Orchestrator<WriteVerifyWorkflow>,
    Orchestrator<HashWorkflow>,
);

// Represents an interface to the disk querying subsystem.
pub trait DiskWatcher {
    /// Get a handle for watching the list of disks available. This may update
    /// as disks are added and removed to the system.
    ///
    /// Although this returns a handle immediately, the initial results may take
    /// a while to load.
    #[expect(unused, reason = "Stub interface created for later use.")]
    fn watch_disks(&self) -> watch::Watch<DiskList>;
}

/// Represents an interface to the file analysis subsystem.
pub trait FileAnalyzer {
    /// Analyze a file to guess how we should handle it.
    ///
    /// This is a fairly fast operation, expected to take 1-3 seconds to run.
    ///
    /// Returns the results of the analysis, or an error if the file could not
    /// be read. This method is fault-tolerant, so the only errors that can
    /// cause this operation to fail are I/O errors.
    ///
    /// The intended workflow is that you call it once, to fill up your UI's
    /// wizard with data, and then ask the user for more information if
    /// there's anything that's not certain.
    #[expect(unused, reason = "Stub interface created for later use.")]
    async fn analyze_file(&self, input: PathBuf) -> std::io::Result<FileAnalysis>;
}

/// Represents an interface to the privilege escalation subsystem.
pub trait Escalator {
    /// Attempt to spawn a child process as root using the provided escalation
    /// method (or [`None`] to automatically guess which one to use).
    ///
    /// Returns [`Ok`] if we successfully managed to escalate, or an error if we
    /// failed. This operation is idempotent: if we were already escalated
    /// before this was called, returns [`Ok`].
    ///
    /// Once this is called, all future workflows will be routed through the
    /// escalated child process rather than executing at the parent's
    /// permission level!
    ///
    /// If your requested method involves the terminal, you should switch back
    /// to the non-alternate screen before calling this.
    async fn escalate(&self, method: Option<EscalationMethod>) -> Result<(), SpawnDaemonError>;

    /// Returns whether or not we have a child process running as root.
    #[expect(unused, reason = "Stub interface created for later use.")]
    fn is_escalated(&self) -> bool;
}
