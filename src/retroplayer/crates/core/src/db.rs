use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::model::{Entry, LoopWindow, Track};

pub struct Db {
    conn: Connection,
    base: PathBuf,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("ouverture de {}", path.display()))?;
        let db = Self {
            conn,
            base: path.to_path_buf(),
        };
        db.init()?;
        db.migrate()?;
        Ok(db)
    }

    /// Colonnes ajoutées après coup : `loop` (fin de boucle mesurée par zxtune).
    fn migrate(&self) -> Result<()> {
        let mut columns: Vec<String> = Vec::new();
        for row in self
            .conn
            .prepare("PRAGMA table_info(tracks)")?
            .query_map([], |r| r.get::<_, String>(1))?
        {
            columns.push(row?);
        }
        for (name, ddl) in [
            ("loop", "ALTER TABLE tracks ADD COLUMN loop REAL"),
            ("loop_start", "ALTER TABLE tracks ADD COLUMN loop_start REAL"),
        ] {
            if !columns.iter().any(|c| c == name) {
                self.conn.execute(ddl, [])?;
            }
        }

        // version 2 : les fenêtres mesurées avant la gestion du fondu PSF étaient
        // fausses (arrêt à `length − fade`) → on les oublie pour les re-mesurer
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 2 {
            self.conn
                .execute("UPDATE tracks SET loop = NULL, loop_start = NULL", [])?;
            self.conn.execute_batch("PRAGMA user_version = 2")?;
        }
        Ok(())
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = 10000;
            PRAGMA temp_store = MEMORY;
            PRAGMA mmap_size = 268435456;
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                site TEXT NOT NULL,
                folder TEXT NOT NULL,
                file_name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                UNIQUE(site, folder, file_name)
            );
            CREATE INDEX IF NOT EXISTS idx_files_site ON files(site);
            CREATE INDEX IF NOT EXISTS idx_files_folder ON files(folder);
            CREATE INDEX IF NOT EXISTS idx_files_name ON files(file_name);
            CREATE INDEX IF NOT EXISTS idx_files_composite ON files(site, folder, file_name);
            CREATE TABLE IF NOT EXISTS tracks (
                entry_id INTEGER NOT NULL,
                ordinal INTEGER NOT NULL,
                subsong INTEGER NOT NULL,
                title TEXT NOT NULL,
                file TEXT NOT NULL,
                length REAL,
                PRIMARY KEY(entry_id, ordinal)
            );
            CREATE TABLE IF NOT EXISTS loops (
                entry_id INTEGER NOT NULL,
                ordinal INTEGER NOT NULL,
                start_ms INTEGER NOT NULL,
                end_ms INTEGER NOT NULL,
                PRIMARY KEY(entry_id, ordinal)
            );
            "#,
        )?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.base
    }

    pub fn count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn all_entries(&self) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, site, folder, file_name, file_path FROM files
             ORDER BY site, folder, file_name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Entry {
                id: r.get(0)?,
                site: r.get(1)?,
                folder: r.get(2)?,
                name: r.get(3)?,
                path: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Insère un lot ; renvoie le nombre de nouvelles lignes.
    pub fn insert_rows(&mut self, rows: &[(String, String, String, String)]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut inserted = 0usize;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO files (site, folder, file_name, file_path)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (site, folder, name, path) in rows {
                inserted += stmt.execute(params![site, folder, name, path])?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    pub fn save_tracks(&self, entry_id: i64, tracks: &[Track], base: &Path) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM tracks WHERE entry_id = ?1", params![entry_id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO tracks (entry_id, ordinal, subsong, title, file, length)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (i, t) in tracks.iter().enumerate() {
                let rel = t
                    .file
                    .strip_prefix(base)
                    .unwrap_or(&t.file)
                    .to_string_lossy()
                    .to_string();
                stmt.execute(params![
                    entry_id,
                    i as i64,
                    t.subsong as i64,
                    t.title,
                    rel,
                    t.length
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_tracks(&self, entry_id: i64, base: &Path) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT subsong, title, file, length, loop_start, loop FROM tracks
             WHERE entry_id = ?1 ORDER BY ordinal",
        )?;
        let rows = stmt.query_map(params![entry_id], |r| {
            let subsong: i64 = r.get(0)?;
            let title: String = r.get(1)?;
            let file: String = r.get(2)?;
            let length: Option<f64> = r.get(3)?;
            let loop_start: Option<f64> = r.get(4)?;
            let loop_end: Option<f64> = r.get(5)?;
            Ok(Track {
                subsong: subsong as u32,
                title,
                file: base.join(file),
                length,
                loop_start,
                loop_end,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Enregistre durée et fin de boucle mesurées par zxtune.
    pub fn set_track_timing(
        &self,
        entry_id: i64,
        ordinal: usize,
        length: Option<f64>,
        loop_start: Option<f64>,
        loop_end: Option<f64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE tracks SET length = COALESCE(?3, length), loop_start = ?4, loop = ?5
             WHERE entry_id = ?1 AND ordinal = ?2",
            params![entry_id, ordinal as i64, length, loop_start, loop_end],
        )?;
        Ok(())
    }

    pub fn track_count(&self, entry_id: i64) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM tracks WHERE entry_id = ?1",
            params![entry_id],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    pub fn forget_tracks(&self, entry_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM tracks WHERE entry_id = ?1", params![entry_id])?;
        Ok(())
    }

    pub fn all_loops(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM loops", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn loop_window(&self, entry_id: i64, ordinal: usize) -> Result<Option<LoopWindow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT start_ms, end_ms FROM loops WHERE entry_id = ?1 AND ordinal = ?2")?;
        let mut rows = stmt.query(params![entry_id, ordinal as i64])?;
        if let Some(r) = rows.next()? {
            let s: i64 = r.get(0)?;
            let e: i64 = r.get(1)?;
            Ok(Some(LoopWindow {
                start: s as f64 / 1000.0,
                end: e as f64 / 1000.0,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn set_loop_window(&self, entry_id: i64, ordinal: usize, w: LoopWindow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO loops (entry_id, ordinal, start_ms, end_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(entry_id, ordinal) DO UPDATE SET start_ms = ?3, end_ms = ?4",
            params![
                entry_id,
                ordinal as i64,
                (w.start * 1000.0).round() as i64,
                (w.end * 1000.0).round() as i64
            ],
        )?;
        Ok(())
    }
}
