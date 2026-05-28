// spectrum_ui_st7789v.rs
//
// Spectrum analyser graphics for ST7789V 240×320 LCD in LANDSCAPE mode.
// Rotation register: MADCTL = 0x48
// Logical screen after rotation: 320 × 240 px
//
// Layout: plot fills the ENTIRE screen (320 × 240).
// Frequency labels are drawn INSIDE the plot at the very bottom (overlay).
// dB ruler is drawn INSIDE the plot at the very right (overlay).
// No reserved strips — full 320 × 240 plot area.
//
// Public API:
//   draw_spectrum_base(&mut display)
//   draw_spectrum_curve(&mut display, &[f32])   — 320 f32 values in dB
//   clear_curve_area(&mut display)
//   flat_response() -> [f32; 320]

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};

// ── Screen dimensions ─────────────────────────────────────────────────────────

const LCD_W: i32 = 320;
const LCD_H: i32 = 240;
const LABEL_AREA: i32 = 16;

// ── Plot area = full screen minus label strip at bottom ─────────────────────────

const PLOT_X0: i32 = 0;
const PLOT_X1: i32 = 319; // full width
const PLOT_Y0: i32 = 0;
const PLOT_Y1: i32 = LCD_H - LABEL_AREA - 1; // reserve bottom area for labels

const PLOT_W: u32 = (PLOT_X1 - PLOT_X0 + 1) as u32; // 320
const PLOT_H: i32 = PLOT_Y1 - PLOT_Y0 + 1; // 224

// 0 dB sits at ~35 % from top  (leaves more room below for cut region)
const DB0_Y: i32 = PLOT_Y0 + (PLOT_H * 35 / 100); // ≈ 84

const DB_MAX: f32 = 12.0;
const DB_MIN: f32 = -60.0;

// ── Palette ───────────────────────────────────────────────────────────────────

pub const BG: Rgb565 = Rgb565::new(2, 4, 2); // near-black
const GRID: Rgb565 = Rgb565::new(5, 12, 5); // very dim teal/grey
const ZERO_LINE: Rgb565 = Rgb565::new(10, 24, 10); // slightly brighter
const LABEL: Rgb565 = Rgb565::new(0, 31, 31); // cyan (matches photo)
const CURVE: Rgb565 = Rgb565::new(31, 44, 0); // gold
const FILL: Rgb565 = Rgb565::new(10, 16, 0); // dim gold fill
pub const SELECT_COLOR: Rgb565 = Rgb565::new(31, 31, 31); // white-like peak color
pub const SELECT_FILL_COLOR: Rgb565 = BG;

// ── dB / frequency mapping ────────────────────────────────────────────────────

#[inline]
fn db_to_py(db: f32) -> i32 {
    let px_up = DB0_Y as f32 / DB_MAX;
    let px_down = (PLOT_H - DB0_Y) as f32 / DB_MIN.abs();
    let y = if db >= 0.0 {
        DB0_Y - (db * px_up) as i32
    } else {
        DB0_Y + (db.abs() * px_down) as i32
    };
    y.clamp(PLOT_Y0, PLOT_Y1)
}

#[inline]
fn freq_to_px(hz: f32) -> i32 {
    use libm::log10f;
    let log_min = log10f(20.0);
    let log_max = log10f(20_000.0);
    let t = (log10f(hz.max(20.0)) - log_min) / (log_max - log_min);
    (PLOT_X0 as f32 + t * PLOT_W as f32) as i32
}

// ── Primitives ────────────────────────────────────────────────────────────────

fn fill_rect<D: DrawTarget<Color = Rgb565>>(d: &mut D, x: i32, y: i32, w: u32, h: u32, c: Rgb565) {
    Rectangle::new(Point::new(x, y), Size::new(w, h))
        .into_styled(PrimitiveStyle::with_fill(c))
        .draw(d)
        .ok();
}

