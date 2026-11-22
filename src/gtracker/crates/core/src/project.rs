//! Projet GTracker (`.gtrack`) : sources, pistes, clips, tempo.
//!
//! **Non destructif** : rien n'est copié en WAV, une source est référencée par
//! son chemin et un clip par une plage de trames. Un clip déposé est **gelé** :
//! retoucher la grille d'une source ne déplace plus la composition.
//!
//! Un projet est un document utilisateur, il vit dans
//! `~/Musique/GTracker/projets/` (XDG MUSIC) et jamais dans `~/.cache`.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::key::Key;

pub type Id = u64;
pub const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StretchMode {
    /// La vitesse change, la hauteur suit (tracker / MPC). Défaut.
    #[default]
    Repitch,
    /// Durée et hauteur indépendantes (WSOLA). Requis pour accorder.
    Stretch,
}

/// Que faire quand un clip est plus court que sa place sur la grille.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fill {
    /// Le reste est du silence (contenu intact).
    #[default]
    Trim,
    /// Le contenu se répète jusqu'à la fin de la place.
    Loop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Master,
    Follow,
}

/// Source audio du projet : de quoi re-décoder après réouverture.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: Id,
    pub title: String,
    /// Site joshw.info (`"fichiers"` pour un fichier local).
    pub site: String,
    pub entry_id: i64,
    pub file: PathBuf,
    pub subsong: u32,
    /// BPM estimé/forcé de la source (indication, jamais la vérité du projet).
    pub bpm: f64,
    pub phase: f64,
    pub key: Option<Key>,
    /// Boucle musicale de la source (secondes), mesurée puis éventuellement
    /// corrigée : c'est elle qui décide **où commence et finit** la découpe,
    /// dans l'éditeur comme dans la pile de sources de l'arrangement.
    /// `serde(default)` : les projets écrits avant ce champ restent lisibles.
    #[serde(default)]
    pub loop_start: Option<f64>,
    #[serde(default)]
    pub loop_end: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub name: String,
    /// Index dans `Project.sources` : la source d'où viennent les tranches.
    pub source: Option<usize>,
    pub role: Role,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub stretch: StretchMode,
    /// « même gamme » : transposition automatique vers la maîtresse (stretch).
    pub key_lock: bool,
    pub transpose: i32,
    /// Index de la palette de couleurs (GUI).
    pub color: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    pub track: Id,
    /// Index dans `Project.sources`.
    pub source: usize,
    /// Plage de **contenu** (trames de la source).
    pub src_start: usize,
    pub src_end: usize,
    /// Placement musical.
    pub at_beat: f64,
    pub length_beats: f64,
    pub gain: f32,
    /// Fades en temps (beats).
    pub fade_in: f64,
    pub fade_out: f64,
    /// Surcharges : `None` = suit la piste.
    pub transpose: Option<i32>,
    pub key_lock: Option<bool>,
    /// Tonalité **de la tranche** (analysée à la création du clip), prioritaire
    /// sur celle de la source : une intro n'est pas dans la tonalité du refrain.
    #[serde(default)]
    pub key: Option<crate::key::Key>,
    pub stretch: Option<StretchMode>,
    pub fill: Fill,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub name: String,
    /// Tempo du transport : **seule** vérité pour le moteur.
    pub tempo_bpm: f64,
    pub master: Option<Id>,
    /// Boucle du projet, en beats `(début, fin)`.
    pub loop_beats: Option<(f64, f64)>,
    /// Gain général de l'écoute (0–2). N'affecte pas les fichiers sources.
    #[serde(default = "default_master_gain")]
    pub master_gain: f32,
    /// Métronome du transport (clics générés par le moteur, jamais exportés).
    #[serde(default)]
    pub metronome: bool,
    pub next_id: Id,
    pub sources: Vec<Source>,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
}

