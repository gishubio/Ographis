//! How colours look to those who see red and green alike, which is about one man in twelve:
//! colours that are easy to tell apart for most, and hard for them, are told about, so that a
//! map or a chart can be made that all can read.

/// The two common kinds: protanopia (weak red) and deuteranopia (weak green), as matrices on
/// linear RGB (Machado, Oliveira and Fernandes, 2009, at full severity).
const KINDS: &[(&str, [[f64; 3]; 3])] = &[
    ("red-blind (protanopia)", [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]]),
    ("green-blind (deuteranopia)", [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]]),
];

fn linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// CIE L*a*b* of linear RGB, under D65.
fn lab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|v| v.clamp(0.0, 1.0));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn seen(c: [u8; 3], m: Option<&[[f64; 3]; 3]>) -> [f64; 3] {
    let l = c.map(linear);
    let l = match m {
        Some(m) => [0, 1, 2].map(|i| m[i][0] * l[0] + m[i][1] * l[1] + m[i][2] * l[2]),
        None => l,
    };
    lab(l)
}

/// The first two colours that most tell apart and those who see red and green alike hardly
/// can: their places in the list, and which kind of seeing it is. Colours that are alike for
/// everyone are not told about.
pub fn confused(colours: &[[u8; 3]]) -> Option<(usize, usize, &'static str)> {
    for (name, m) in KINDS {
        for i in 0..colours.len() {
            for j in i + 1..colours.len() {
                let normal = distance(seen(colours[i], None), seen(colours[j], None));
                let weak = distance(seen(colours[i], Some(m)), seen(colours[j], Some(m)));
                if normal > 20.0 && weak < 8.0 {
                    return Some((i, j, name));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_and_green_are_alike_and_blue_and_orange_are_not() {
        let (red, green) = ([0xd7, 0x30, 0x27], [0x4d, 0x9a, 0x21]);
        assert!(confused(&[red, green]).is_some());
        assert!(confused(&[[0x2a, 0x78, 0xd6], [0xeb, 0x68, 0x34]]).is_none());
        // The ramps for numbers are safe, each from light to dark.
        for (name, _, stops) in crate::thematic::RAMPS {
            assert!(confused(stops).is_none(), "{} {:?}", name, confused(stops));
        }
    }
}
