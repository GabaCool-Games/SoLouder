use super::{fft, ifft};

fn signal(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| ((i * 37 % 101) as f32 / 50.0) - 1.0)
        .collect()
}

#[test]
fn impulse_transforms_to_ones() {
    for bits in 2..=11 {
        let mut buffer = vec![0.0; 1 << bits];
        buffer[0] = 1.0;
        fft(&mut buffer);
        for pair in buffer.chunks(2) {
            assert_eq!(pair, [1.0, 0.0], "length {}", 1 << bits);
        }
    }
}

#[test]
fn single_tone_lands_in_one_bin() {
    let count = 64;
    let mut buffer = vec![0.0; 2 * count];
    for k in 0..count {
        let phase = 2.0 * std::f64::consts::PI * 5.0 * k as f64 / count as f64;
        buffer[2 * k] = phase.cos() as f32;
        buffer[2 * k + 1] = phase.sin() as f32;
    }
    fft(&mut buffer);
    let peak = (0..count)
        .max_by(|&a, &b| {
            let ma = buffer[2 * a].hypot(buffer[2 * a + 1]);
            let mb = buffer[2 * b].hypot(buffer[2 * b + 1]);
            ma.total_cmp(&mb)
        })
        .unwrap();
    assert!(peak == 5 || peak == count - 5, "peak at {peak}");
    assert!((buffer[2 * peak].hypot(buffer[2 * peak + 1]) - count as f32).abs() < 1e-3);
}

#[test]
fn inverse_undoes_forward() {
    for bits in 2..=12 {
        let original = signal(1 << bits);
        let mut buffer = original.clone();
        fft(&mut buffer);
        ifft(&mut buffer);
        for (i, (a, b)) in buffer.iter().zip(&original).enumerate() {
            assert!(
                (a - b).abs() < 1e-4,
                "length {} index {i}: {a} vs {b}",
                1 << bits
            );
        }
    }
}

#[test]
#[should_panic(expected = "power of two")]
fn rejects_other_lengths() {
    fft(&mut [0.0; 12]);
}