fn default_master_gain() -> f32 {
    1.0
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: VERSION,
            name: "projet".into(),
            tempo_bpm: 120.0,
            master: None,
            loop_beats: None,
            master_gain: 1.0,
            metronome: false,
            next_id: 1,
            sources: Vec::new(),
            tracks: Vec::new(),
            clips: Vec::new(),
        }
    }
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: sanitize_name(name),
            ..Default::default()
        }
    }

    pub fn next_id(&mut self) -> Id {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// Ajoute une source, ou rend l'index de celle qui a déjà ce fichier et ce
    /// sous-morceau (ouvrir deux fois la même piste ne duplique rien).
    pub fn add_source(&mut self, source: Source) -> usize {
        if let Some(i) = self
            .sources
            .iter()
            .position(|s| s.file == source.file && s.subsong == source.subsong)
        {
            return i;
        }
        self.sources.push(source);
        self.sources.len() - 1
    }

    pub fn add_track(&mut self, name: &str, source: Option<usize>, color: u8) -> Id {
        let id = self.next_id();
        let role = if self.master.is_none() {
            Role::Master
        } else {
            Role::Follow
        };
        if role == Role::Master {
            self.master = Some(id);
        }
        self.tracks.push(Track {
            id,
            name: name.into(),
            source,
            role,
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            stretch: StretchMode::Repitch,
            key_lock: false,
            transpose: 0,
            color,
        });
        id
    }

    pub fn add_clip(&mut self, mut clip: Clip) -> Id {
        let id = self.next_id();
        clip.id = id;
        self.clips.push(clip);
        id
    }

    pub fn track(&self, id: Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn clip(&self, id: Id) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn clip_mut(&mut self, id: Id) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == id)
    }

    /// Supprime une piste **et ses clips**, y compris la dernière : un projet
    /// vide est valide, et le dépôt d'une tranche recrée une piste. Si c'était
    /// la maîtresse, la première restante prend le rôle (aucune s'il n'en
    /// reste aucune).
    pub fn remove_track(&mut self, id: Id) -> bool {
        if self.track(id).is_none() {
            return false;
        }
        self.clips.retain(|c| c.track != id);
        self.tracks.retain(|t| t.id != id);
        if self.master == Some(id) {
            self.master = self.tracks.first().map(|t| t.id);
            if let Some(first) = self.tracks.first_mut() {
                first.role = Role::Master;
            }
            // la nouvelle maîtresse repose le tempo, comme un switch manuel
            if let Some(bpm) = self.master.and_then(|m| self.track(m)).and_then(|t| self.track_bpm(t)) {
                self.tempo_bpm = bpm.clamp(20.0, 400.0);
            }
        }
        true
    }

    pub fn remove_clip(&mut self, id: Id) -> bool {
        let before = self.clips.len();
        self.clips.retain(|c| c.id != id);
        self.clips.len() != before
    }

    /// BPM porté par une piste (celui de sa source), s'il existe.
    pub fn track_bpm(&self, track: &Track) -> Option<f64> {
        let s = self.sources.get(track.source?)?;
        if s.bpm > 1.0 && s.bpm.is_finite() {
            Some(s.bpm)
        } else {
            None
        }
    }

    /// Change la piste maîtresse et **pose** le tempo du transport sur le sien
    /// (une fois) : c'est le geste « on choisit une principale et on peut
    /// switcher quand on veut ».
    pub fn set_master(&mut self, id: Id) -> bool {
        if self.track(id).is_none() {
            return false;
        }
        for t in &mut self.tracks {
            t.role = if t.id == id {
                Role::Master
            } else {
                Role::Follow
            };
        }
        self.master = Some(id);
        if let Some(t) = self.track(id) {
            if let Some(bpm) = self.track_bpm(t) {
                self.tempo_bpm = bpm.clamp(20.0, 400.0);
            }
        }
        true
    }

    pub fn master_key(&self) -> Option<Key> {
        let t = self.track(self.master?)?;
        let s = self.sources.get(t.source?)?;
        s.key
    }

    /// Demi-tons effectifs d'un clip : surcharge du clip, sinon piste, plus la
    /// transposition « même gamme » quand elle est active.
    pub fn effective_transpose(&self, clip: &Clip) -> i32 {
        let track = self.track(clip.track);
        let base = clip.transpose.or(track.map(|t| t.transpose)).unwrap_or(0);
        let lock = clip.key_lock.or(track.map(|t| t.key_lock)).unwrap_or(false);
        if !lock {
            return base;
        }
        // tonalité de la tranche d'abord : c'est elle qui décrit le matériau
        // réellement déposé, la source entière n'est qu'un repli
        let mine = clip
            .key
            .or_else(|| self.sources.get(clip.source).and_then(|s| s.key));
        match (mine, self.master_key()) {
            (Some(a), Some(b)) => base + a.transpose_for_scale(&b),
            _ => base,
        }
    }

    /// Le clip doit-il passer par le stretch ? « Même gamme » impose le stretch :
    /// en repitch, transposer casserait la synchro (la durée changerait).
    pub fn effective_stretch(&self, clip: &Clip) -> StretchMode {
        let track = self.track(clip.track);
        let key_lock = clip.key_lock.or(track.map(|t| t.key_lock)).unwrap_or(false);
        if key_lock {
            return StretchMode::Stretch;
        }
        clip.stretch
            .or(track.map(|t| t.stretch))
            .unwrap_or(StretchMode::Repitch)
    }

    /// Gain effectif d'un clip (mute/solo de piste et gain de clip).
    pub fn clip_gain(&self, clip: &Clip) -> f32 {
        let Some(t) = self.track(clip.track) else {
            return 0.0;
        };
        if t.mute {
            return 0.0;
        }
        let any_solo = self.tracks.iter().any(|x| x.solo);
        if any_solo && !t.solo {
            return 0.0;
        }
        (clip.gain.clamp(0.0, 2.0) * t.gain.clamp(0.0, 2.0)).clamp(0.0, 4.0)
    }

    /// Fin du projet en beats (le dernier clip), au moins une mesure.
    pub fn len_beats(&self) -> f64 {
        self.clips
            .iter()
            .map(|c| c.at_beat + c.length_beats)
            .fold(0.0f64, f64::max)
            .max(4.0)
    }

    pub fn beat_sec(&self) -> f64 {
        60.0 / self.tempo_bpm.clamp(20.0, 400.0)
    }

    /// Répare ce qui peut l'être après une ouverture : références mortes,
    /// valeurs aberrantes. Ne jette jamais un fichier lisible en entier.
    pub fn validate(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.version > VERSION {
            notes.push(format!(
                "version {} inconnue (lue comme v{VERSION})",
                self.version
            ));
            self.version = VERSION;
        }
        if !self.tempo_bpm.is_finite() || self.tempo_bpm < 20.0 || self.tempo_bpm > 400.0 {
            self.tempo_bpm = 120.0;
            notes.push("tempo hors bornes : 120 BPM".into());
        }
        let track_ids: HashSet<Id> = self.tracks.iter().map(|t| t.id).collect();
        let src_len = self.sources.len();
        let before = self.clips.len();
        self.clips.retain(|c| {
            track_ids.contains(&c.track)
                && c.source < src_len
                && c.src_end > c.src_start
                && c.length_beats > 0.0
                && c.at_beat.is_finite()
        });
        if self.clips.len() != before {
            notes.push(format!(
                "{} clip(s) invalide(s) retiré(s)",
                before - self.clips.len()
            ));
        }
        let ids: HashSet<Id> = self.tracks.iter().map(|t| t.id).collect();
        if self.master.is_some_and(|m| !ids.contains(&m)) {
            self.master = self.tracks.first().map(|t| t.id);
            notes.push("piste maîtresse absente : première piste".into());
        }
        // unicité des rôles : exactement une maîtresse si le projet a des pistes
        if let Some(m) = self.master {
            for t in &mut self.tracks {
                t.role = if t.id == m {
                    Role::Master
                } else {
                    Role::Follow
                };
            }
        }
        self.next_id = self
            .next_id
            .max(self.tracks.iter().map(|t| t.id + 1).max().unwrap_or(1))
            .max(self.clips.iter().map(|c| c.id + 1).max().unwrap_or(1))
            .max(self.sources.iter().map(|s| s.id + 1).max().unwrap_or(1));
        notes
    }

    /// Écriture atomique (`.part` puis renommage) : un projet à moitié écrit ne
    /// doit jamais remplacer l'ancien.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("gtrack.part");
        let json = serde_json::to_vec_pretty(self).context("sérialisation du projet")?;
        {
            let mut f = std::fs::File::create(&tmp)
                .with_context(|| format!("création de {}", tmp.display()))?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)
            .with_context(|| format!("renommage {} → {}", tmp.display(), path.display()))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?;
        let mut p: Project = serde_json::from_slice(&raw)
            .with_context(|| format!("projet illisible : {}", path.display()))?;
        if p.version == 0 || p.version > 100 {
            bail!("version de projet invalide : {}", p.version);
        }
        p.validate();
        if p.name.is_empty() {
            p.name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "projet".into());
        }
        Ok(p)
    }
}