fn hline<D: DrawTarget<Color = Rgb565>>(d: &mut D, x0: i32, x1: i32, y: i32, c: Rgb565, w: u32) {
    Line::new(Point::new(x0, y), Point::new(x1, y))
        .into_styled(PrimitiveStyle::with_stroke(c, w))
        .draw(d)
        .ok();
}

fn vline<D: DrawTarget<Color = Rgb565>>(d: &mut D, x: i32, y0: i32, y1: i32, c: Rgb565, w: u32) {
    Line::new(Point::new(x, y0), Point::new(x, y1))
        .into_styled(PrimitiveStyle::with_stroke(c, w))
        .draw(d)
        .ok();
}

/// Buffer for differential spectrum updates.
/// Хранит предыдущую высоту кривой для каждого пиксельного столбца.
pub struct SpectrumFrame {
    pub prev_y: [i32; PLOT_W as usize],
}

impl SpectrumFrame {
    pub const fn new() -> Self {
        Self {
            prev_y: [PLOT_Y1; PLOT_W as usize],
        }
    }
}

// ── Public: full background ───────────────────────────────────────────────────

pub fn draw_spectrum_base<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    fill_rect(display, 0, 0, LCD_W as u32, LCD_H as u32, BG);
    draw_db_grid(display);
    draw_freq_grid(display);
    draw_zero_line(display);
    // Overlaid labels drawn last so they appear on top of grid lines
    draw_freq_labels(display);
    draw_db_ruler(display);
}

// ── Grid ──────────────────────────────────────────────────────────────────────

fn draw_db_grid<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    // Only clean even-dB lines; skip 0 dB (drawn separately as ZERO_LINE)
    const DB_LINES: &[f32] = &[
        12.0, 6.0, -6.0, -12.0, -18.0, -24.0, -30.0, -36.0, -42.0, -48.0, -54.0, -60.0,
    ];
    for &db in DB_LINES {
        let y = db_to_py(db);
        hline(display, PLOT_X0, PLOT_X1, y, GRID, 1);
    }
}

fn draw_freq_grid<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    const FREQ_LINES: &[f32] = &[
        20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0,
        700.0, 800.0, 900.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        8_000.0, 9_000.0, 10_000.0, 20_000.0,
    ];
    for &hz in FREQ_LINES {
        let x = freq_to_px(hz);
        vline(display, x, PLOT_Y0, PLOT_Y1, GRID, 1);
    }
}

fn draw_zero_line<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    let y = db_to_py(0.0);
    hline(display, PLOT_X0, PLOT_X1, y, ZERO_LINE, 1);
}

// ── Overlaid dB ruler — right edge, inside plot ───────────────────────────────

fn draw_db_ruler<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    const RULER_LABELS: &[(f32, &str)] = &[
        (12.0, "+12"),
        (6.0, "+6"),
        (3.0, "+3"),
        (0.0, "0"),
        (-6.0, "-6"),
        (-12.0, "-12"),
        (-20.0, "-20"),
        (-40.0, "-40"),
        (-60.0, "-60"),
    ];

    let style = MonoTextStyle::new(&FONT_6X10, LABEL);

    for &(db, label) in RULER_LABELS {
        let y = db_to_py(db);
        // right-align text, 2 px from right edge
        Text::with_alignment(label, Point::new(LCD_W - 2, y - 2), style, Alignment::Right)
            .draw(display)
            .ok();
    }
}

// ── Overlaid frequency labels — bottom edge, inside plot ─────────────────────

fn draw_freq_labels<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    const LABELS: &[(f32, &str)] = &[
        (20.0, "20"),
        (50.0, "50"),
        (100.0, "100"),
        (200.0, "200"),
        (500.0, "500"),
        (1_000.0, "1k"),
        (2_000.0, "2k"),
        (5_000.0, "5k"),
        (10_000.0, "10k"),
        (20_000.0, "20k"),
    ];

    let style = MonoTextStyle::new(&FONT_6X10, LABEL);

    for &(hz, label) in LABELS {
        let x = freq_to_px(hz);
        // Draw text 12 px above the bottom edge
        Text::with_alignment(label, Point::new(x, LCD_H - 3), style, Alignment::Center)
            .draw(display)
            .ok();
    }
}

