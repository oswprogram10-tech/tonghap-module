//! 「정보」 화면: 데이터 폴더 위치, 저장 공간 사용량, 기록 개수.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::backup::Counts;
use crate::db::{lock, DbExt, DbResult, MediaDir};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    /// 데이터 폴더(`.../data`)
    pub data_dir: String,
    /// 「폴더 열기」로 탐색기에서 선택해 보여 줄 경로(DB 파일이 있으면 그 파일)
    pub reveal_path: String,
    pub db_bytes: u64,
    pub media_bytes: u64,
    pub models_bytes: u64,
    pub total_bytes: u64,
    pub counts: Counts,
}

/// 파일이면 크기, 폴더면 안의 모든 파일 크기 합. 없거나 읽을 수 없으면 0.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(rd) = std::fs::read_dir(path) else { return 0 };
    rd.flatten().map(|e| dir_size(&e.path())).sum()
}

/// DB 파일과 WAL·SHM 크기 합.
pub fn db_size(data_dir: &Path) -> u64 {
    ["tonghap.db", "tonghap.db-wal", "tonghap.db-shm"].iter().map(|n| dir_size(&data_dir.join(n))).sum()
}

pub fn counts(conn: &Connection) -> DbResult<Counts> {
    let count = |sql: &str| -> DbResult<i64> { conn.query_row(sql, [], |r| r.get(0)).db() };
    Ok(Counts {
        classes: count("SELECT count(*) FROM classes")?,
        students: count("SELECT count(*) FROM students")?,
        observations: count("SELECT count(*) FROM observations WHERE deleted_at IS NULL")?,
        attachments: count("SELECT count(*) FROM attachments")?,
    })
}

/// 폴더 계산만 하는 순수 함수(DB 개수는 호출 쪽에서 채운다).
pub fn usage(data_dir: &Path, models_dir: &Path, counts: Counts) -> StorageUsage {
    let db_bytes = db_size(data_dir);
    let media_bytes = dir_size(&data_dir.join("media"));
    let models_bytes = dir_size(models_dir);
    let db_file = data_dir.join("tonghap.db");
    let reveal = if db_file.exists() { db_file } else { data_dir.to_path_buf() };
    StorageUsage {
        data_dir: data_dir.to_string_lossy().into_owned(),
        reveal_path: reveal.to_string_lossy().into_owned(),
        db_bytes,
        media_bytes,
        models_bytes,
        total_bytes: db_bytes + media_bytes + models_bytes,
        counts,
    }
}

fn paths(app: &AppHandle) -> DbResult<(PathBuf, PathBuf)> {
    let data_dir = app
        .state::<MediaDir>()
        .0
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "데이터 폴더를 찾을 수 없습니다".to_string())?;
    // 모델 폴더는 데이터 폴더와 나란히 있다(만들지 않고 위치만 계산).
    let models = data_dir.parent().map(|p| p.join("models")).unwrap_or_else(|| data_dir.join("models"));
    Ok((data_dir, models))
}

#[tauri::command]
pub async fn about_storage(app: AppHandle) -> DbResult<StorageUsage> {
    let (data_dir, models) = paths(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Mutex<Connection>>();
        let c = counts(&*lock(&db)?)?;
        Ok(usage(&data_dir, &models, c))
    })
    .await
    .map_err(|e| format!("작업이 중단되었습니다: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::new_id;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tonghap-about-{}-{}", name, new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sums_files_in_nested_folders() {
        let root = tmp("sum");
        let data = root.join("data");
        let models = root.join("models");
        std::fs::create_dir_all(data.join("media/a/b")).unwrap();
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(data.join("tonghap.db"), vec![0u8; 100]).unwrap();
        std::fs::write(data.join("tonghap.db-wal"), vec![0u8; 30]).unwrap();
        std::fs::write(data.join("media/x.jpg"), vec![0u8; 10]).unwrap();
        std::fs::write(data.join("media/a/b/y.webm"), vec![0u8; 20]).unwrap();
        std::fs::write(models.join("m.gguf"), vec![0u8; 1000]).unwrap();
        let u = usage(&data, &models, Counts::default());
        assert_eq!(u.db_bytes, 130);
        assert_eq!(u.media_bytes, 30);
        assert_eq!(u.models_bytes, 1000);
        assert_eq!(u.total_bytes, 1160);
        assert!(u.reveal_path.ends_with("tonghap.db"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_folders_are_zero() {
        let root = tmp("missing");
        let u = usage(&root.join("nope"), &root.join("nomodels"), Counts::default());
        assert_eq!((u.db_bytes, u.media_bytes, u.models_bytes, u.total_bytes), (0, 0, 0, 0));
        assert_eq!(dir_size(&root.join("nothing")), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn counts_of_empty_db_are_zero() {
        let conn = crate::db::test_conn();
        let c = counts(&conn).unwrap();
        assert_eq!((c.classes, c.students, c.observations, c.attachments), (0, 0, 0, 0));
    }
}
