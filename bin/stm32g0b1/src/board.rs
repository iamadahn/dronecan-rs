use embassy_stm32::{bind_interrupts, can, peripherals::FDCAN1, time::Hertz};

pub const BITRATE: u32 = 1_000_000;

bind_interrupts!(struct Irqs {
    TIM16_FDCAN_IT0 => can::IT0InterruptHandler<FDCAN1>;
    TIM17_FDCAN_IT1 => can::IT1InterruptHandler<FDCAN1>;
});

pub fn init() -> can::Can<'static> {
    use embassy_stm32::rcc::*;
    let mut config = embassy_stm32::Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(8_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV1,
        mul: PllMul::MUL16,
        divp: Some(PllPDiv::DIV2),
        divq: Some(PllQDiv::DIV2),
        divr: Some(PllRDiv::DIV2),
    });
    config.rcc.mux.fdcansel = mux::Fdcansel::PLL1_Q;
    config.rcc.sys = Sysclk::PLL1_R;
    let p = embassy_stm32::init(config);
    let mut can = can::CanConfigurator::new(p.FDCAN1, p.PD0, p.PD1, Irqs);
    can.properties().set_extended_filter(
        can::filter::ExtendedFilterSlot::_0,
        can::filter::ExtendedFilter::accept_all_into_fifo1(),
    );
    let mut fd_config = can.config();
    fd_config.global_filter = can::config::GlobalFilter::reject_all();
    fd_config.tx_buffer_mode = can::config::TxBufferMode::Priority;
    #[cfg(feature = "canfd")]
    {
        fd_config.frame_transmit = can::config::FrameTransmissionConfig::AllowFdCan;
    }
    can.set_config(fd_config);
    can.set_bitrate(BITRATE);
    defmt::info!("CAN initialized at {} bps", BITRATE);
    can.start(can::OperatingMode::NormalOperationMode)
}
