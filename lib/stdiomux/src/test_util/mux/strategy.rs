use std::collections::VecDeque;
use std::convert::identity;
use std::fmt::Debug;
use std::iter;
use std::sync::Arc;

use super::action::*;
use bytes::Bytes;
use proptest::prelude::*;
use proptest::sample::SizeRange;
use proptest::strategy::Strategy;
use proptest::test_runner::TestRng;

/// Returns a proptest Strategy that generates a valid list of actions to perform on a single channel.
pub fn random_channel_strat(
    stream_len: impl Into<SizeRange> + Clone,
    payload_size: impl Into<SizeRange> + Clone,
    max_concurrency: usize,
) -> impl Strategy<Value = Vec<SidedAction<ChannelAction>>> + Clone {
    random_duplex_stream_actions_strat(stream_len, payload_size, max_concurrency).prop_flat_map(
        |(req, res)| {
            interlace_strategy(vec![
                proptest::strategy::Just(
                    wrap_sided(SidedAction::Client, SidedAction::Server, req.into_iter())
                        .collect::<Vec<_>>(),
                ),
                proptest::strategy::Just(
                    wrap_sided(SidedAction::Server, SidedAction::Client, res.into_iter())
                        .collect::<Vec<_>>(),
                ),
            ])
            .prop_map(|xs| xs.into_iter().map(|(_, x)| x).collect())
        },
    )
}

fn wrap_sided<'a, I, T, R>(
    txr: T,
    rxr: R,
    stream: I,
) -> impl Iterator<Item = SidedAction<ChannelAction>> + 'a
where
    T: Fn(ChannelAction) -> SidedAction<ChannelAction> + 'a,
    R: Fn(ChannelAction) -> SidedAction<ChannelAction> + 'a,
    I: Iterator<Item = StreamAction> + 'a,
{
    let close = [
        txr(ChannelAction::CloseTx),
        rxr(ChannelAction::AssertRxClosed),
    ];
    stream
        .into_iter()
        .map(move |x| match x {
            StreamAction::Tx(bytes) => txr(ChannelAction::Tx(bytes)),
            StreamAction::Rx(bytes) => rxr(ChannelAction::Rx(bytes)),
        })
        .chain(close)
}

/// Returns a proptest Strategy that generates a valid list of actions to perform on both directions of a byte stream.
pub fn random_duplex_stream_actions_strat(
    stream_len: impl Into<SizeRange> + Clone,
    payload_size: impl Into<SizeRange> + Clone,
    max_concurrency: usize,
) -> impl Strategy<Value = (Vec<StreamAction>, Vec<StreamAction>)> + Clone {
    proptest::strategy::Just((
        random_stream_actions_strat(stream_len.clone(), payload_size.clone(), max_concurrency),
        random_stream_actions_strat(stream_len.clone(), payload_size.clone(), max_concurrency),
    ))
    .prop_ind_flat_map(identity)
}

/// Returns a proptest Strategy that generates a valid list of actions to perform on a single direction of a byte stream.
///
/// It will transmit bytes, then receive those same bytes, but with various combinations of stuff-in-flightness.
pub fn random_stream_actions_strat(
    stream_len: impl Into<SizeRange> + Clone,
    payload_size: impl Into<SizeRange> + Clone,
    max_concurrency: usize,
) -> impl Strategy<Value = Vec<StreamAction>> + Clone {
    random_bytes_list(stream_len, payload_size).prop_perturb(move |bss, mut rng| {
        random_req_res(bss, &mut rng, max_concurrency).collect::<Vec<_>>()
    })
}

/// Given a stream of bytes, returns a sequence of transmits and receives in the expected order.
pub fn random_req_res<'a>(
    stream_len: impl IntoIterator<Item = Bytes> + 'a,
    rng: &'a mut TestRng,
    max_concurrency: usize,
) -> impl Iterator<Item = StreamAction> + 'a {
    let mut rxq = VecDeque::<Bytes>::new();
    let mut txq = stream_len.into_iter().collect::<VecDeque<Bytes>>();

    iter::from_fn(move || {
        // check what actions are available
        let tx = if rxq.len() < max_concurrency {
            txq.pop_front()
        } else {
            None
        };
        let rx = rxq.pop_front();

        match (tx, rx) {
            // no more actions -- quit
            (None, None) => None,

            // only rx: drain the rx by one
            (None, Some(rx)) => Some(StreamAction::Rx(rx)),

            // only tx: return tx but also add it to the rxq
            (Some(tx), None) => {
                rxq.push_back(tx.clone());
                Some(StreamAction::Tx(tx))
            }

            // if both are available, randomly pick one and return the other back into its respective queue
            (Some(tx), Some(rx)) => match rng.next_u32() % 2 {
                0 => {
                    // put rx back
                    rxq.push_front(rx);

                    // do a tx
                    rxq.push_back(tx.clone());
                    Some(StreamAction::Tx(tx))
                }
                _ => {
                    // put tx back
                    txq.push_front(tx);

                    // do a rx
                    Some(StreamAction::Rx(rx))
                }
            },
        }
    })
}

/// Returns a proptest Strategy that generates a random stream of [`Bytes`] of the given size ranges.
pub fn random_bytes_list(
    stream_len: impl Into<SizeRange>,
    payload_size: impl Into<SizeRange>,
) -> impl Strategy<Value = Vec<Bytes>> + Clone {
    proptest::collection::vec(random_payload(payload_size), stream_len)
}

/// Returns a proptest Strategy that generates random [`Bytes`] of the given size range.
pub fn random_payload(size: impl Into<SizeRange>) -> impl Strategy<Value = Bytes> + Clone {
    proptest::collection::vec(any::<u8>(), size).prop_map(|bs| Bytes::from_owner(bs))
}

