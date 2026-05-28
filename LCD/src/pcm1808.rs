use embassy_stm32::Peri;
use embassy_stm32::dma::{self, Transfer, TransferOptions};
use embassy_stm32::gpio::{AfType, Flex, OutputType, Pull};
use embassy_stm32::pac::sai::vals;
use embassy_stm32::peripherals::SAI1;
use embassy_stm32::sai::{self, Mode, TxRx};

trait SubBlockIndex {
    const INDEX: usize;
}

impl SubBlockIndex for sai::A {
    const INDEX: usize = 0;
}

impl SubBlockIndex for sai::B {
    const INDEX: usize = 1;
}

pub struct Pcm1808<'d, D: dma::Channel> {
    dma: Peri<'d, D>,
    buffer: &'d mut [u16],
    request: u8,
    data_ptr: *mut u16,
}

impl<'d, D: dma::Channel> Pcm1808<'d, D> {
    pub fn new<
        S: sai::SubBlockInstance + SubBlockIndex,
        SCK: sai::SckPin<SAI1, S>,
        SD: sai::SdPin<SAI1, S>,
        FS: sai::FsPin<SAI1, S>,
        MCLK: sai::MclkPin<SAI1, S>,
    >(
        mut sai_peri: Peri<'d, peripherals::SAI1>,
        mut sck: Peri<'d, SCK>,
        mut sd: Peri<'d, SD>,
        mut fs: Peri<'d, FS>,
        mut mclk: Peri<'d, MCLK>,
        dma: Peri<'d, D>,
        dma_buf: &'d mut [u16],
    ) -> Self
    where
        D: sai::Dma<peripherals::SAI1, S>,
    {
        embassy_stm32::rcc::enable_and_reset::<peripherals::SAI1>();

        let (sd_af_type, ck_af_type) = get_af_types(Mode::Master, TxRx::Receiver);

        let mut sd = Flex::new(sd);
        sd.set_as_af_unchecked(sd.af_num(), sd_af_type);
        let mut sck = Flex::new(sck);
        sck.set_as_af_unchecked(sck.af_num(), ck_af_type);
        let mut fs = Flex::new(fs);
        fs.set_as_af_unchecked(fs.af_num(), ck_af_type);
        let mut mclk = Flex::new(mclk);
        mclk.set_as_af_unchecked(mclk.af_num(), ck_af_type);

        let regs = unsafe { &*embassy_stm32::pac::SAI1::ptr() };
        let ch = regs.ch(S::INDEX);
        let data_ptr = ch.dr().as_ptr() as *mut u16;

        ch.cr1().modify(|w| w.set_saien(false));
        ch.cr2().modify(|w| w.set_fflush(true));

        ch.cr1().modify(|w| {
            w.set_mode(vals::Mode::MASTER_RX);
            w.set_prtcfg(vals::Prtcfg::FREE);
            w.set_ds(vals::Ds::BIT16);
            w.set_lsbfirst(vals::Lsbfirst::MSB_FIRST);
            w.set_ckstr(vals::Ckstr::RISING_EDGE);
            w.set_syncen(vals::Syncen::ASYNCHRONOUS);
            w.set_mono(vals::Mono::STEREO);
            w.set_outdriv(vals::Outdriv::IMMEDIATELY);
            w.set_mckdiv(12);
            w.set_nodiv(false);
            w.set_dmaen(true);
        });

        ch.cr2().modify(|w| {
            w.set_fth(vals::Fth::QUARTER3);
            w.set_comp(vals::Comp::NO_COMPANDING);
            w.set_cpl(vals::Cpl::TWOS_COMPLEMENT);
            w.set_muteval(vals::Muteval::SEND_ZERO);
            w.set_mutecnt(4);
            w.set_tris(false);
        });

        ch.frcr().modify(|w| {
            w.set_fsoff(vals::Fsoff::BEFORE_FIRST);
            w.set_fspol(vals::Fspol::FALLING_EDGE);
            w.set_fsdef(false);
            w.set_fsall(15);
            w.set_frl(31);
        });

        ch.slotr().modify(|w| {
            w.set_nbslot(1);
            w.set_slotsz(vals::Slotsz::BIT16);
            w.set_fboff(0);
            w.set_sloten(vals::Sloten::from_bits(0b11));
        });

        ch.cr1().modify(|w| w.set_saien(true));

        if ch.cr1().read().saien() == false {
            panic!(
                "SAI failed to enable. Check that config is valid (frame length, slot count, etc)"
            );
        }

        Self {
            dma,
            buffer: dma_buf,
            request: dma.request(),
            data_ptr,
        }
    }

    pub async fn read(&mut self, buf: &mut [u16]) -> Result<(), sai::Error> {
        if buf.len() > self.buffer.len() {
            return Err(sai::Error::Overrun);
        }

        let transfer = unsafe {
            Transfer::new_read(
                self.dma.reborrow(),
                self.request,
                self.data_ptr,
                self.buffer,
                TransferOptions::default(),
            )
        };

        transfer.await;
        buf.copy_from_slice(&self.buffer[..buf.len()]);
        Ok(())
    }
}

fn get_af_types(mode: Mode, tx_rx: TxRx) -> (AfType, AfType) {
    (
        match tx_rx {
            TxRx::Transmitter => {
                AfType::output(OutputType::PushPull, embassy_stm32::gpio::Speed::VeryHigh)
            }
            TxRx::Receiver => AfType::input(Pull::Down),
        },
        match mode {
            Mode::Master => {
                AfType::output(OutputType::PushPull, embassy_stm32::gpio::Speed::VeryHigh)
            }
            Mode::Slave => AfType::input(Pull::Down),
        },
    )
}
