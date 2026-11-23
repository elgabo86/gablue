//! Index persistant des paquets (SQLite) : au démarrage le launcher affiche la
//! liste sans relire les dossiers, et seuls les changements signalés par le
//! veilleur touchent l'index.
//!
//! La table `dirs` mémorise la date de modification ET la liste des
//! sous-dossiers de chaque dossier : un dossier inchangé n'est ni relu
//! (`readdir`) ni `stat`é fichier par fichier — indispensable sur un disque
//! externe qui contient 170 000 fichiers. Un ajout/suppression quelque part
//! change la mtime du dossier concerné, donc rien ne passe à la trappe.
//!
//! Aucune écriture n'a lieu dans les dossiers de jeux : la base vit dans
//! `<cache>/index.sqlite`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::library::{Kind, PackRow, Walk};

/// Version du schéma : une base d'une autre version est jetée (cache).
const SCHEMA: i64 = 2;

pub struct Index {
    conn: Connection,
}

/// Dossier connu de l'index.
#[derive(Debug, Clone, Default)]
pub struct KnownDir {
    pub mtime: i64,
    pub parent: Option<PathBuf>,
    pub children: Vec<PathBuf>,
}

/// Photo de l'index, utilisée par le parcours pour éviter les lectures inutiles.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub dirs: HashMap<PathBuf, KnownDir>,
    /// paquets connus, groupés par dossier parent
    pub packs: HashMap<PathBuf, Vec<PackRow>>,
}

