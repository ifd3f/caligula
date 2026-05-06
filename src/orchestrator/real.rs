use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use futures::StreamExt;
use tokio::sync::watch;

use super::herder_facade::{DaemonError, HerderFacade, StartWriterError};
use crate::{
    byteseries::ByteSeries,
    escalation::EscalationMethod,
    herder_api::write_verify::WriteVerifyEvent,
    orchestrator::{
        HashStarted, Orchestrator, StartHashParams, WriteVerifyParams, WriteVerifyStarted,
        WriterState,
        hash::{HashError, HashReading, HashState, run_bg_hash},
    },
};

/// Actual orchestrator implementation used by Caligula.
pub struct OrchestratorImpl<H> {
    inner: tokio::sync::Mutex<Inner<H>>,
}

struct Inner<H> {
    // TODO: get rid of the entire herder facade thing altogether. just assimilate the good parts
    // into orchestrator.
    h: H,
    escalation: Option<Option<EscalationMethod>>,
}

impl<H> OrchestratorImpl<H> {
    pub fn new(h: H) -> Self {
        Self {
            inner: Inner {
                h,
                escalation: None,
            }
            .into(),
        }
    }
}

impl<H: HerderFacade + Send + 'static> Orchestrator for OrchestratorImpl<H> {
    async fn start_hash(&self, params: StartHashParams) -> HashStarted {
        let (tracker, jh) = run_bg_hash(params).await;
        let (tx, rx) = watch::channel(HashState::new(Instant::now()));
        tokio::task::spawn_local(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(50));
            loop {
                interval.tick().await;
                if jh.is_finished() {
                    let r = match jh.join() {
                        Ok(Ok(x)) => Ok(x),
                        Ok(Err(e)) => Err(HashError::ReadError(e)),
                        Err(_) => Err(HashError::Panicked),
                    };
                    tx.send_modify(move |s| {
                        s.result = Some(r);
                    });
                    return;
                }
                let read = tracker.load(Ordering::Relaxed);
                let now = Instant::now();
                tx.send_modify(|s| s.read_series.push(now, read));
            }
        });
        HashStarted {
            state: super::watch::Watch { rx },
        }
    }

    async fn start_write_verify(
        &self,
        params: WriteVerifyParams,
    ) -> Result<WriteVerifyStarted, StartWriterError<WriteVerifyEvent>> {
        tracing::info!("Requesting herder to start");

        // request the herder to start the action
        let mut inner = self.inner.lock().await;
        let esc = inner.escalation.is_some();
        let handle = inner.h.start_herd(params.make_child_config(), esc).await?;
        drop(inner);

        // create state reduction task
        let (tx_state, rx_state) = tokio::sync::watch::channel(WriterState::initial(
            Instant::now(),
            !params.compression.is_identity(),
            handle.initial_info.input_file_bytes,
        ));
        let mut events = handle.events;
        let _jh = tokio::spawn(async move {
            while !tx_state.borrow().is_finished() && !tx_state.is_closed() {
                let event = events.next().await;
                tx_state.send_modify(move |state| {
                    *state = std::mem::take(state).on_status(Instant::now(), event);
                });
            }
        });
        let state = super::watch::Watch { rx: rx_state };

        Ok(WriteVerifyStarted {
            start: handle.initial_info,
            state,
        })
    }

    async fn escalate(&self, method: Option<EscalationMethod>) -> Result<(), DaemonError> {
        let mut inner = self.inner.lock().await;
        inner.escalation = Some(method);
        inner.h.ensure_escalated_daemon().await?;
        Ok(())
    }

    fn is_escalated(&self) -> bool {
        todo!()
    }
}