/// Returns a proptest Strategy that generates interlaced `(usize, T)` pairs from multiple strategies.
///
/// Each input strategy produces a sequence of values, and the result interlaces them randomly while preserving
/// intra-sequence ordering.
pub fn interlace_strategy<S, I, T>(
    strategies: Vec<S>,
) -> impl Strategy<Value = Vec<(usize, T)>> + Clone
where
    S: Strategy<Value = I> + Clone + 'static,
    I: IntoIterator<Item = T> + Debug + 'static,
    T: Debug + 'static,
{
    let cat = Arc::new(interlace_concat_strategy(strategies.clone()));
    let rr = Arc::new(interlace_round_robin_strategy(strategies.clone()));
    let rand = Arc::new(interlace_random_strategy(strategies));

    proptest::strategy::TupleUnion::new(((1, cat), (1, rr), (10, rand)))
}

/// Returns a proptest Strategy that generates interlaced `(usize, T)` pairs from multiple strategies.
///
/// Each input strategy produces a sequence of values, and the result interlaces them randomly while preserving
/// intra-sequence ordering.
pub fn interlace_random_strategy<S, I, T>(
    strategies: Vec<S>,
) -> impl Strategy<Value = Vec<(usize, T)>> + Clone
where
    S: Strategy<Value = I> + Clone + 'static,
    I: IntoIterator<Item = T> + Debug + 'static,
    T: Debug + 'static,
{
    proptest::strategy::Just(strategies)
        .prop_ind_flat_map(identity)
        .prop_perturb(|xss, mut rng| interlace_random(xss, &mut rng).collect())
        .boxed()
}

/// Returns a proptest Strategy that generates interlaced `(usize, T)` pairs from multiple strategies.
///
/// Each input strategy produces a sequence of values, and those sequences are interlaced with each other one after another.
pub fn interlace_round_robin_strategy<S, I, T>(
    strategies: Vec<S>,
) -> impl Strategy<Value = Vec<(usize, T)>> + Clone
where
    S: Strategy<Value = I> + Clone + 'static,
    I: IntoIterator<Item = T> + Debug + 'static,
    T: Debug + 'static,
{
    proptest::strategy::Just(strategies)
        .prop_ind_flat_map(identity)
        .prop_map(|xss| interlace_round_robin(xss).collect())
        .boxed()
}

/// Returns a proptest Strategy that generates interlaced `(usize, T)` pairs from multiple strategies.
///
/// Each input strategy produces a sequence of values, and those sequences are concatenated with each other.
pub fn interlace_concat_strategy<S, I, T>(
    strategies: Vec<S>,
) -> impl Strategy<Value = Vec<(usize, T)>> + Clone
where
    S: Strategy<Value = I> + Clone + 'static,
    I: IntoIterator<Item = T> + Debug + 'static,
    T: Debug + 'static,
{
    proptest::strategy::Just(strategies)
        .prop_ind_flat_map(identity)
        .prop_map(|xss| {
            xss.into_iter()
                .enumerate()
                .flat_map(|(i, xs)| xs.into_iter().map(move |x| (i, x)))
                .collect()
        })
        .boxed()
}

/// Interlaces multiple iterators randomly while preserving order within each vector.
///
/// Uses proptest's TestRng for deterministic testing.
fn interlace_random<'a, I, T>(
    xss: Vec<I>,
    rng: &'a mut TestRng,
) -> impl Iterator<Item = (usize, T)> + 'a
where
    I: IntoIterator<Item = T> + 'a,
    T: 'a,
{
    let mut iters: Vec<I::IntoIter> = xss.into_iter().map(|v| v.into_iter()).collect();

    std::iter::from_fn(move || {
        while !iters.is_empty() {
            // pick a random item
            let i = rng.next_u32() as usize % iters.len();

            // try pulling from it
            let Some(val) = iters[i].next() else {
                // it's dead, remove it
                drop(iters.remove(i));
                continue;
            };

            return Some((i, val));
        }
        None
    })
}

/// Interlaces multiple iterators in a round-robin style.
pub fn interlace_round_robin<'a, I, T>(xss: Vec<I>) -> impl Iterator<Item = (usize, T)> + 'a
where
    I: IntoIterator<Item = T> + 'a,
    T: 'a,
{
    let mut iters: VecDeque<(usize, I::IntoIter)> =
        xss.into_iter().map(|v| v.into_iter()).enumerate().collect();

    std::iter::from_fn(move || {
        while let Some((i, mut iter)) = iters.pop_front() {
            // try pulling from it
            let Some(val) = iter.next() else {
                // it's dead, don't put it back into the queue and try the next one
                drop(iter);
                continue;
            };

            // success, put it back in the queue
            iters.push_back((i, iter));
            return Some((i, val));
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_strategy::proptest(
        ProptestConfig {
            timeout: 10,
            .. ProptestConfig::default()
        },
    )]
    #[test_log::test]
    fn random_stream_actions_strat_has_tx_for_every_rx_in_correct_order(
        #[strategy(random_stream_actions_strat(0..10, 1..24, usize::MAX))] actions: Vec<
            StreamAction,
        >,
    ) {
        let mut in_flight = VecDeque::new();
        for a in actions {
            match a {
                StreamAction::Tx(bytes) => in_flight.push_back(bytes),
                StreamAction::Rx(bytes) => {
                    let actual = in_flight.pop_front().expect("bytes are missing");
                    assert_eq!(actual, bytes)
                }
            }
        }
        assert!(in_flight.is_empty(), "there are more txs than rxs");
    }
}
