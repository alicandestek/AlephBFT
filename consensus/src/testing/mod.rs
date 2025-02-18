#![cfg(test)]
mod alerts;
mod byzantine;
mod consensus;
mod crash;
mod crash_recovery;
mod creation;
mod dag;
mod unreliable;

use crate::{
    run_session, Config, DelayConfig, LocalIO, Network as NetworkT, NodeCount, NodeIndex,
    SpawnHandle, TaskHandle, Terminator,
};
use aleph_bft_mock::{
    Data, DataProvider, FinalizationHandler, Hasher64, Keychain, Loader, Network as MockNetwork,
    PartialMultisignature, ReconnectSender as ReconnectSenderGeneric, Saver, Signature, Spawner,
};
use futures::channel::{mpsc::UnboundedReceiver, oneshot};
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};

pub type NetworkData = crate::NetworkData<Hasher64, Data, Signature, PartialMultisignature>;

pub type Network = MockNetwork<NetworkData>;
pub type ReconnectSender = ReconnectSenderGeneric<NetworkData>;

pub fn init_log() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::max())
        .is_test(true)
        .try_init();
}

pub fn complete_oneshot<T: std::fmt::Debug>(t: T) -> oneshot::Receiver<T> {
    let (tx, rx) = oneshot::channel();
    tx.send(t).unwrap();
    rx
}

pub fn gen_config(node_ix: NodeIndex, n_members: NodeCount) -> Config {
    let delay_config = DelayConfig {
        tick_interval: Duration::from_millis(5),
        unit_rebroadcast_interval_min: Duration::from_millis(400),
        unit_rebroadcast_interval_max: Duration::from_millis(500),
        //50, 50, 50, 50, ...
        unit_creation_delay: Arc::new(|_| Duration::from_millis(50)),
        //100, 100, 100, ...
        coord_request_delay: Arc::new(|_| Duration::from_millis(100)),
        //3, 1, 1, 1, ...
        coord_request_recipients: Arc::new(|t| if t == 0 { 3 } else { 1 }),
        // 50, 50, 50, 50, ...
        parent_request_delay: Arc::new(|_| Duration::from_millis(50)),
        // 1, 1, 1, ...
        parent_request_recipients: Arc::new(|_| 1),
        // 50, 50, 50, 50, ...
        newest_request_delay: Arc::new(|_| Duration::from_millis(50)),
    };
    Config {
        node_ix,
        session_id: 0,
        n_members,
        delay_config,
        max_round: 5000,
    }
}

pub fn spawn_honest_member(
    spawner: Spawner,
    node_index: NodeIndex,
    n_members: NodeCount,
    units: Vec<u8>,
    network: impl 'static + NetworkT<NetworkData>,
) -> (
    UnboundedReceiver<Data>,
    Arc<Mutex<Vec<u8>>>,
    oneshot::Sender<()>,
    TaskHandle,
) {
    let data_provider = DataProvider::new();
    let (finalization_handler, finalization_rx) = FinalizationHandler::new();
    let config = gen_config(node_index, n_members);
    let (exit_tx, exit_rx) = oneshot::channel();
    let spawner_inner = spawner;
    let unit_loader = Loader::new(units);
    let saved_state = Arc::new(Mutex::new(vec![]));
    let unit_saver = Saver::new(saved_state.clone());
    let local_io = LocalIO::new(data_provider, finalization_handler, unit_saver, unit_loader);
    let member_task = async move {
        let keychain = Keychain::new(n_members, node_index);
        run_session(
            config,
            local_io,
            network,
            keychain,
            spawner_inner,
            Terminator::create_root(exit_rx, "AlephBFT-member"),
        )
        .await
    };
    let handle = spawner.spawn_essential("member", member_task);
    (finalization_rx, saved_state, exit_tx, handle)
}
