//! Vérifie le projet `.gtrack` sans interface : aller-retour, maîtresse
//! switchable, réparation des références mortes, nom de fichier.
//!
//! `cargo run --release -p gtracker-core --example projcheck`

use std::path::PathBuf;

use gtracker_core::key::{Key, Mode};
use gtracker_core::project::{project_path, Clip, Fill, Project, Role, Source, StretchMode};

fn main() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("gtrack-projcheck-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("projcheck.gtrack");

    let mut p = Project::new("projcheck");
    p.tempo_bpm = 128.0;
    let a = p.add_source(Source {
        id: 1,
        title: "a".into(),
        site: "fichiers".into(),
        entry_id: -11,
        file: PathBuf::from("/tmp/a.wav"),
        subsong: 1,
        loop_start: None,
        loop_end: None,
        bpm: 128.0,
        phase: 0.0,
        key: Some(Key::new(9, Mode::Minor, 0.6)),
    });
    let b = p.add_source(Source {
        id: 2,
        title: "b".into(),
        site: "fichiers".into(),
        entry_id: -12,
        file: PathBuf::from("/tmp/b.wav"),
        subsong: 1,
        loop_start: None,
        loop_end: None,
        bpm: 96.0,
        phase: 0.0,
        key: Some(Key::new(0, Mode::Major, 0.6)),
    });
    let t1 = p.add_track("piste 1", Some(a), 0);
    let t2 = p.add_track("piste 2", Some(b), 1);
    p.add_clip(Clip {
        id: 0,
        track: t1,
        source: a,
        src_start: 0,
        src_end: 44_100,
        at_beat: 0.0,
        length_beats: 4.0,
        gain: 0.8,
        fade_in: 0.0,
        fade_out: 0.25,
        transpose: None,
        key_lock: Some(true),
        key: None,
        stretch: None,
        fill: Fill::Loop,
    });

    anyhow::ensure!(
        p.master == Some(t1),
        "la première piste doit être maîtresse"
    );
    anyhow::ensure!(p.set_master(t2), "switch de maîtresse");
    anyhow::ensure!(
        (p.tempo_bpm - 96.0).abs() < 1e-9,
        "le switch doit poser le BPM"
    );
    anyhow::ensure!(
        p.track(t2).unwrap().role == Role::Master,
        "rôle non mis à jour"
    );
    anyhow::ensure!(
        p.track(t1).unwrap().role == Role::Follow,
        "ancienne maîtresse toujours maître"
    );
    p.set_master(t1);
    anyhow::ensure!(
        (p.tempo_bpm - 128.0).abs() < 1e-9,
        "retour au BPM de la piste 1"
    );

    // « même gamme » : la mineur ↔ do majeur = mêmes notes, stretch imposé
    let c = p.clips[0].clone();
    anyhow::ensure!(
        p.effective_transpose(&c) == 0,
        "la mineur ↔ do majeur = 0 demi-ton"
    );
    anyhow::ensure!(
        p.effective_stretch(&c) == StretchMode::Stretch,
        "même gamme impose le stretch"
    );

    p.save(&path)?;
    let q = Project::load(&path)?;
    anyhow::ensure!(q.tracks.len() == 2 && q.clips.len() == 1, "contenu perdu");
    anyhow::ensure!((q.tempo_bpm - p.tempo_bpm).abs() < 1e-9, "tempo perdu");
    anyhow::ensure!(q.clips[0].src_end == 44_100, "plage de clip perdue");
    anyhow::ensure!(q.clips[0].fill == Fill::Loop, "mode de remplissage perdu");
    anyhow::ensure!(q.sources[0].key.is_some(), "tonalité perdue");
    anyhow::ensure!(
        !path.with_extension("gtrack.part").exists(),
        "fichier .part laissé"
    );

    // réparation : un clip qui pointe une piste inexistante disparaît
    let mut broken = q.clone();
    broken.clips[0].track = 9_999;
    let notes = broken.validate();
    anyhow::ensure!(
        broken.clips.is_empty() && !notes.is_empty(),
        "réparation muette"
    );

    // nom : jamais d'extension en double, toujours dans le dossier projets
    let n = project_path("Mon projet");
    anyhow::ensure!(
        n.to_string_lossy().ends_with("Mon projet.gtrack"),
        "nom de projet"
    );
    anyhow::ensure!(n.parent().unwrap().ends_with("projets"), "dossier projets");

    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    std::fs::remove_dir_all(&dir).ok();
    println!(
        "PROJET OK · aller-retour {bytes} octets · maîtresse switchable · même gamme = stretch · {} clips valides",
        q.clips.len()
    );
    Ok(())
}