// ── Public: clear & redraw grid ───────────────────────────────────────────────
#[allow(dead_code)]
pub fn clear_curve_area<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    fill_rect(display, PLOT_X0, PLOT_Y0, PLOT_W, PLOT_H as u32, BG);
    draw_db_grid(display);
    draw_freq_grid(display);
    draw_zero_line(display);
    draw_freq_labels(display);
    draw_db_ruler(display);
}

/// Draw the frequency-response curve completely. Используется один раз при инициализации.
#[allow(dead_code)]
pub fn draw_spectrum_curve<D: DrawTarget<Color = Rgb565>>(display: &mut D, db_values: &[f32]) {
    draw_spectrum_curve_colored(display, db_values, CURVE, FILL);
}

#[allow(dead_code)]
pub fn draw_spectrum_curve_colored<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    db_values: &[f32],
    curve_color: Rgb565,
    fill_color: Rgb565,
) {
    if db_values.is_empty() {
        return;
    }

    let len = db_values.len().min(PLOT_W as usize);
    let zero_y = db_to_py(0.0);
    let line_style = PrimitiveStyle::with_stroke(curve_color, 1);

    let mut prev_x = PLOT_X0;
    let mut prev_y = db_to_py(db_values[0]);

    for i in 1..len {
        let x = PLOT_X0 + i as i32;
        let y = db_to_py(db_values[i]);

        Line::new(Point::new(prev_x, prev_y), Point::new(x, y))
            .into_styled(line_style)
            .draw(display)
            .ok();

        let (fy0, fy1) = if y <= zero_y {
            (y, zero_y)
        } else {
            (zero_y, y)
        };
        vline(display, x, fy0, fy1, fill_color, 1);

        prev_x = x;
        prev_y = y;
    }
}

/// Обновление только изменившихся столбцов спектра.
/// Здесь реализована «двойная буферизация» состояния кривой.
pub fn update_spectrum_curve<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    frame: &mut SpectrumFrame,
    db_values: &[f32],
) {
    update_spectrum_curve_colored(display, frame, db_values, CURVE, FILL);
}

pub fn update_spectrum_curve_colored<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    frame: &mut SpectrumFrame,
    db_values: &[f32],
    curve_color: Rgb565,
    fill_color: Rgb565,
) {
    let len = db_values.len().min(PLOT_W as usize);
    let zero_y = db_to_py(0.0);

    // Предвычислим Y позиции db линий
    const DB_LINES: &[f32] = &[
        12.0, 6.0, -6.0, -12.0, -18.0, -24.0, -30.0, -36.0, -42.0, -48.0, -54.0, -60.0,
    ];

    for i in 0..len {
        let x = PLOT_X0 + i as i32;
        let new_y = db_to_py(db_values[i]);
        let old_y = frame.prev_y[i];

        if new_y == old_y {
            continue;
        }

        // Перерисовываем столбец фоном
        fill_rect(display, x, PLOT_Y0, 1, PLOT_H as u32, BG);

        // Восстанавливаем вертикальную частотную линию
        if is_freq_grid_x(x) {
            vline(display, x, PLOT_Y0, PLOT_Y1, GRID, 1);
        }

        // Восстанавливаем горизонтальные db линии в этом столбце
        for &db in DB_LINES {
            let y = db_to_py(db);
            fill_rect(display, x, y, 1, 1, GRID);
        }

        // Восстанавливаем нулевую линию
        fill_rect(display, x, zero_y, 1, 1, ZERO_LINE);

        // Рисуем заливку и кривую
        let (fill_y0, fill_y1) = if new_y <= zero_y {
            (new_y, zero_y)
        } else {
            (zero_y, new_y)
        };

        if fill_y1 > fill_y0 {
            vline(display, x, fill_y0, fill_y1, fill_color, 1);
        }

        fill_rect(display, x, new_y, 1, 1, curve_color);
        frame.prev_y[i] = new_y;
    }
}

