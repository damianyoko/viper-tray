// 32x32 RGBA tray icon: a compact circular gauge around a Viper "V".
// The outer ring carries the battery level, while the center stays readable at
// small tray sizes.

const W: u32 = 32;
const H: u32 = 32;

#[derive(Clone, Copy)]
struct Rgba(u8, u8, u8, u8);

const TRACK: Rgba = Rgba(50, 55, 58, 210);
const CENTER_V: Rgba = Rgba(225, 235, 225, 255);
const CENTER_SHADOW: Rgba = Rgba(0, 0, 0, 120);
const OFFLINE: Rgba = Rgba(135, 135, 135, 255);
const OFFLINE_SLASH: Rgba = Rgba(220, 70, 70, 255);

const CX: f32 = 15.5;
const CY: f32 = 15.5;
const OUTER_R: f32 = 14.2;
const INNER_R: f32 = 10.2;

fn fill_color(pct: u8) -> Rgba {
    if pct < 20 {
        Rgba(255, 60, 60, 255)
    } else if pct < 60 {
        Rgba(255, 165, 0, 255)
    } else {
        Rgba(70, 220, 90, 255)
    }
}

fn put(buf: &mut [u8], x: i32, y: i32, c: Rgba) {
    if x < 0 || y < 0 || (x as u32) >= W || (y as u32) >= H {
        return;
    }
    let i = ((y as u32 * W + x as u32) * 4) as usize;
    buf[i] = c.0;
    buf[i + 1] = c.1;
    buf[i + 2] = c.2;
    buf[i + 3] = c.3;
}

fn fill_row(buf: &mut [u8], y: i32, x_start: i32, x_end: i32, c: Rgba) {
    if x_end < x_start {
        return;
    }
    for x in x_start..=x_end {
        put(buf, x, y, c);
    }
}

fn draw_disc(buf: &mut [u8], cx: i32, cy: i32, radius: i32, c: Rgba) {
    let r2 = radius * radius;
    for y in (cy - radius)..=(cy + radius) {
        for x in (cx - radius)..=(cx + radius) {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= r2 {
                put(buf, x, y, c);
            }
        }
    }
}

fn draw_line(buf: &mut [u8], from: (i32, i32), to: (i32, i32), thickness: i32, c: Rgba) {
    let (mut x0, mut y0) = from;
    let (x1, y1) = to;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_disc(buf, x0, y0, thickness, c);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn draw_ring(buf: &mut [u8], pct: Option<u8>) {
    let fill_limit = pct.map(|p| (p.min(100) as f32 / 100.0) * std::f32::consts::TAU);
    let fill = pct.map(fill_color).unwrap_or(OFFLINE);

    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let dx = x as f32 + 0.5 - CX;
            let dy = y as f32 + 0.5 - CY;
            let dist = (dx * dx + dy * dy).sqrt();
            if !(INNER_R..=OUTER_R).contains(&dist) {
                continue;
            }

            put(buf, x, y, TRACK);
            if let Some(limit) = fill_limit {
                let mut angle = dy.atan2(dx) + std::f32::consts::FRAC_PI_2;
                if angle < 0.0 {
                    angle += std::f32::consts::TAU;
                }
                if angle <= limit {
                    put(buf, x, y, fill);
                }
            }
        }
    }
}

fn draw_center_v(buf: &mut [u8], c: Rgba) {
    draw_line(buf, (9, 11), (15, 22), 1, CENTER_SHADOW);
    draw_line(buf, (22, 11), (15, 22), 1, CENTER_SHADOW);
    draw_line(buf, (10, 10), (16, 21), 1, c);
    draw_line(buf, (22, 10), (16, 21), 1, c);
    fill_row(buf, 22, 15, 17, c);
}

fn draw_offline_slash(buf: &mut [u8]) {
    draw_line(buf, (9, 23), (23, 9), 1, OFFLINE_SLASH);
}

pub fn build_icon_rgba(pct: Option<u8>) -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];

    draw_ring(&mut buf, pct);
    match pct {
        Some(_) => draw_center_v(&mut buf, CENTER_V),
        None => {
            draw_center_v(&mut buf, OFFLINE);
            draw_offline_slash(&mut buf);
        }
    }

    buf
}

pub fn icon_dim() -> (u32, u32) {
    (W, H)
}
