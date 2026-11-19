//! 批量插入 / upsert 的 SQLite 集成测试（真库，验证方言 SQL 端到端）。

#![allow(dead_code)]

use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmDateTime;
use nothings_orm::NOrmEntity;
use nothings_orm_sqlite::NOrmSqliteDriver;

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

fn setup_items() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER NOT NULL);")
        .unwrap();
    driver
}

fn setup_notes() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE notes (
                id INTEGER PRIMARY KEY,
                body TEXT NOT NULL,
                deleted_at TEXT DEFAULT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );",
        )
        .unwrap();
    driver
}

fn item(id: i64, name: &str, qty: i64) -> Item {
    Item { id, name: name.into(), qty }
}

#[test]
fn batch_insert_rows_land_in_db() {
    let mut driver = setup_items();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        let rows = vec![item(1, "a", 10), item(2, "b", 20), item(3, "c", 30)];
        assert_eq!(repo.insert_batch(&rows).unwrap(), 3);
    }
    // 用 query 验证行数与内容
    let mut repo = NOrmRepository::new(&mut driver);
    assert_eq!(repo.query::<Item>().count().unwrap(), 3);
    let mut all = repo.find_all::<Item>().unwrap();
    all.sort_by_key(|x| x.id);
    assert_eq!(all[0].name, "a");
    assert_eq!(all[2].qty, 30);
}

#[test]
fn batch_insert_empty_is_noop() {
    let mut driver = setup_items();
    let mut repo = NOrmRepository::new(&mut driver);
    assert_eq!(repo.insert_batch::<Item>(&[]).unwrap(), 0);
    assert_eq!(repo.query::<Item>().count().unwrap(), 0);
}

#[test]
fn batch_insert_respects_param_chunking() {
    // 每行 2 列 → 每块 450 行；插入 460 行分两块，全部落库。
    let mut driver = setup_items();
    let rows = (1..=460).map(|i| item(i, "x", i)).collect::<Vec<_>>();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        assert_eq!(repo.insert_batch(&rows).unwrap(), 460);
    }
    let mut repo = NOrmRepository::new(&mut driver);
    assert_eq!(repo.query::<Item>().count().unwrap(), 460);
}

#[test]
fn upsert_inserts_new_and_updates_existing() {
    let mut driver = setup_items();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        // 先插入两条
        repo.insert_batch(&[item(1, "orig", 1), item(2, "keep", 2)]).unwrap();
        // upsert：id=1 更新，id=3 新增
        let mixed = vec![item(1, "changed", 99), item(3, "new", 7)];
        assert_eq!(repo.upsert_batch(&mixed).unwrap(), 2);
    }
    let mut repo = NOrmRepository::new(&mut driver);
    let one = repo.get::<Item>(nothings_orm::value::NOrmValue::I64(1)).unwrap();
    assert_eq!(one.name, "changed");
    assert_eq!(one.qty, 99);
    let three = repo.get::<Item>(nothings_orm::value::NOrmValue::I64(3)).unwrap();
    assert_eq!(three.name, "new");
    assert_eq!(repo.query::<Item>().count().unwrap(), 3);
}

#[test]
fn batch_insert_managed_columns() {
    let mut driver = setup_notes();
    let old = NOrmDateTime::new(2000, 1, 1, 0, 0, 0, 0).unwrap();
    let note = Note {
        id: 1,
        body: "hello".into(),
        deleted_at: Some(old.clone()), // 实体自带软删值应被忽略
        created_at: old.clone(),
        updated_at: old.clone(),
    };
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert_batch(&[note]).unwrap();
    }
    let mut repo = NOrmRepository::new(&mut driver);
    let got = repo.get::<Note>(nothings_orm::value::NOrmValue::I64(1)).unwrap();
    assert!(got.deleted_at.is_none(), "软删列应强制 NULL");
    assert!(
        got.created_at.to_unix_secs(&nothings_orm::value::NOrmUtcOffset::UTC)
            > old.to_unix_secs(&nothings_orm::value::NOrmUtcOffset::UTC),
        "created_at 应注入 now"
    );
    assert_eq!(got.created_at, got.updated_at, "同批注入同一 now");
}

#[test]
fn upsert_preserves_soft_deleted_and_created_at() {
    let mut driver = setup_notes();
    let old = NOrmDateTime::new(2000, 1, 1, 0, 0, 0, 0).unwrap();
    let mut base = Note {
        id: 1,
        body: "v1".into(),
        deleted_at: None,
        created_at: old.clone(),
        updated_at: old.clone(),
    };
    let created_before;
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert(&base).unwrap();
        // 软删除该行
        repo.delete::<Note>(nothings_orm::value::NOrmValue::I64(1)).unwrap();
        // 记录库中真实的 created_at（insert 钩子已注入 now，非实体自带的 old）
        created_before = repo
            .query::<Note>()
            .unscoped()
            .one()
            .unwrap()
            .unwrap()
            .created_at;
        // upsert 同主键：应更新 body/updated_at，但不复活软删、不改 created_at
        base.body = "v2".into();
        repo.upsert_batch(&[base]).unwrap();
    }
    let mut repo = NOrmRepository::new(&mut driver);
    // 软删行默认读不到
    assert_eq!(repo.find_all::<Note>().unwrap().len(), 0);
    let got = repo.query::<Note>().unscoped().one().unwrap().unwrap();
    assert_eq!(got.body, "v2");
    assert!(got.deleted_at.is_some(), "软删行不应被 upsert 复活");
    assert_eq!(got.created_at, created_before, "created_at 不应被 upsert 覆盖");
}
