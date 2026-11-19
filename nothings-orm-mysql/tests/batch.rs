//! 批量插入 / upsert 的 MySQL 集成测试（真库，验证 ON DUPLICATE KEY UPDATE 方言）。
//! 强制真库，需 NOTHINGS_MYSQL_DSN。

#![allow(dead_code)]

use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::{NOrmDateTime, NOrmUtcOffset, NOrmValue};
use nothings_orm::NOrmEntity;
mod common;

use common::TestDb;

#[derive(Debug, NOrmEntity)]
#[entity(table = "items")]
struct Item {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "name")]
    name: String,
    #[entity(column = "qty")]
    qty: i64,
}

#[derive(Debug, NOrmEntity)]
#[entity(table = "notes")]
struct Note {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "body")]
    body: String,
    #[entity(soft_delete, column = "deleted_at")]
    deleted_at: Option<NOrmDateTime>,
    #[entity(created_at)]
    created_at: NOrmDateTime,
    #[entity(updated_at)]
    updated_at: NOrmDateTime,
}

fn setup_items() -> TestDb {
    let mut db = TestDb::new();
    db.driver
        .execute_ddl(
            "CREATE TABLE items (
                id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                name VARCHAR(255) NOT NULL,
                qty INT NOT NULL
             )",
        )
        .unwrap();
    db
}

fn setup_notes() -> TestDb {
    let mut db = TestDb::new();
    db.driver
        .execute_ddl(
            "CREATE TABLE notes (
                id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                body VARCHAR(255) NOT NULL,
                deleted_at DATETIME(6) NULL,
                created_at DATETIME(6) NOT NULL,
                updated_at DATETIME(6) NOT NULL
             )",
        )
        .unwrap();
    db
}

fn item(id: i64, name: &str, qty: i64) -> Item {
    Item { id, name: name.into(), qty }
}

#[test]
fn batch_insert_rows_land_in_db() {
    let mut db = setup_items();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        let rows = vec![item(1, "a", 10), item(2, "b", 20), item(3, "c", 30)];
        assert_eq!(repo.insert_batch(&rows).unwrap(), 3);
    }
    let mut repo = NOrmRepository::new(&mut db.driver);
    assert_eq!(repo.query::<Item>().count().unwrap(), 3);
    let mut all = repo.find_all::<Item>().unwrap();
    all.sort_by_key(|x| x.id);
    assert_eq!(all[0].name, "a");
    assert_eq!(all[2].qty, 30);
}

#[test]
fn batch_insert_respects_param_chunking() {
    // 每行 2 列 → 每块 450 行；插入 460 行分两块。
    let mut db = setup_items();
    let rows = (1..=460).map(|i| item(i, "x", i)).collect::<Vec<_>>();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        assert_eq!(repo.insert_batch(&rows).unwrap(), 460);
    }
    let mut repo = NOrmRepository::new(&mut db.driver);
    assert_eq!(repo.query::<Item>().count().unwrap(), 460);
}

#[test]
fn upsert_inserts_new_and_updates_existing() {
    let mut db = setup_items();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        repo.insert_batch(&[item(1, "orig", 1), item(2, "keep", 2)]).unwrap();
        // id=1 更新，id=3 新增（ON DUPLICATE KEY UPDATE 以主键为冲突目标）
        let mixed = vec![item(1, "changed", 99), item(3, "new", 7)];
        // MySQL 受影响行数：插入 1 行计 1，更新 1 行计 2 → 共 3
        let n = repo.upsert_batch(&mixed).unwrap();
        assert!(n >= 2, "受影响行数应 >= 2，实际 {n}");
    }
    let mut repo = NOrmRepository::new(&mut db.driver);
    let one = repo.get::<Item>(NOrmValue::I64(1)).unwrap();
    assert_eq!(one.name, "changed");
    assert_eq!(one.qty, 99);
    let three = repo.get::<Item>(NOrmValue::I64(3)).unwrap();
    assert_eq!(three.name, "new");
    assert_eq!(repo.query::<Item>().count().unwrap(), 3);
}

#[test]
fn batch_insert_managed_columns() {
    let mut db = setup_notes();
    let old = NOrmDateTime::from_unix_secs(946_684_800, &NOrmUtcOffset::UTC); // 2000-01-01 UTC
    let note = Note {
        id: 1,
        body: "hello".into(),
        deleted_at: Some(old.clone()),
        created_at: old.clone(),
        updated_at: old.clone(),
    };
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        repo.insert_batch(&[note]).unwrap();
    }
    let mut repo = NOrmRepository::new(&mut db.driver);
    let got = repo.get::<Note>(NOrmValue::I64(1)).unwrap();
    assert!(got.deleted_at.is_none(), "软删列应强制 NULL");
    assert!(
        got.created_at.to_unix_secs(&NOrmUtcOffset::UTC)
            > old.to_unix_secs(&NOrmUtcOffset::UTC),
        "created_at 应注入 now"
    );
}

#[test]
fn upsert_preserves_soft_deleted_and_created_at() {
    let mut db = setup_notes();
    let old = NOrmDateTime::from_unix_secs(946_684_800, &NOrmUtcOffset::UTC);
    let mut base = Note {
        id: 1,
        body: "v1".into(),
        deleted_at: None,
        created_at: old.clone(),
        updated_at: old.clone(),
    };
    let created_before;
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        repo.insert(&base).unwrap();
        repo.delete::<Note>(NOrmValue::I64(1)).unwrap();
        created_before = repo
            .query::<Note>()
            .unscoped()
            .one()
            .unwrap()
            .unwrap()
            .created_at;
        base.body = "v2".into();
        repo.upsert_batch(&[base]).unwrap();
    }
    let mut repo = NOrmRepository::new(&mut db.driver);
    assert_eq!(repo.find_all::<Note>().unwrap().len(), 0);
    let got = repo.query::<Note>().unscoped().one().unwrap().unwrap();
    assert_eq!(got.body, "v2");
    assert!(got.deleted_at.is_some(), "软删行不应被 upsert 复活");
    assert_eq!(got.created_at, created_before, "created_at 不应被 upsert 覆盖");
}
