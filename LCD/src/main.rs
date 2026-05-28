#![no_std]
#![no_main]

use core::cell::RefCell;
use defmt_rtt as _;
use display_interface_spi::SPIInterface;
use embassy_embedded_hal::shared_bus::blocking::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_stm32::adc::adc4::Adc4;
use embassy_stm32::gpio::OutputType;
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_stm32::rcc::*;
use embassy_stm32::spi::{Config as SpiConfig, Spi};
use embassy_stm32::time::mhz;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Delay, Duration, Instant, Timer};
use ili9341::{DisplaySize240x320, Ili9341, Mode};
use panic_probe as _;

mod adc_fft;
mod spectrum_ui;

const FREQ_MIN_HZ: u32 = 20;
const FREQ_MAX_HZ: u32 = 20_000;

#[derive(Clone, Copy, PartialEq, defmt::Format)]
enum DisplayMode {
    ChooseFreq,
    SpectrumAnalyse,
}

struct MyOrientation;
impl Mode for MyOrientation {
    fn mode(&self) -> u8 {
        0x68
    }
    fn is_landscape(&self) -> bool {
        true
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.pll1 = Some(Pll {
        source: PllSource::HSI,
        prediv: PllPreDiv::DIV1,
        mul: PllMul::MUL10,
        divp: None,
        divq: None,
        divr: Some(PllDiv::DIV1),
    });
    config.rcc.sys = Sysclk::PLL1_R;
    let p = embassy_stm32::init(config);

    let enc_clk = Input::new(p.PA0, Pull::Up);
    let enc_dt = Input::new(p.PA1, Pull::Up);
    let enc_sw = Input::new(p.PC1, Pull::Up);

    let pwm_pin = PwmPin::new(p.PB3, OutputType::PushPull);
    let mut current_freq_hz: u32 = 1_000;
    let mut pwm = SimplePwm::new(
        p.TIM2,
        None,
        Some(pwm_pin),
        None,
        None,
        embassy_stm32::time::hz(current_freq_hz),
        Default::default(),
    );
    pwm.ch2().set_duty_cycle_percent(50);
    pwm.ch2().enable();

    let adc = Adc4::new(p.ADC4);
    let mut fft = adc_fft::AdcFft::new(adc, p.PB0);
    let _measured_sr = fft.measure_sample_rate();

    let mut spi_config = SpiConfig::default();
    spi_config.frequency = mhz(24);
    let spi = Spi::new_blocking(p.SPI1, p.PA5, p.PA7, p.PA6, spi_config);

    let cs = Output::new(p.PC9, Level::High, Speed::High);
    let dc = Output::new(p.PB6, Level::Low, Speed::High);
    let rst = Output::new(p.PC7, Level::High, Speed::VeryHigh);

    let display_bus = Mutex::<CriticalSectionRawMutex, _>::new(RefCell::new(spi));
    let display_spi = SpiDevice::<CriticalSectionRawMutex, _, _>::new(&display_bus, cs);
    let iface = SPIInterface::new(display_spi, dc);

    let mut delay = Delay;
    let mut display =
        Ili9341::new(iface, rst, &mut delay, MyOrientation, DisplaySize240x320).unwrap();

    spectrum_ui::draw_spectrum_base(&mut display);
    spectrum_ui::draw_page_label(&mut display, "choose freq");

    let mut last_clk = enc_clk.is_high();
    let mut last_sw = enc_sw.is_high();
    let mut last_turn = Instant::now();
    let mut frame = spectrum_ui::SpectrumFrame::new();
    let mut display_mode = DisplayMode::ChooseFreq;
    let mut button_pressed = false;
    let mut spectrum = [0.0f32; 320];

    loop {
        let clk_now = enc_clk.is_high();
        if clk_now != last_clk {
            let dt_now = enc_dt.is_high();
            let elapsed = last_turn.elapsed().as_millis();
            last_turn = Instant::now();

            // Логарифмический шаг — пик двигается равномерно по экрану
            // Быстрее крутишь — больше шаг
            let factor: f32 = if elapsed < 50 {
                1.5
            } else if elapsed < 150 {
                1.2
            } else {
                1.08
            };

            if clk_now != dt_now {
                let new_freq = current_freq_hz as f32 * factor;
                current_freq_hz = (new_freq as u32).min(FREQ_MAX_HZ);
            } else {
                let new_freq = current_freq_hz as f32 / factor;
                current_freq_hz = (new_freq as u32).max(FREQ_MIN_HZ);
            }

            pwm.set_frequency(embassy_stm32::time::hz(current_freq_hz));
            pwm.ch2().set_duty_cycle_percent(50);
            defmt::info!("freq = {} Hz", current_freq_hz);
        }
        last_clk = clk_now;

        let sw_now = enc_sw.is_high();

        // Запоминаем факт нажатия кнопки
        if !sw_now && last_sw {
            button_pressed = true;
        }

        // Переключение страницы при отпускании кнопки
        if sw_now && !last_sw && button_pressed {
            display_mode = match display_mode {
                DisplayMode::ChooseFreq => DisplayMode::SpectrumAnalyse,
                DisplayMode::SpectrumAnalyse => DisplayMode::ChooseFreq,
            };
            spectrum_ui::draw_spectrum_base(&mut display);
            if display_mode == DisplayMode::ChooseFreq {
                spectrum_ui::draw_page_label(&mut display, "choose freq");
            } else {
                spectrum_ui::draw_page_label(&mut display, "fft analyse");
            }
            frame = spectrum_ui::SpectrumFrame::new();
            button_pressed = false;
            defmt::info!("Mode switched to {:?}", display_mode);
        }
        last_sw = sw_now;

        match display_mode {
            DisplayMode::ChooseFreq => {
                spectrum_ui::generate_single_tone_spectrum(current_freq_hz, &mut spectrum);
                spectrum_ui::update_spectrum_curve_colored(
                    &mut display,
                    &mut frame,
                    &spectrum,
                    spectrum_ui::SELECT_COLOR,
                    spectrum_ui::SELECT_FILL_COLOR,
                );
                spectrum_ui::draw_page_label(&mut display, "choose freq");
            }
            DisplayMode::SpectrumAnalyse => {
                fft.capture_and_compute(&mut spectrum);
                spectrum_ui::update_spectrum_curve(&mut display, &mut frame, &spectrum);
                spectrum_ui::draw_page_label(&mut display, "fft analyse");
            }
        }

        spectrum_ui::redraw_plot_overlays(&mut display);
        Timer::after(Duration::from_millis(1)).await;
    }
}
