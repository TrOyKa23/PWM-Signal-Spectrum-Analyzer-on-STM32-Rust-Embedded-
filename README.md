# PWM Signal Spectrum Analyzer on STM32

Real-time audio spectrum visualization using DFT on embedded hardware

:::info

**Author**: TrOyKa23 \
**GitHub Project Link**: https://github.com/UPB-PMRust-Students/fils-project-2026-TrOyKa23
:::

## Description

This project implements a real-time audio spectrum analyzer using an STM32U545RE-Q microcontroller.

The device generates a PWM square wave signal on pin PB3 using hardware timer TIM2, with a frequency adjustable from 20 Hz to 20 kHz via a rotary encoder. The same signal is fed back into analog input PB0, digitized using the internal ADC4 peripheral, processed using a Discrete Fourier Transform (DFT), and visualized on a 2.4" ILI9341 TFT display over SPI.

The system operates in two modes:
- **Choose Freq** — displays a synthetic single-peak spectrum representing the selected PWM frequency
- **FFT Analyse** — performs real-time DFT of the captured ADC signal and renders the actual frequency spectrum

---

## Motivation

The motivation behind this project is to better understand real-time digital signal processing on embedded systems and to build a practical tool for audio frequency analysis.

It combines multiple important concepts:

- Embedded Rust development with the Embassy async framework
- Digital Signal Processing (DSP) — DFT, Hann windowing, dBFS scaling
- Real-time data acquisition using the internal ADC
- Logarithmic frequency axis rendering on constrained hardware
- Hardware-software integration with SPI display, PWM generation, and rotary encoder input

---

## Architecture

The system is composed of the following main components:

- **Signal Generation Module** — generates a PWM square wave via TIM2 on PB3
- **ADC Acquisition Module** — captures 512 samples from PB0 using ADC4
- **DSP Processing Module** — removes DC offset, applies Hann window, computes DFT, maps to dBFS
- **Display Module** — renders frequency spectrum on ILI9341 240×320 TFT over SPI
- **User Input Module** — rotary encoder on PA0/PA1/PC1 for frequency selection and mode switching

### Data Flow

```
TIM2 PWM (PB3) → [wire] → ADC4 (PB0) → DC removal → Hann window → DFT → dBFS mapping → ILI9341 display
```

### Block Diagram

```
+------------------+     +------------------+     +------------------+
|   Rotary         |     |   STM32U545RE-Q  |     |   ILI9341 TFT    |
|   Encoder        +---->+                  +---->+   2.4" Display   |
|  PA0 / PA1 / PC1 |     |  - Embassy async |     |   240x320, SPI   |
+------------------+     |  - TIM2 PWM      |     +------------------+
                         |  - ADC4 capture  |
+------------------+     |  - DFT + window  |
|   PWM output     |     |  - Log freq axis |
|   TIM2 / PB3     +<----+                  |
+--------+---------+     +--------+---------+
         |                        ^
         | (loopback wire)        |
         v                        |
+------------------+              |
|   ADC input      +--------------+
|   ADC4 / PB0     |
+------------------+
```

---

![Audio Spectrum Analyzer](./schematics.webp)

## Log

### Week 4–5 — Project Definition & Research

Defined the project idea: a real-time spectrum analyzer running entirely on the STM32U545RE-Q using its internal ADC and PWM timer — no external ADC required.

Researched DFT/FFT algorithms suitable for embedded no-std Rust. Evaluated CMSIS-DSP but decided against it due to FFI complexity; implemented a selective DFT directly in Rust computing only the bins corresponding to 20 Hz–20 kHz.

Studied logarithmic frequency axis mapping and dBFS normalization. Reviewed FabFilter Pro-Q 3 spectrum analyzer as a visual reference for layout and scale.

### Week 6–7 — Display & Signal Generation

Connected the ILI9341 display over SPI. Configured SPI1 at 24 MHz with correct CPOL/CPHA. Initial display showed color artifacts — traced to incorrect MADCTL rotation byte; corrected to `0x68` for landscape mode.

Implemented `spectrum_ui.rs`: full 320×240 plot area with logarithmic X axis (20 Hz–20 kHz), dB grid (−60 to +12 dBFS), frequency labels, and differential per-column redraw using `SpectrumFrame` to avoid full-screen flicker.

Brought up TIM2 PWM on PB3. Verified 50% duty cycle square wave on oscilloscope. Implemented rotary encoder reading with quadrature decoding and logarithmic frequency stepping (multiply/divide by factor instead of add/subtract).

### Week 8 — ADC Capture & DFT Pipeline

Wired PB3 to PB0 with a short jumper. Verified ADC4 reads meaningful values from the PWM signal.

Implemented `adc_fft.rs`: 512-sample capture, DC offset removal, Hann window, selective DFT for bins k_min–k_max, magnitude computation, logarithmic pixel mapping, and linear interpolation between bins to avoid "staircase" gaps.

