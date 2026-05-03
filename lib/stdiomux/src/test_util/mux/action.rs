use bytes::Bytes;

/// Actions split by client side and server side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidedAction<T> {
    Client(T),
    Server(T),
}

/// Actions to perform on a single end of a single channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelAction {
    /// Transmit the given payload.
    Tx(Bytes),
    /// Assert that the next payload received matches this payload.
    Rx(Bytes),
    /// Close the transmit end.
    CloseTx,
    /// Assert the receive end is closed.
    AssertRxClosed,
}

/// Actions to perform on both ends of a single-direction stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamAction {
    /// Transmit the given payload.
    Tx(Bytes),
    /// Assert that the next payload received matches this payload.
    Rx(Bytes),
}
