use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: i64,
    pub site: String,
    pub folder: String,
    pub name: String,
    pub path: String,
}

impl Entry {
    /// URL du .7z sur joshw.info.
    ///
    /// Le dossier de la base est la vraie source (`a`..`z`, `0-9`, `zzz_unlicensed`,
    /// `zzz_prototypes`) — l'ancien code Go recalculait la première lettre et rendait
    /// les entrées de `zzz_*` inaccessibles.
    pub fn url(&self) -> String {
        format!("{}/{}/{}", self.site, self.folder, self.path)
    }

    /// Nom lisible : `(SNES) Super Mario World` sans l'extension `.7z`.
    pub fn display_name(&self) -> String {
        let short = crate::joshw::short_name(&self.site);
        format!(
            "({short}) {}",
            crate::util::decode_entities(&self.name).trim_end_matches(".7z")
        )
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    /// Numéro de sous-piste passé à zxtune (`fichier?#N`), 1 = début.
    pub subsong: u32,
    pub title: String,
    /// Fichier audio à l'intérieur de l'archive extraite.
    pub file: PathBuf,
    pub length: Option<f64>,
    /// Fenêtre de boucle propre au format (hors fondu) : `(début, fin)`.
    pub loop_start: Option<f64>,
    pub loop_end: Option<f64>,
}

impl Track {
    pub fn label(&self) -> String {
        match self.length {
            Some(l) => format!("{:>7}  {}", crate::util::fmt_time(l), self.title),
            None => format!("{:>5}  {}", "", self.title),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopWindow {
    pub start: f64,
    pub end: f64,
}

impl LoopWindow {
    pub fn len(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}