pub fn draw_page_label<D: DrawTarget<Color = Rgb565>>(display: &mut D, label: &str) {
    let style = MonoTextStyle::new(&FONT_6X10, LABEL);
    fill_rect(display, 0, 0, 120, 12, BG);
    Text::with_alignment(label, Point::new(4, 2), style, Alignment::Left)
        .draw(display)
        .ok();
}

pub fn redraw_plot_overlays<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    draw_freq_labels(display);
    draw_db_ruler(display);
}

fn is_freq_grid_x(x: i32) -> bool {
    const FREQ_LINES: &[f32] = &[
        20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0,
        700.0, 800.0, 900.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 6_000.0, 7_000.0,
        8_000.0, 9_000.0, 10_000.0, 20_000.0,
    ];
    FREQ_LINES.iter().any(|&hz| freq_to_px(hz) == x)
}

// ── Test helpers ──────────────────────────────────────────────────────────────

/// 320-element flat 0 dB response.
#[allow(dead_code)]
pub fn flat_response() -> [f32; 320] {
    [0.0f32; 320]
}

/// Синтетический логарифмический спектр для демонстрации.
/// Возвращает несколько узких пиков, расположенных на логарифмической оси.
#[allow(dead_code)]
pub fn generate_test_spectrum(frame_index: u32, out: &mut [f32; 320]) {
    use libm::sinf;

    const PEAKS: &[(f32, f32, f32)] = &[
        // (frequency in Hz, base dB, width px)
        (50.0, 6.0, 10.0),
        (200.0, -3.0, 12.0),
        (800.0, 0.0, 16.0),
        (2_500.0, -8.0, 14.0),
        (8_000.0, -12.0, 18.0),
    ];

    out.fill(DB_MIN);
    let phase = (frame_index as f32) * 0.11;

    for &(hz, base_db, width) in PEAKS {
        let cx = freq_to_px(hz);
        let peak_db = base_db + sinf(phase + hz * 0.0003).abs() * 6.0;
        let sigma = width * 0.75;

        for x in 0..320 {
            let dx = (x as f32 - cx as f32).abs();
            let energy = libm::expf(-dx * dx / (2.0 * sigma * sigma));
            let db = peak_db * energy + DB_MIN * (1.0 - energy);
            if db > out[x] {
                out[x] = db;
            }
        }
    }
}
/// Рисует спектр с одним пиком на заданной частоте.
/// Ось X логарифмическая: 20 Гц → 20 кГц на 320 пикселей.
pub fn generate_single_tone_spectrum(freq_hz: u32, out: &mut [f32; 320]) {
    use libm::{expf, log10f};

    let log_min = log10f(20.0);
    let log_max = log10f(20_000.0);
    let log_f = log10f(freq_hz as f32);

    let peak_px = ((log_f - log_min) / (log_max - log_min) * 319.0) as i32;

    out.fill(DB_MIN); // заполняем -60 dB вместо 0.0

    for x in 0..320usize {
        let dist = (x as i32 - peak_px) as f32;
        // Гауссов пик: на вершине +6 dB, ширина ~8 px
        let db = 6.0 * expf(-dist * dist / (2.0 * 8.0 * 8.0));
        out[x] = db;
    }
}

/// Отображает индикатор режима в верхнем левом углу экрана
#[allow(dead_code)]
pub fn draw_mode_indicator<D: DrawTarget<Color = Rgb565>>(display: &mut D, is_raw_fft: bool) {
    let style = MonoTextStyle::new(&FONT_6X10, LABEL);
    let mode_text = if is_raw_fft { "RAW FFT" } else { "PROCESSED" };

    // Чёрный фон для текста
    fill_rect(display, 0, 0, 80, 12, BG);

    // Текст режима
    Text::with_alignment(mode_text, Point::new(4, 2), style, Alignment::Left)
        .draw(display)
        .ok();
}
