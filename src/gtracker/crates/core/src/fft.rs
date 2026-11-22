//! FFT radix-2 minimale, pour l'analyse de tonalité.
//!
//! Pas de dépendance : l'analyse harmonique a besoin d'une FFT, pas d'un
//! moteur de transformée généraliste. 60 lignes suffisent et restent
//! déterministes (aucun dispatch SIMD, aucun ordre d'accumulation variable).

/// Transformée en place. `n` doit être une puissance de deux.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len().min(im.len());
    debug_assert!(n.is_power_of_two(), "fft : taille non puissance de deux");
    if n < 2 {
        return;
    }
    // permutation bit-reversée
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    // papillons par étage
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut start = 0usize;
        while start < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = cr * re[b] - ci * im[b];
                let ti = cr * im[b] + ci * re[b];
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            start += len;
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pic_a_la_bonne_bin() {
        // sinusoïde à exactement 4 périodes sur 64 échantillons → bin 4
        let n = 64;
        let mut re: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 4.0 * i as f32 / n as f32).sin())
            .collect();
        let mut im = vec![0.0f32; n];
        fft(&mut re, &mut im);
        let mag: Vec<f32> = re.iter().zip(&im).map(|(a, b)| (a * a + b * b).sqrt()).collect();
        let peak = (0..n / 2)
            .max_by(|a, b| mag[*a].partial_cmp(&mag[*b]).unwrap())
            .unwrap();
        assert_eq!(peak, 4);
        // symétrie hermitienne : mag(4) == mag(n-4)
        assert!((mag[4] - mag[n - 4]).abs() < 1e-3);
    }

    #[test]
    fn dc_pur() {
        let n = 8;
        let mut re = vec![1.0f32; n];
        let mut im = vec![0.0f32; n];
        fft(&mut re, &mut im);
        assert!((re[0] - 8.0).abs() < 1e-4);
        for k in 1..n {
            assert!(re[k].abs() < 1e-4 && im[k].abs() < 1e-4);
        }
    }
}
