use tokio::sync::mpsc;

pub fn priority_queue<T>() -> (PriorityQueueSender<T>, PriorityQueueReceiver<T>) {
    let (tx_queue_appender, tx_queue) = mpsc::channel(128);
    let (high_pri_tx_queue_appender, high_pri_tx_queue) = mpsc::unbounded_channel();

    (
        PriorityQueueSender {
            tx_queue_appender,
            high_pri_tx_queue_appender,
        },
        PriorityQueueReceiver {
            tx_queue,
            high_pri_tx_queue,
        },
    )
}

#[derive(Debug, thiserror::Error)]
#[error("Priority queue is disconnected")]
pub struct Disconnected;

pub struct PriorityQueueReceiver<T> {
    tx_queue: mpsc::Receiver<T>,
    high_pri_tx_queue: mpsc::UnboundedReceiver<T>,
}

impl<T> PriorityQueueReceiver<T> {
    pub async fn recv(&mut self) -> Option<T> {
        if self.high_pri_tx_queue.is_closed() && self.high_pri_tx_queue.is_empty() {
            return self.tx_queue.recv().await;
        }

        tokio::select! {
            biased;
            v = self.high_pri_tx_queue.recv() => {
                v
            }
            v = self.tx_queue.recv() => {
                v
            }
        }
    }
}

pub struct PriorityQueueSender<T> {
    tx_queue_appender: mpsc::Sender<T>,
    high_pri_tx_queue_appender: mpsc::UnboundedSender<T>,
}

impl<T> PriorityQueueSender<T> {
    pub fn send_high_pri(&self, t: T) -> Result<(), Disconnected> {
        self.high_pri_tx_queue_appender
            .send(t)
            .map_err(|_| Disconnected)
    }

    pub async fn send_low_pri(&self, t: T) -> Result<(), Disconnected> {
        self.tx_queue_appender
            .send(t)
            .await
            .map_err(|_| Disconnected)
    }

    #[expect(dead_code)]
    pub async fn send(&self, t: T, priority: bool) -> Result<(), Disconnected> {
        if priority {
            self.send_high_pri(t)
        } else {
            self.send_low_pri(t).await
        }
    }
}

impl<T> Clone for PriorityQueueSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx_queue_appender: self.tx_queue_appender.clone(),
            high_pri_tx_queue_appender: self.high_pri_tx_queue_appender.clone(),
        }
    }
}
