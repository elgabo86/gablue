//! Diagnostic : y a-t-il du silence au **début** de chaque tranche ? Sert à
//! distinguer un blanc ajouté par le décodage/découpe d'un blanc présent dans
//! le rendu (vécu 21/11 : « mini blanc au lieu du début de musique » sur FF8
//! « Force Your Way » quand une tranche cède la place à la première).
//!
//! `cargo run --release -p gtracker-core --example silencecheck -- <fichier> <subsong> <durée .m3u> <beats>`

use std::path::PathBuf;

use gtracker_core as core;

/// Durée, en millisecondes, du silence (< 0,02) au tout début d'une tranche.
fn tete_silencieuse(pcm: &[f32], ch: usize, from_frame: usize, rate: u32) -> (f64, f32) {
    let mut peak = 0.0f32;
    let mut silent = 0usize;
    for f in 0..(rate as usize / 10) {
        let i = (from_frame + f) * ch;
        if i + ch > pcm.len() {
            break;
        }
        let mut local = 0.0f32;
        for c in 0..ch {
            local = local.max(pcm[i + c].abs());
        }
        peak = peak.max(local);
        if local < 0.02 {
            silent += 1;
        } else {
            break;
        }
    }
    (silent as f64 * 1000.0 / rate as f64, peak)
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let file = PathBuf::from(&a[1]);
    let subsong: usize = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(1);
    let length: Option<f64> = a.get(3).and_then(|s| s.parse().ok());
    let beats: f64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(8.0);
    let track = core::Track {
        subsong: subsong as u32,
        title: "test".into(),
        file,
        length,
        loop_start: None,
        loop_end: None,
    };
    let d = core::decode::decode(&core::zxtune_path(), &track)?;
    let ch = d.channels.max(1) as usize;
    let fps = d.rate as f64 * ch as f64;
    let frame_of = |s: f64| (s * fps).round() as usize;

    // tête du fichier entier
    let (tete, pic) = tete_silencieuse(&d.pcm, ch, 0, d.rate);
    println!(
        "fichier : total {:.3} s · {} canaux · {} Hz · silence initial {:.2} ms (pic {:.3})",
        d.total, d.channels, d.rate, tete, pic
    );

    // profil de la tête du fichier : 2 ms par colonne (un blanc numérique est
    // exactement à 0, un vrai départ doux monte progressivement)
    {
        let step = (d.rate as usize / 500).max(1);
        let mut prof = String::new();
        let mut f = 0usize;
        while f < d.rate as usize / 20 && (f + 1) * ch <= d.pcm.len() {
            let mut peak = 0.0f32;
            for k in f..(f + step).min(d.pcm.len() / ch) {
                for c in 0..ch {
                    peak = peak.max(d.pcm[k * ch + c].abs());
                }
            }
            prof.push(if peak == 0.0 {
                '.'
            } else if peak < 0.001 {
                ','
            } else if peak < 0.02 {
                ':'
            } else if peak < 0.1 {
                '+'
            } else {
                '#'
            });
            f += step;
        }
        println!("tête (2 ms/col, 50 ms) : {prof}");
    }

    // ce que la lecture enjambe : zéros exacts, en échantillons entrelacés
    let head = core::decode::head_silence(&d.pcm, ch, d.rate);
    println!(
        "blanc enjambé à la lecture : {} échantillons ({} trames, {:.2} ms, parité {})",
        head,
        head / ch,
        head as f64 * 1000.0 / fps,
        if (head / ch) % 2 == 0 { "paire" } else { "impaire" }
    );

    // même grille que l'UI
    let tempo = core::decode::analyse_tempo(&d.pcm, d.channels, d.rate, d.total, d.window);
    let whole = d.window.map(|(_, b)| b).unwrap_or(d.total);
    let mut t = tempo;
    if let Some((a, b)) = d.window {
        t.phase = a;
        if b - a > 4.0 {
            t = t.across(b - a);
        }
    }
    let clips = core::tempo::cut_on(whole, &t, beats, d.envelope.as_ref());
    println!("{} tranches · pas {:.3} s", clips.len(), t.beat * beats);
    for (i, c) in clips.iter().enumerate().take(4) {
        let (sil, pak) = tete_silencieuse(&d.pcm, ch, frame_of(c.start), d.rate);
        println!(
            "  {i}: {:.3} → {:.3} · silence de tête {:.2} ms (pic {:.3})",
            c.start, c.end, sil, pak
        );
    }
    // et la couture « dernière → première » : que trouve-t-on juste avant la fin
    // de la dernière tranche, et au début de la première ?
    if let (Some(first), Some(last)) = (clips.first(), clips.last()) {
        let (sil, pak) = tete_silencieuse(&d.pcm, ch, frame_of(first.start), d.rate);
        println!(
            "couture fin → début : 1re tranche {:.3} s, silence de tête {:.2} ms (pic {:.3}) ; fin de la dernière à {:.3} s",
            first.start, sil, pak, last.end
        );
    }
    Ok(())
}
