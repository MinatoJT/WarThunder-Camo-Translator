use crate::scene::{TriangleUv, Uv};

fn edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn top_left(a: [f64; 2], b: [f64; 2]) -> bool {
    b[1] < a[1] || (b[1] == a[1] && b[0] > a[0])
}

pub fn triangle(uv: TriangleUv, size: u32, mut visit: impl FnMut(usize, [f32; 3])) {
    let pixels = uv.map(|[u, v]| {
        [
            f64::from(u * size as f32),
            f64::from((1.0 - v) * size as f32),
        ]
    });
    let area = edge(pixels[0], pixels[1], pixels[2]);
    if area.abs() <= 1e-8 {
        return;
    }
    let sign = area.signum();
    let edges = [[1, 2], [2, 0], [0, 1]];
    let inclusive = edges.map(|[a, b]| {
        if sign > 0.0 {
            top_left(pixels[a], pixels[b])
        } else {
            top_left(pixels[b], pixels[a])
        }
    });
    let bounds = |axis| {
        let min = pixels.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
        let max = pixels
            .iter()
            .map(|p| p[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        ((min - 0.5).ceil() as i64, (max - 0.5).floor() as i64)
    };
    let (x0, x1) = bounds(0);
    let (y0, y1) = bounds(1);
    let side = i64::from(size);

    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = [x as f64 + 0.5, y as f64 + 0.5];
            let weights = edges.map(|[a, b]| edge(pixels[a], pixels[b], p) * sign);
            if weights
                .iter()
                .zip(inclusive)
                .all(|(&v, include)| v > 1e-9 || (v.abs() <= 1e-9 && include))
            {
                let i = (y.rem_euclid(side) * side + x.rem_euclid(side)) as usize;
                visit(i, weights.map(|v| (v / area.abs()) as f32));
            }
        }
    }
}

pub fn interpolate<const N: usize>(values: [[f32; N]; 3], weights: [f32; 3]) -> [f32; N] {
    std::array::from_fn(|i| {
        weights[0] * values[0][i] + weights[1] * values[1][i] + weights[2] * values[2][i]
    })
}

pub fn texel([u, v]: Uv, size: u32) -> usize {
    let axis = |value: f32| ((value.rem_euclid(1.0) * size as f32) as u32).min(size - 1);
    (axis(1.0 - v) * size + axis(u)) as usize
}

/// Extend a texture past its rasterized islands without overwriting any covered texels.
pub fn pad_edges(size: u32, coverage: &mut [u8], mut copy_pixel: impl FnMut(usize, usize)) {
    let stride = size as usize;
    let count = coverage.len();
    let neighbors = |index: usize| {
        let x = index % stride;
        [
            if x == 0 {
                index + stride - 1
            } else {
                index - 1
            },
            if x + 1 == stride {
                index + 1 - stride
            } else {
                index + 1
            },
            if index < stride {
                index + count - stride
            } else {
                index - stride
            },
            if index + stride >= count {
                index + stride - count
            } else {
                index + stride
            },
        ]
    };
    let mut frontier: Vec<_> = coverage
        .iter()
        .enumerate()
        .filter(|&(index, &value)| {
            value != 0 && neighbors(index).iter().any(|&near| coverage[near] == 0)
        })
        .map(|(index, _)| index)
        .collect();
    let mut next = Vec::new();
    for _ in 0..(size / 512).clamp(2, 16) {
        if frontier.is_empty() {
            break;
        }
        next.clear();
        for source in frontier.drain(..) {
            for target in neighbors(source) {
                if coverage[target] == 0 {
                    coverage[target] = coverage[source];
                    copy_pixel(target, source);
                    next.push(target);
                }
            }
        }
        std::mem::swap(&mut frontier, &mut next);
    }
}
