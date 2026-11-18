use std::io::ErrorKind;

use nothings_orm::entity::Entity as EntityTrait;
use nothings_orm::repository::Repository;
use nothings_orm::value::Value;
use nothings_orm::Entity;
use nothings_orm_sqlite::SqliteDriver;

#[derive(Debug, Entity)]
#[entity(table = "kv")]
struct Kv {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "v")]
    v: String,
}

#[test]
fn test_file_db_persists_after_reopen() {
    // 用进程 ID + 固定名字避免并行测试互相踩；测试结束即清理。
    let path = std::env::temp_dir().join(format!(
        "nothings-orm-sqlite-{}-{}.db",
        std::process::id(),
        "persist"
    ));
    let path_str = path.to_str().unwrap();
    let _ = std::fs::remove_file(&path);

    {
        let mut driver = SqliteDriver::open(path_str).unwrap();
        driver
            .execute_batch("CREATE TABLE kv (id INTEGER PRIMARY KEY, v TEXT NOT NULL)")
            .unwrap();
        let mut repo = Repository::new(&mut driver);
        repo.insert(&Kv { id: 0, v: "persisted".into() }).unwrap();
    }

    {
        let mut driver = SqliteDriver::open(path_str).unwrap();
        let mut repo = Repository::new(&mut driver);
        let loaded: Kv = repo.get(Value::I64(1)).unwrap();
        assert_eq!(loaded.v, "persisted");
        assert_eq!(<Kv as EntityTrait>::pk_value(&loaded), Value::I64(1));
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_open_nonexistent_parent() {
    let err = match SqliteDriver::open("/definitely/not/existing/dir/x.db") {
        Ok(_) => panic!("打开不存在的路径应当失败"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), ErrorKind::NotFound);
}
