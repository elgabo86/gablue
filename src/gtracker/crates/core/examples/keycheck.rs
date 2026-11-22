//! Vérifie la tonalité sans interface : chroma d'un sinus, profils idéaux,
//! transposition de gamme, et analyse d'un accord synthétique.
//!
//! `cargo run --release -p gtracker-core --example keycheck`

use gtracker_core::key::{analyse, best_key, chroma, Key, Mode};

fn tone(rate: u32, secs: f32, hz: f32, amp: f32) -> Vec<f32> {
    (0..(rate as f32 * secs) as usize)
        .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
        .collect()
}

fn main() -> anyhow::Result<()> {
    let rate = 22_050u32;

    // 1. un la (440 Hz) tombe sur la classe 9 (do = 0)
    let a = tone(rate, 2.0, 440.0, 0.5);
    let ch = chroma(&a, 1, rate, 0, a.len()).expect("chroma du la");
    let peak = (0..12).max_by(|x, y| ch[*x].partial_cmp(&ch[*y]).unwrap()).unwrap();
    anyhow::ensure!(peak == 9, "440 Hz devrait être la classe 9, pas {peak}");

    // 2. profils idéaux : la corrélation doit retrouver la bonne tonalité
    let mut do_majeur = [0.0f32; 12];
    let mut la_mineur = [0.0f32; 12];
    for i in 0..12 {
        // profil majeur : tonique, tierce, quinte dominantes
        do_majeur[i] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88][i];
        la_mineur[(i + 9) % 12] =
            [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17][i];
    }
    let kd = best_key(&do_majeur);
    anyhow::ensure!(kd.tonic == 0 && kd.mode == Mode::Major, "do majeur mal détecté : {kd:?}");
    let ka = best_key(&la_mineur);
    anyhow::ensure!(ka.tonic == 9 && ka.mode == Mode::Minor, "la mineur mal détecté : {ka:?}");

    // 3. accord réel : la mineur (la + do + mi) avec basse de la
    let mut chord = vec![0.0f32; (rate as f32 * 3.0) as usize];
    for (hz, amp) in [(220.0, 0.30), (261.63, 0.25), (329.63, 0.25), (440.0, 0.15)] {
        let t = tone(rate, 3.0, hz, amp);
        for (x, y) in chord.iter_mut().zip(t) {
            *x += y;
        }
    }
    let detected = analyse(&chord, 1, rate, 0, chord.len()).expect("analyse de l'accord");
    anyhow::ensure!(
        (detected.tonic == 9 && detected.mode == Mode::Minor)
            || (detected.tonic == 0 && detected.mode == Mode::Major),
        "accord de la mineur mal lu : {detected:?}"
    );

    // 4. transpositions : même gamme, plus court chemin
    let la = Key::new(9, Mode::Minor, 1.0);
    let do_k = Key::new(0, Mode::Major, 1.0);
    anyhow::ensure!(la.transpose_for_scale(&do_k) == 0, "la mineur / do majeur = 0");
    anyhow::ensure!(
        la.transpose_for_scale(&Key::new(2, Mode::Major, 1.0)) == 2,
        "la mineur vers ré majeur = +2"
    );
    anyhow::ensure!(do_k.transpose_for_scale(&Key::new(11, Mode::Major, 1.0)) == -1, "si majeur = -1");
    anyhow::ensure!(
        (Key::induced_semitones(1.5) - 7.019_550).abs() < 1e-4,
        "repitch 1,5× = quinte"
    );

    // 5. silence : pas de tonalité inventée
    anyhow::ensure!(analyse(&vec![0.0f32; rate as usize], 1, rate, 0, rate as usize).is_none(), "silence");

    println!(
        "TONALITE OK · la={} · do={} · accord={} (confiance {:.2}) · transpositions justes",
        ka.name(),
        kd.name(),
        detected.name(),
        detected.confidence
    );
    Ok(())
}
