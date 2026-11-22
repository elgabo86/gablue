//! Diagnostic : pourquoi les tranches n'ont pas la même durée ? Compare la
//! grille théorique (période constante), le recollage séquentiel actuel
//! (`cut_on`) et un ajustement global (ancre + période) sur les attaques.
//!
//! `cargo run --release -p gtracker-core --example cutjitter -- <fichier> <subsong> <durée .m3u> <beats>`

use std::path::PathBuf;

use gtracker_core as core;

fn nearest(env: &core::tempo::Envelope, t: f64, win: f64) -> Option<f64> {
    let n = env.flux.len();
    if n < 3 || env.hop <= 0.0 || t <= 0.0 {
        return None;
    }
    let i0 = ((t - win) / env.hop).max(0.0) as usize;
    let i1 = (((t + win) / env.hop) as usize + 1).min(n);
    if i1 <= i0 + 2 {
        return None;
    }
    let (mut vmax, mut jmax, mut sum) = (0.0f64, i0, 0.0f64);
    for j in i0..i1 {
        let v = env.flux[j];
        sum += v;
        if v > vmax {
            vmax = v;
            jmax = j;
        }
    }
    let mean = sum / (i1 - i0) as f64;
    if vmax <= 1e-9 || vmax < mean * 1.8 {
        return None;
    }
    Some(jmax as f64 * env.hop)
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
    let tempo = core::decode::analyse_tempo(&d.pcm, d.channels, d.rate, d.total, d.window);
    let whole = d.window.map(|(_, b)| b).unwrap_or(d.total);
    let mut t = tempo;
    if let Some((a, b)) = d.window {
        t.phase = a;
        if b - a > 4.0 {
            t = t.across(b - a);
        }
    }
    let len = t.beat * beats;
    println!(
        "total {:.3} · whole {:.3} · {:.2} BPM · tranche théorique {:.4} s · phase {:.4}",
        d.total, whole, t.bpm, len, t.phase
    );

    // tranches actuelles (recollage séquentiel)
    let clips = core::tempo::cut_on(whole, &t, beats, d.envelope.as_ref());
    let lens: Vec<f64> = clips.iter().map(|c| c.len()).collect();
    let (mini, maxi) = lens.iter().fold((f64::MAX, 0.0f64), |(m, x), l| (m.min(*l), x.max(*l)));
    println!(
        "cut_on : {} tranches · {:.3} à {:.3} s (écart {:.0} ms)",
        clips.len(),
        mini,
        maxi,
        (maxi - mini) * 1000.0
    );
    for (i, c) in clips.iter().enumerate().take(5) {
        println!("  {i}: {:.3} → {:.3} ({:.3})", c.start, c.end, c.len());
    }
    if let Some(c) = clips.get(14) {
        let i = 14;
        println!("  {i}: {:.3} → {:.3} ({:.3})", c.start, c.end, c.len());
    }

    // l'attaque **réelle** dans le PCM (RMS 1 ms) contre celle de l'enveloppe :
    // dit si l'écart médian est un décalage du morceau ou un biais de mesure
    {
        let ch = d.channels.max(1) as usize;
        let fps = d.rate as f64 * ch as f64;
        let step = (d.rate as usize / 1000).max(1); // ~1 ms
        let rms_at = |t: f64| -> f64 {
            let f = (t * d.rate as f64).max(0.0) as usize;
            let mut e = 0.0f64;
            let mut n = 0usize;
            for k in f..(f + step).min(d.pcm.len() / ch) {
                for c in 0..ch {
                    let v = d.pcm[k * ch + c] as f64;
                    e += v * v;
                }
                n += 1;
            }
            (e / (n * ch).max(1) as f64).sqrt()
        };
        let env = d.envelope.as_ref().unwrap();
        println!("frontière · attaque PCM (RMS 1 ms) · attaque enveloppe");
        for k in 0..6 {
            let pred = t.phase + k as f64 * len;
            if pred > whole {
                break;
            }
            let mut best = (0.0f64, 0.0f64);
            let mut dt = -0.15;
            while dt <= 0.15 {
                let tt = pred + dt;
                let v = rms_at(tt);
                if v > best.0 {
                    best = (v, tt);
                }
                dt += step as f64 / d.rate as f64;
            }
            let flux = nearest(env, pred, t.beat * 0.25).unwrap_or(f64::NAN);
            println!(
                "  {k}: pcm {:+.0} ms · flux {:+.0} ms",
                (best.1 - pred) * 1000.0,
                (flux - pred) * 1000.0
            );
        }
        let _ = fps;
    }

    // attaques les plus proches de chaque frontière théorique
    let env = match d.envelope.as_ref() {
        Some(e) => e,
        None => {
            println!("pas d'enveloppe");
            return Ok(());
        }
    };
    let win = t.beat * 0.25;
    let mut pts: Vec<(f64, f64)> = Vec::new(); // (k, attaque)
    let mut k = 0usize;
    loop {
        let pred = t.phase + k as f64 * len;
        if pred > whole {
            break;
        }
        if let Some(at) = nearest(env, pred, win) {
            let dev = at - pred;
            if dev.abs() <= len * 0.1 {
                pts.push((k as f64, at));
            }
        }
        k += 1;
    }
    println!("attaques trouvées : {} / {}", pts.len(), k);
    let devs: Vec<f64> = pts.iter().map(|(k, at)| at - (t.phase + k * len)).collect();
    let mut abs = devs.clone();
    abs.sort_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap());
    let med = {
        let mut s = devs.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s[s.len() / 2]
    };
    println!(
        "écart à la grille : médian {:+.0} ms · |écart| médian {:.0} ms · max {:.0} ms",
        med * 1000.0,
        abs[abs.len() / 2] * 1000.0,
        abs.last().copied().unwrap_or(0.0) * 1000.0
    );
    // combien d'attaques tombent près de la grille une fois l'ancre corrigée ?
    let centred: Vec<f64> = devs.iter().map(|d| d - med).collect();
    let mut ac: Vec<f64> = centred.iter().map(|d| d.abs()).collect();
    ac.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let proches = ac.iter().filter(|d| **d <= 0.015).count();
    println!(
        "une fois l'ancre corrigée de {:+.0} ms : |reste| médian {:.0} ms · max {:.0} ms · {} / {} sous 15 ms",
        med * 1000.0,
        ac[ac.len() / 2] * 1000.0,
        ac.last().copied().unwrap_or(0.0) * 1000.0,
        proches,
        ac.len()
    );

    // moindres carrés : attaque = a + k * L
    if pts.len() >= 4 {
        let n = pts.len() as f64;
        let sk: f64 = pts.iter().map(|(k, _)| k).sum();
        let sa: f64 = pts.iter().map(|(_, a)| a).sum();
        let skk: f64 = pts.iter().map(|(k, _)| k * k).sum();
        let ska: f64 = pts.iter().map(|(k, a)| k * a).sum();
        let l = (n * ska - sk * sa) / (n * skk - sk * sk);
        let anc = (sa - l * sk) / n;
        let resid: Vec<f64> = pts.iter().map(|(k, a)| (a - (anc + k * l)).abs()).collect();
        let mut rs = resid.clone();
        rs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "ajustement : ancre {:.4} s (vs {:.4}) · période {:.4} s (vs {:.4}, {:+.2} %) · résidu médian {:.0} ms · max {:.0} ms",
            anc,
            t.phase,
            l,
            len,
            (l / len - 1.0) * 100.0,
            rs[rs.len() / 2] * 1000.0,
            rs.last().copied().unwrap_or(0.0) * 1000.0
        );
    }
    Ok(())
}
