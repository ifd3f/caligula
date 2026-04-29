use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    sync::watch,
};

use crate::{
    channel::state::ChannelBuffer,
    frame::{AsyncReadExt as _, AsyncWriteExt as _, Frame, MuxControlHeader},
    mux::state::MuxState,
    queue::priority_queue,
};

pub struct AsyncMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    r: R,
    w: W,
}

impl<R, W> AsyncMuxController<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    pub async fn open(mut r: R, mut w: W) -> std::io::Result<Self> {
        w.write_frame_async(&Frame::MuxControl(MuxControlHeader::Hello))
            .await?;
        w.flush().await?;
        let read = r.read_frame_async().await?;
        if read != Frame::MuxControl(MuxControlHeader::Hello) {
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "did not receive a hello from the other end",
            ))?;
        }

        let (queue_tx, mut queue_rx) = priority_queue::<Frame>();
        let (state_tx, state_rx) = watch::channel(MuxState::<Buffer>::opened());

        #[derive(Debug, Default)]
        struct Buffer {}

        impl ChannelBuffer for Buffer {
            fn poll_rx_capacity(
                &mut self,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<std::num::NonZero<u64>>> {
                todo!()
            }

            fn accept_rx(
                &mut self,
                data: bytes::Bytes,
            ) -> Result<(), crate::channel::state::AcceptRxError> {
                todo!()
            }

            fn poll_tx(
                &mut self,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<bytes::Bytes>> {
                todo!()
            }
        }

        todo!()
        /*
        let inner_value = Arc::new(Inner {
            channels: DashMap::new(),
        });

        let _rx_actor = tokio::spawn({
            let inner = inner_value.clone();
            let queue_tx = queue_tx.clone();
            async move {
                loop {
                    let f = r.read_frame_async().await.expect("failed to read frame");
                    inner.on_recv(f);
                }
            }
        });

        let _tx_actor = tokio::spawn(async move {
            loop {
                let Some(v) = queue_rx.recv().await else {
                    return;
                };
                w.write_frame_async(v).await.unwrap()
            }
        });
        Ok(Self {
            inner: inner_value,
            queue_tx,
            _phantom: PhantomData,
        })
        */
    }

    // pub async fn open_channel(
    //     &self,
    //     channel_id: ChannelId,
    //     initial_rx_buffer: usize,
    // ) -> Result<ChannelIo, OpenChannelError> {
    //     todo!()
    // }
}
