#![no_std]
#![no_main]

mod board;
mod node_config;
mod runner;

use defmt_rtt as _;
use embassy_executor::Spawner;
use panic_probe as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let can = board::init();
    let config = node_config::config();
    defmt::info!(
        "Starting DroneCAN node {}, {}",
        config.id.get(),
        config.name.as_str()
    );
    runner::run(can, config).await;
}