Initial spectrum showed peaks at wrong frequencies — diagnosed as incorrect assumed sample rate (DFT frequency depends entirely on actual ADC throughput). Calibrated `SAMPLE_RATE` constant by measuring real ADC capture time and verifying that a known 1 kHz PWM input produces a peak at the correct pixel position.

Added `cortex_m::asm::delay()` between ADC reads to enforce a stable sample rate.

![Audio Spectrum Analyzer](./workflow.webp)

### Week 9 — Integration, Two-Mode UI & Documentation

Integrated both modes into the main event loop. Button press on encoder SW toggles between `ChooseFreq` and `SpectrumAnalyse` with full screen redraw.

`ChooseFreq` mode uses `generate_single_tone_spectrum()` — a synthetic Gaussian peak at the selected frequency — to give immediate visual feedback before the FFT confirms it.

`SpectrumAnalyse` mode runs the real DFT pipeline on every frame.

Added `draw_page_label()` and `redraw_plot_overlays()` to restore frequency/dB labels after the differential curve update overwrites them.

Designed and modeled a custom 3D-printed stand for the device. Took physical measurements of the STM32 board and display module, then recreated the geometry in Blender to ensure correct alignment, mounting hole placement, and cable clearance. The stand was designed to hold the display at an ergonomic viewing angle while keeping the overall construction compact and stable.

Cleaned up firmware, removed dead code, finalized documentation.

![Audio Spectrum Analyzer](./3d_modeling.webp)
![Audio Spectrum Analyzer](./3d_printing.webp)
---

## Hardware

| Device | Role | Pin(s) |
|---|---|---|
| STM32 Nucleo-U545RE-Q | Main microcontroller | — |
| ILI9341 2.4" TFT (240×320) | Spectrum display | SPI1: PA5/PA7/PA6, CS: PC9, DC: PB6, RST: PC7 |
| Rotary encoder | Frequency selection & mode switch | CLK: PA0, DT: PA1, SW: PC1 |
| Jumper wire (PB3 → PB0) | PWM loopback into ADC | PB3 → PB0 |

### Schematics

```
                     [NUCLEO-U545RE-Q]
                            |
          +-----------------+-----------------+
          |                 |                 |
        [SPI1]           [TIM2]           [ADC4]
          |                 |                 |
          v                 v                 ^
   ILI9341 Display    PWM output          ADC input
   PA5 SCK            PB3 (50% duty)      PB0
   PA7 MOSI                |               ^
   PA6 MISO                +---------------+
   PC9 CS               (loopback wire)
   PB6 DC
   PC7 RST

                         [GPIO]
                            |
                    Rotary Encoder
                    PA0 CLK
                    PA1 DT
                    PC1 SW (button)
```

### Bill of Materials

| Device | Usage | Price |
|---|---|---|
| STM32 Nucleo-U545RE-Q | Main microcontroller | [106.00 RON](https://ro.mouser.com/ProductDetail/STMicroelectronics/NUCLEO-U545RE-Q) |
| ILI9341 TFT Display 2.4" | Frequency spectrum visualization | ~50 RON |
| Rotary encoder (KY-040) | Frequency selection, mode switching | ~10 RON |
| Jumper wires | PB3→PB0 loopback, connections | ~5 RON |

---

## Software

| Library | Description | Usage |
|---|---|---|
| embassy-stm32 | STM32 async HAL | ADC4, TIM2 PWM, SPI, GPIO |
| embassy-executor | Async executor | Main task scheduling |
| embedded-graphics | 2D graphics primitives | Lines, rectangles, text on display |
| ili9341 | ILI9341 display driver | Display initialization and SPI interface |
| display-interface-spi | SPI display interface | Bridges embedded-hal SPI to display driver |
| libm | Math library (no_std) | `log10f`, `cosf`, `sinf`, `sqrtf`, `expf` |
| defmt / defmt-rtt | Logging over RTT | Debug output via probe |
| panic-probe | Panic handler | Halt + print on panic |

---

## Links

1. [Fast Fourier Transform — Wikipedia](https://en.wikipedia.org/wiki/Fast_Fourier_transform)
2. [Discrete Fourier Transform — Wikipedia](https://en.wikipedia.org/wiki/Discrete_Fourier_transform)
3. [Hann window — Wikipedia](https://en.wikipedia.org/wiki/Hann_function)
4. [Embassy — async embedded Rust](https://embassy.dev/)
5. [embedded-graphics crate](https://github.com/embedded-graphics/embedded-graphics)
6. [ILI9341 datasheet](https://cdn-shop.adafruit.com/datasheets/ILI9341.pdf)
7. [STM32U545 Reference Manual](https://www.st.com/resource/en/reference_manual/rm0456-stm32u5-series-armbased-32bit-mcus-stmicroelectronics.pdf)

