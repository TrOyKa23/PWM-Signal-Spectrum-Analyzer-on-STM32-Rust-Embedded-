use cortex_m::asm;
use embassy_stm32::Peri;
use embassy_stm32::adc::adc4::{Adc4, SampleTime};
use embassy_stm32::peripherals::{ADC4, PB0};
use embassy_time::Instant;

pub const FFT_SIZE: usize = 1024;
// Целевая частота дискретизации с помощью задержки между чтениями.
pub const SAMPLE_RATE: f32 = 40_000.0;

pub struct AdcFft {
    adc: Adc4<'static, ADC4>,
    pin: Peri<'static, PB0>,
}

impl AdcFft {
    pub fn new(mut adc: Adc4<'static, ADC4>, pin: Peri<'static, PB0>) -> Self {
        // CYCLES47_5 даёт более стабильный результат чем CYCLES1_5
        adc.set_sample_time(SampleTime::CYCLES7_5);
        Self { adc, pin }
    }

    pub fn capture_and_compute(&mut self, out: &mut [f32; 320]) {
        let mut buf = [0u16; FFT_SIZE];

        // Захват сэмплов с фиксированной задержкой между ними.
        for sample in buf.iter_mut() {
            *sample = self.adc.blocking_read(&mut self.pin);
            asm::delay(3628); // ≈22.5 мкс при 160 MHz, чтобы получить ~44.1 kHz
        }

        // Убираем DC offset
        let mean = buf.iter().map(|&x| x as f32).sum::<f32>() / FFT_SIZE as f32;

        // --- Вычисляем только нужные бины, не все FFT_SIZE/2 ---
        // Бины от k=1 до k=FFT_SIZE/2-1, но только те, чьи частоты в [20, 20000] Гц
        let k_min = ((20.0 * FFT_SIZE as f32 / SAMPLE_RATE) as usize).max(1);
        let k_max = ((20_000.0 * FFT_SIZE as f32 / SAMPLE_RATE) as usize).min(FFT_SIZE / 2 - 1);

        // Предвычисляем оконную функцию Ханна
        let mut windowed = [0.0f32; FFT_SIZE];
        for n in 0..FFT_SIZE {
            let w = 0.5
                - 0.5
                    * libm::cosf(2.0 * core::f32::consts::PI * n as f32 / (FFT_SIZE as f32 - 1.0));
            windowed[n] = (buf[n] as f32 - mean) * w;
        }

        // Считаем магнитуды только нужных бинов.
        // Стек-аллокация — убедитесь что FFT_SIZE/2 влезает
        let mut magnitudes = [0.0f32; FFT_SIZE / 2];

        for k in k_min..=k_max {
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            let step = core::f32::consts::TAU * k as f32 / FFT_SIZE as f32;
            for n in 0..FFT_SIZE {
                let angle = step * n as f32;
                re += windowed[n] * libm::cosf(angle);
                im -= windowed[n] * libm::sinf(angle);
            }
            magnitudes[k] = libm::sqrtf(re * re + im * im) / (FFT_SIZE as f32 * 0.5);
            // делим на N*0.5 для коррекции окна Ханна
        }

        // Абсолютный reference: полная шкала ADC = 4095 → 0 dBFS
        // magnitudes в единицах "отсчётов" относительно full scale
        let adc_full_scale = 4095.0_f32;
        let ref_mag = adc_full_scale / 2.0; // амплитуда синуса full scale после окна

        // Маппинг бинов → пиксели с правильным логарифмическим масштабом
        let log_min = libm::log10f(20.0_f32);
        let log_max = libm::log10f(20_000.0_f32);

        let mut pixel_max = [-80.0f32; 320];

        for k in k_min..=k_max {
            let freq = k as f32 * SAMPLE_RATE / FFT_SIZE as f32;
            let t = (libm::log10f(freq) - log_min) / (log_max - log_min);
            let px = (t * 319.0) as usize;
            if px >= 320 {
                continue;
            }

            let mag = magnitudes[k].max(1e-9);
            // dBFS относительно full scale синуса
            let db = (20.0 * libm::log10f(mag / ref_mag)).clamp(-80.0, 6.0);
            if db > pixel_max[px] {
                pixel_max[px] = db;
            }
        }

        // Интерполяция без "hold": линейная интерполяция между известными бинами
        // Сначала отмечаем какие пиксели заняты
        let mut pixel_has = [false; 320];
        for k in k_min..=k_max {
            let freq = k as f32 * SAMPLE_RATE / FFT_SIZE as f32;
            let t = (libm::log10f(freq) - log_min) / (log_max - log_min);
            let px = (t * 319.0) as usize;
            if px < 320 {
                pixel_has[px] = true;
            }
        }

        // Линейная интерполяция между соседними известными пикселями
        out.fill(-80.0);
        let mut left_px: Option<usize> = None;

        for px in 0..320 {
            if pixel_has[px] {
                // Заполняем промежуток между left_px и px
                if let Some(lp) = left_px {
                    let span = (px - lp) as f32;
                    for i in lp..px {
                        let t = (i - lp) as f32 / span;
                        out[i] = pixel_max[lp] * (1.0 - t) + pixel_max[px] * t;
                    }
                }
                out[px] = pixel_max[px];
                left_px = Some(px);
            }
        }
        // Заполняем хвост
        if let Some(lp) = left_px {
            for px in lp..320 {
                out[px] = pixel_max[lp];
            }
        }

        // Лёгкое сглаживание (3-точечное) чтобы убрать артефакты
        let copy = *out;
        for i in 1..319 {
            out[i] = (copy[i - 1] + copy[i] * 2.0 + copy[i + 1]) / 4.0;
        }
    }

    pub fn measure_sample_rate(&mut self) -> f32 {
        let mut buf = [0u16; FFT_SIZE];
        let t0 = Instant::now();
        for sample in buf.iter_mut() {
            *sample = self.adc.blocking_read(&mut self.pin);
            asm::delay(3628);
        }
        let elapsed_us = t0.elapsed().as_micros();
        let real_sr = FFT_SIZE as f32 * 1_000_000.0 / elapsed_us as f32;
        defmt::info!(
            "Real sample rate: {} Hz, elapsed: {} us",
            real_sr,
            elapsed_us
        );
        real_sr
    }
}