/// Résultat d'un rafraîchissement : la liste complète et les chemins qui ont
/// changé (pour invalider les visuels mémorisés).
#[derive(Debug, Clone)]
pub struct Refresh {
    pub rows: Vec<PackRow>,
    pub changed: Vec<PathBuf>,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path).map_err(|err| format!("{} : {err}", path.display()))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or_default();
        if version != SCHEMA {
            conn.execute_batch("DROP TABLE IF EXISTS packs; DROP TABLE IF EXISTS dirs;")
                .map_err(|err| format!("migration index : {err}"))?;
            let _ = conn.pragma_update(None, "user_version", SCHEMA);
        }

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS packs (
                 path  TEXT PRIMARY KEY,
                 dir   TEXT NOT NULL,
                 name  TEXT NOT NULL,
                 kind  TEXT NOT NULL,
                 mtime INTEGER NOT NULL,
                 size  INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS packs_dir ON packs (dir);
             CREATE TABLE IF NOT EXISTS dirs (
                 path   TEXT PRIMARY KEY,
                 parent TEXT,
                 mtime  INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS dirs_parent ON dirs (parent);",
        )
        .map_err(|err| format!("schéma index : {err}"))?;

        Ok(Self { conn })
    }

    pub fn all(&self) -> Result<Vec<PackRow>, String> {
        let mut statement = self
            .conn
            .prepare("SELECT path, dir, name, kind, mtime, size FROM packs")
            .map_err(|err| format!("lecture index : {err}"))?;
        let rows = statement
            .query_map([], row_from_sql)
            .map_err(|err| format!("lecture index : {err}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("lecture index : {err}"))
    }

    /// Photo de l'index (arbre des dossiers + paquets par dossier).
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let mut dirs: HashMap<PathBuf, KnownDir> = HashMap::new();
        {
            let mut statement = self
                .conn
                .prepare("SELECT path, parent, mtime FROM dirs")
                .map_err(|err| format!("lecture index : {err}"))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        PathBuf::from(row.get::<_, String>(0)?),
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|err| format!("lecture index : {err}"))?;

            for (path, parent, mtime) in rows.flatten() {
                dirs.insert(
                    path,
                    KnownDir {
                        mtime,
                        parent: parent.map(PathBuf::from),
                        children: Vec::new(),
                    },
                );
            }
        }

        let parents: Vec<(PathBuf, Option<PathBuf>)> = dirs
            .iter()
            .map(|(path, dir)| (path.clone(), dir.parent.clone()))
            .collect();
        for (path, parent) in parents {
            if let Some(parent) = parent
                && let Some(known) = dirs.get_mut(&parent)
            {
                known.children.push(path);
            }
        }
        for dir in dirs.values_mut() {
            dir.children.sort();
        }

        let mut packs: HashMap<PathBuf, Vec<PackRow>> = HashMap::new();
        for row in self.all()? {
            packs.entry(row.dir.clone()).or_default().push(row);
        }

        Ok(Snapshot { dirs, packs })
    }

    /// Remplace tout l'index (scan complet).
    pub fn replace_all(&mut self, walk: &Walk) -> Result<(), String> {
        let mut transaction = self
            .conn
            .transaction()
            .map_err(|err| format!("index : {err}"))?;
        transaction
            .execute("DELETE FROM packs", [])
            .map_err(|err| format!("index : {err}"))?;
        transaction
            .execute("DELETE FROM dirs", [])
            .map_err(|err| format!("index : {err}"))?;
        write(
            &mut transaction,
            &walk.rows,
            &walk.dirs,
            "INSERT OR REPLACE INTO packs (path, dir, name, kind, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        transaction.commit().map_err(|err| format!("index : {err}"))
    }

    /// Remplace le sous-arbre `root` (dossier modifié).
    pub fn replace_subtree(&mut self, root: &Path, walk: &Walk) -> Result<(), String> {
        let mut transaction = self
            .conn
            .transaction()
            .map_err(|err| format!("index : {err}"))?;
        delete_subtree(&transaction, root)?;
        delete_dirs_under(&transaction, root)?;
        write(
            &mut transaction,
            &walk.rows,
            &walk.dirs,
            "INSERT OR REPLACE INTO packs (path, dir, name, kind, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        transaction.commit().map_err(|err| format!("index : {err}"))
    }

    pub fn upsert(&mut self, row: &PackRow) -> Result<(), String> {
        let mut statement = self
            .conn
            .prepare("INSERT OR REPLACE INTO packs (path, dir, name, kind, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
            .map_err(|err| format!("index : {err}"))?;
        insert(&mut statement, row)
    }

    /// Oublie les paquets (et les dossiers) dont le chemin n'existe plus sur
    /// le disque — disque externe débranché, dossier de jeux retiré. Renvoie le
    /// nombre de paquets oubliés. Ne touche à aucun visuel : ils vivent dans
    /// `assets/`.
    pub fn purge_missing(&mut self) -> Result<usize, String> {
        let rows = self.all()?;
        let mut retires = 0;
        for row in rows {
            if row.path.exists() {
                continue;
            }
            self.conn
                .execute(
                    "DELETE FROM packs WHERE path = ?1",
                    params![row.path.to_string_lossy().to_string()],
                )
                .map_err(|err| format!("index : {err}"))?;
            retires += 1;
        }

        let dirs: Vec<String> = {
            let mut statement = self
                .conn
                .prepare("SELECT path FROM dirs")
                .map_err(|err| format!("lecture index : {err}"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|err| format!("lecture index : {err}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|err| format!("lecture index : {err}"))?
        };
        for dir in dirs {
            if Path::new(&dir).exists() {
                continue;
            }
            self.conn
                .execute("DELETE FROM dirs WHERE path = ?1", params![dir])
                .map_err(|err| format!("index : {err}"))?;
        }
        Ok(retires)
    }

    /// Supprime une entrée, et tout son sous-arbre si c'est un dossier disparu.
    pub fn remove_subtree(&mut self, path: &Path) -> Result<(), String> {
        delete_subtree(&self.conn, path)?;
        delete_dirs_under(&self.conn, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_missing_oublie_les_paquets_disparus() {
        let dir = std::env::temp_dir().join(format!("glauncher-index-purge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let present = dir.join("present.wgp");
        std::fs::write(&present, b"x").unwrap();
        let absent = dir.join("absent.lgp");

        let mut index = Index::open(&dir.join("index.sqlite")).unwrap();
        for (path, kind) in [(&present, Kind::Wgp), (&absent, Kind::Lgp)] {
            index
                .upsert(&PackRow {
                    path: path.clone(),
                    dir: dir.clone(),
                    name: path.display().to_string(),
                    kind,
                    mtime: 0,
                    size: 1,
                })
                .unwrap();
        }
        assert_eq!(index.all().unwrap().len(), 2);
        assert_eq!(index.purge_missing().unwrap(), 1);
        let restants = index.all().unwrap();
        assert_eq!(restants.len(), 1);
        assert_eq!(restants[0].path, present);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn write(
    transaction: &mut rusqlite::Transaction<'_>,
    rows: &[PackRow],
    dirs: &[(PathBuf, Option<PathBuf>, i64)],
    pack_sql: &str,
) -> Result<(), String> {
    {
        let mut packs = transaction
            .prepare(pack_sql)
            .map_err(|err| format!("index : {err}"))?;
        for row in rows {
            insert(&mut packs, row)?;
        }

        let mut statement = transaction
            .prepare("INSERT OR REPLACE INTO dirs (path, parent, mtime) VALUES (?1, ?2, ?3)")
            .map_err(|err| format!("index : {err}"))?;
        for (path, parent, mtime) in dirs {
            statement
                .execute(params![
                    path.to_string_lossy(),
                    parent.as_ref().map(|parent| parent.to_string_lossy()),
                    mtime
                ])
                .map_err(|err| format!("index : {err}"))?;
        }
    }
    Ok(())
}

fn row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<PackRow> {
    Ok(PackRow {
        path: PathBuf::from(row.get::<_, String>(0)?),
        dir: PathBuf::from(row.get::<_, String>(1)?),
        name: row.get(2)?,
        kind: Kind::from_db(&row.get::<_, String>(3)?),
        mtime: row.get(4)?,
        size: row.get::<_, i64>(5)? as u64,
    })
}

fn insert(statement: &mut rusqlite::Statement<'_>, row: &PackRow) -> Result<(), String> {
    statement
        .execute(params![
            row.path.to_string_lossy(),
            row.dir.to_string_lossy(),
            row.name,
            row.kind.as_str(),
            row.mtime,
            row.size as i64
        ])
        .map(|_| ())
        .map_err(|err| format!("écriture index : {err}"))
}

fn delete_subtree(conn: &Connection, path: &Path) -> Result<(), String> {
    let exact = path.to_string_lossy().to_string();
    let prefix = format!("{exact}/");

    conn.execute(
        "DELETE FROM packs WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
        params![exact, prefix],
    )
    .map(|_| ())
    .map_err(|err| format!("index : {err}"))
}

fn delete_dirs_under(conn: &Connection, path: &Path) -> Result<(), String> {
    let exact = path.to_string_lossy().to_string();
    let prefix = format!("{exact}/");

    conn.execute(
        "DELETE FROM dirs WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
        params![exact, prefix],
    )
    .map(|_| ())
    .map_err(|err| format!("index : {err}"))
}
