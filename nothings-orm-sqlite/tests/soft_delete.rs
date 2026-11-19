//! 软删除 / 时间戳钩子的 SQLite 集成测试。

#![allow(dead_code)]

use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmDateTime;
use nothings_orm::NOrmEntity;
use nothings_orm_sqlite::NOrmSqliteDriver;

#[derive(Debug, NOrmEntity)]
#[entity(table = "notes")]
struct Note {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "title")]
    title: String,
    #[entity(soft_delete, column = "deleted_at")]
    deleted_at: Option<NOrmDateTime>,
    #[entity(created_at)]
    created_at: NOrmDateTime,
    #[entity(updated_at)]
    updated_at: NOrmDateTime,
}

fn setup() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE notes (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                deleted_at TEXT DEFAULT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );",
        )
        .unwrap();
    driver
}

fn note(id: i64, title: &str) -> Note {
    let now = NOrmDateTime::now_utc();
    Note {
        id,
        title: title.into(),
        deleted_at: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

#[test]
fn test_soft_delete_hides_from_reads() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert(&note(1, "one")).unwrap();
        repo.insert(&note(2, "two")).unwrap();
        assert_eq!(repo.find_all::<Note>().unwrap().len(), 2);
        assert_eq!(repo.query::<Note>().count().unwrap(), 2);

        assert_eq!(repo.delete::<Note>(nothings_orm::value::NOrmValue::I64(1)).unwrap(), 1);
        assert_eq!(repo.find_all::<Note>().unwrap().len(), 1);
        assert_eq!(repo.query::<Note>().count().unwrap(), 1);
        assert!(repo.find_one::<Note>(nothings_orm::value::NOrmValue::I64(1)).unwrap().is_none());
        assert!(repo.get::<Note>(nothings_orm::value::NOrmValue::I64(1)).is_err());
    }
}

#[test]
fn test_unscoped_sees_deleted() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        let id = repo.insert(&note(1, "one")).unwrap();
        repo.delete::<Note>(nothings_orm::value::NOrmValue::I64(id)).unwrap();

        let visible = repo.find_all::<Note>().unwrap();
        assert!(visible.is_empty());
        let all: Vec<Note> = repo.query::<Note>().unscoped().all().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].deleted_at.is_some());
    }
}

#[test]
fn test_hard_delete_removes_row() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        let id = repo.insert(&note(1, "one")).unwrap();
        repo.hard_delete::<Note>(nothings_orm::value::NOrmValue::I64(id)).unwrap();
        assert_eq!(repo.query::<Note>().unscoped().all().unwrap().len(), 0);
    }
}

#[test]
fn test_update_deleted_row_affected_zero() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        let id = repo.insert(&note(1, "one")).unwrap();
        repo.delete::<Note>(nothings_orm::value::NOrmValue::I64(id)).unwrap();
        let mut stale = note(id, "changed");
        stale.id = id;
        assert_eq!(repo.update(&stale).unwrap(), 0);
    }
}

#[test]
fn test_timestamp_hooks() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        let id = repo.insert(&note(1, "one")).unwrap();
        let saved = repo.get::<Note>(nothings_orm::value::NOrmValue::I64(id)).unwrap();
        assert!(saved.deleted_at.is_none());
        assert_eq!(saved.created_at, saved.updated_at, "插入时两列应为同一 now");

        // update 后 updated_at 前进、created_at 不变
        let mut changed = note(id, "changed");
        changed.created_at = NOrmDateTime::parse("2000-01-01 00:00:00").unwrap();
        repo.update(&changed).unwrap();
        let after = repo.get::<Note>(nothings_orm::value::NOrmValue::I64(id)).unwrap();
        assert_eq!(after.created_at, saved.created_at, "created_at 不应被 UPDATE 改动");
        assert!(
            after.updated_at.to_unix_secs(&nothings_orm::value::NOrmUtcOffset::UTC)
                >= saved.updated_at.to_unix_secs(&nothings_orm::value::NOrmUtcOffset::UTC)
        );
        assert_eq!(after.title, "changed");
    }
}

#[test]
fn test_managed_values_not_written_back_to_entity() {
    let mut driver = setup();
    let entity = note(1, "one");
    let before = entity.updated_at.clone();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert(&entity).unwrap();
    }
    // &E 签名：内存实体字段保持不变
    assert_eq!(entity.updated_at, before);
    assert!(entity.deleted_at.is_none());
}