/// Dossier des projets : `musique/GTracker/projets` (jamais `~/.cache`).
pub fn projects_dir() -> PathBuf {
    crate::util::music_dir().join("GTracker").join("projets")
}

/// Chemin complet d'un projet par son nom.
pub fn project_path(name: &str) -> PathBuf {
    projects_dir().join(format!("{}.gtrack", sanitize_name(name)))
}

/// Projets `.gtrack` d'un dossier, triés par nom (insensible à la casse).
/// Un fichier illisible reste listé : c'est à l'ouverture de le dire.
pub fn projects_in(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("gtrack") {
            continue;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        out.push((name, path));
    }
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out
}

/// Projets du dossier GTracker de l'utilisateur.
pub fn list_projects() -> Vec<(String, PathBuf)> {
    projects_in(&projects_dir())
}

pub fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.trim().chars() {
        if ch.is_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '(' | ')' | '\'' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let out = out.trim_matches(['.', ' ']).to_string();
    if out.is_empty() {
        "projet".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project() -> Project {
        let mut p = Project::new("essai");
        let idx = p.add_source(Source {
            id: 1,
            title: "piste A".into(),
            site: "fichiers".into(),
            entry_id: -1,
            file: PathBuf::from("/tmp/a.wav"),
            subsong: 1,
            bpm: 132.0,
            phase: 0.0,
            key: Some(Key::new(9, crate::key::Mode::Minor, 0.5)),
            loop_start: None,
            loop_end: None,
        });
        let t = p.add_track("piste 1", Some(idx), 0);
        p.add_clip(Clip {
            id: 0,
            track: t,
            source: idx,
            src_start: 0,
            src_end: 44_100,
            at_beat: 0.0,
            length_beats: 4.0,
            gain: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            transpose: None,
            key_lock: None,
            key: None,
            stretch: None,
            fill: Fill::Trim,
        });
        p
    }

    #[test]
    fn aller_retour_json_et_atomique() {
        let p = sample_project();
        let dir = std::env::temp_dir().join(format!("gtrack-test-{}", std::process::id()));
        let path = dir.join("essai.gtrack");
        p.save(&path).expect("save");
        let q = Project::load(&path).expect("load");
        assert_eq!(q.name, "essai");
        assert_eq!(q.clips.len(), 1);
        assert_eq!(q.tracks.len(), 1);
        assert_eq!(q.tempo_bpm, p.tempo_bpm);
        assert_eq!(q.clips[0].src_end, 44_100);
        // les champs récents survivent au disque (un projet d'hier s'ouvre
        // avec les défauts, un projet d'aujourd'hui garde ses réglages)
        assert_eq!(q.master_gain, p.master_gain);
        assert_eq!(q.metronome, p.metronome);
        assert_eq!(q.clips[0].key, p.clips[0].key);
        // pas de `.part` laissé derrière soi
        assert!(!path.with_extension("gtrack.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn master_switch_pose_le_tempo() {
        let mut p = sample_project();
        let t2 = p.add_track("piste 2", Some(0), 1);
        p.sources[0].bpm = 90.0;
        assert!(p.set_master(t2));
        assert_eq!(p.tempo_bpm, 90.0);
        assert_eq!(p.track(t2).unwrap().role, Role::Master);
        assert_eq!(p.tracks[0].role, Role::Follow);
        // une seule maîtresse
        assert_eq!(
            p.tracks.iter().filter(|t| t.role == Role::Master).count(),
            1
        );
    }

    #[test]
    fn meme_gamme_impose_stretch_et_transpose() {
        let mut p = sample_project();
        // maîtresse en do majeur, clip en la mineur (mêmes notes) → 0 demi-ton
        let mut src = p.sources[0].clone();
        src.key = Some(Key::new(9, crate::key::Mode::Minor, 0.8));
        p.sources[0] = src;
        p.sources.push(Source {
            id: 2,
            title: "maître".into(),
            site: "fichiers".into(),
            entry_id: -2,
            file: PathBuf::from("/tmp/b.wav"),
            subsong: 1,
            bpm: 120.0,
            phase: 0.0,
            key: Some(Key::new(0, crate::key::Mode::Major, 0.8)),
            loop_start: None,
            loop_end: None,
        });
        let tm = p.add_track("maître", Some(1), 2);
        p.set_master(tm);
        let cid = p.clips[0].id;
        p.clip_mut(cid).unwrap().key_lock = Some(true);
        assert_eq!(p.effective_transpose(&p.clips[0]), 0);
        assert_eq!(p.effective_stretch(&p.clips[0]), StretchMode::Stretch);
        // ré majeur : +2 demi-tons
        p.sources[1].key = Some(Key::new(2, crate::key::Mode::Major, 0.8));
        assert_eq!(p.effective_transpose(&p.clips[0]), 2);
    }

    #[test]
    fn la_tonalite_de_la_tranche_prime_sur_la_source() {
        let mut p = sample_project();
        // maître en do majeur ; la source est en la mineur (0 demi-ton),
        // mais la tranche déposée est en ré majeur → 10 demi-tons (ou -2)
        p.sources[0].key = Some(Key::new(9, crate::key::Mode::Minor, 0.8));
        p.sources.push(Source {
            id: 2,
            title: "maître".into(),
            site: "fichiers".into(),
            entry_id: -2,
            file: PathBuf::from("/tmp/b.wav"),
            subsong: 1,
            bpm: 120.0,
            phase: 0.0,
            key: Some(Key::new(0, crate::key::Mode::Major, 0.8)),
            loop_start: None,
            loop_end: None,
        });
        let tm = p.add_track("maître", Some(1), 2);
        p.set_master(tm);
        let cid = p.clips[0].id;
        {
            let c = p.clip_mut(cid).unwrap();
            c.key_lock = Some(true);
            c.key = Some(Key::new(2, crate::key::Mode::Major, 0.7));
        }
        // ré majeur → do majeur : -2 demi-tons (le plus court chemin dans ±6)
        assert_eq!(p.effective_transpose(&p.clips[0]), -2);
        // sans tonalité de tranche, on retombe sur la source (la mineur = 0)
        p.clip_mut(cid).unwrap().key = None;
        assert_eq!(p.effective_transpose(&p.clips[0]), 0);
    }

    #[test]
    fn liste_des_projets_trie_par_nom() {
        let dir = std::env::temp_dir().join(format!("gtrack-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["beta.gtrack", "alpha.gtrack", "note.txt"] {
            std::fs::write(dir.join(n), b"{}").unwrap();
        }
        let list = projects_in(&dir);
        assert_eq!(list.len(), 2, "seuls les .gtrack comptent");
        assert_eq!(list[0].0, "alpha");
        assert_eq!(list[1].0, "beta");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn supprimer_une_piste_emporte_ses_clips() {
        let mut p = sample_project();
        let t2 = p.add_track("piste 2", Some(0), 1);
        assert_eq!(p.master, Some(p.tracks[0].id));
        // la piste 2 reçoit un clip
        let cid = p.add_clip(Clip {
            id: 0,
            track: t2,
            source: 0,
            src_start: 0,
            src_end: 1000,
            at_beat: 0.0,
            length_beats: 1.0,
            gain: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            transpose: None,
            key_lock: None,
            key: None,
            stretch: None,
            fill: Fill::Trim,
        });
        assert!(p.remove_track(t2));
        assert!(p.clip(cid).is_none(), "les clips de la piste partent");
        assert_eq!(p.tracks.len(), 1);
        // la dernière piste part aussi : un projet vide est valide, le dépôt
        // d'une tranche recrée une piste
        let last = p.tracks[0].id;
        assert!(p.remove_track(last));
        assert!(p.tracks.is_empty());
        assert_eq!(p.master, None, "plus de maîtresse sans piste");
        // supprimer la maîtresse promeut une autre piste
        let mut q = sample_project();
        let m = q.tracks[0].id;
        let t3 = q.add_track("trois", Some(0), 2);
        assert_eq!(q.master, Some(m));
        assert!(q.remove_track(m));
        assert_eq!(q.master, Some(t3));
        assert_eq!(q.track(t3).unwrap().role, Role::Master);
    }

    #[test]
    fn validation_retire_les_references_mortes() {
        let mut p = sample_project();
        p.clips[0].track = 999;
        p.tempo_bpm = 0.0;
        let notes = p.validate();
        assert!(p.clips.is_empty());
        assert_eq!(p.tempo_bpm, 120.0);
        assert!(!notes.is_empty());
    }

    #[test]
    fn nom_de_fichier_propre() {
        assert_eq!(sanitize_name("Mon / projet: 1"), "Mon _ projet_ 1");
        assert_eq!(sanitize_name("  .. "), "projet");
        assert!(project_path("x").ends_with("x.gtrack"));
    }
}
