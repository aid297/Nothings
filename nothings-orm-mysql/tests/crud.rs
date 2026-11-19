mod common;

use std::io::ErrorKind;

use common::{TestDb, User, user};
use nothings_orm::driver::NOrmDriver;
use nothings_orm::entity::NOrmEntity as EntityTrait;
use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmValue;

#[test]
fn test_crud_roundtrip() {
    let mut db = TestDb::with_users();
    let mut repo = NOrmRepository::new(&mut db.driver);

    let alice = user("alice");
    let id = repo.insert(&alice).unwrap();
    assert!(id >= 1);

    let loaded: User = repo.get(NOrmValue::I64(id)).unwrap();
    assert_eq!(loaded.name, "alice");
    assert_eq!(loaded.email, None);
    assert!(!loaded.active);
    assert_eq!(loaded.payload, None);
    assert_eq!(<User as EntityTrait>::pk_value(&loaded), NOrmValue::I64(id));

    let mut updated = loaded;
    updated.name = "alice2".into();
    updated.email = Some("a@example.com".into());
    updated.active = true;
    assert_eq!(repo.update(&updated).unwrap(), 1);

    let loaded: User = repo.get(NOrmValue::I64(id)).unwrap();
    assert_eq!(loaded.name, "alice2");
    assert_eq!(loaded.email.as_deref(), Some("a@example.com"));
    assert!(loaded.active);

    assert_eq!(repo.delete::<User>(NOrmValue::I64(id)).unwrap(), 1);
    assert!(repo.find_one::<User>(NOrmValue::I64(id)).unwrap().is_none());
    assert_eq!(
        repo.get::<User>(NOrmValue::I64(id)).unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn test_query_builder() {
    let mut db = TestDb::with_users();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        for name in ["alice", "bob", "carol"] {
            repo.insert(&user(name)).unwrap();
        }
    }

    let mut repo = NOrmRepository::new(&mut db.driver);
    let rows = repo
        .query::<User>()
        .r#where("name", "bob")
        .order_by("id", false)
        .all()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "bob");

    // MySQL 要求 OFFSET 必须搭配 LIMIT；单独 offset() 会报 1064。
    let rows = repo
        .query::<User>()
        .order_by("id", true)
        .limit(2)
        .offset(1)
        .all()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "bob");
    assert_eq!(rows[1].name, "carol");

    assert_eq!(repo.query::<User>().count().unwrap(), 3);
}

#[test]
fn test_query_builder_invalid_column() {
    let mut db = TestDb::with_users();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let err = repo
        .query::<User>()
        .r#where("name; DROP TABLE users", "x")
        .all()
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    // core 的白名单在生成 SQL 前就拦下，表仍在。
    assert_eq!(repo.find_all::<User>().unwrap().len(), 0);
}

#[test]
fn test_constraint_conflict() {
    let mut db = TestDb::with_users();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let dup = user("dup");
    repo.insert(&dup).unwrap();
    assert_eq!(repo.insert(&dup).unwrap_err().kind(), ErrorKind::AlreadyExists);
}

#[test]
fn test_transaction_commit_and_rollback() {
    let mut db = TestDb::with_users();
    let mut repo = NOrmRepository::new(&mut db.driver);

    repo.transaction(|repo| {
        repo.insert(&user("committed"))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(repo.find_all::<User>().unwrap().len(), 1);

    let result: Result<(), std::io::Error> = repo.transaction(|repo| {
        repo.insert(&user("rolled_back"))?;
        Err(std::io::Error::other("boom"))
    });
    assert!(result.is_err());
    assert_eq!(repo.find_all::<User>().unwrap().len(), 1);
}

#[test]
fn test_nested_transaction_rejected() {
    let mut db = TestDb::with_users();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let err = repo
        .transaction(|repo| repo.transaction(|_| Ok(())))
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
}

#[test]
fn test_count_alias() {
    let mut db = TestDb::with_users();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        repo.insert(&user("alice")).unwrap();
        repo.insert(&user("bob")).unwrap();
    }
    let rows = db
        .driver
        .query("SELECT COUNT(*) AS __count FROM users", &[])
        .unwrap();
    assert_eq!(rows[0].names()[0], "__count");
    assert_eq!(rows[0].values()[0], NOrmValue::I64(2));
    assert_eq!(NOrmRepository::new(&mut db.driver).query::<User>().count().unwrap(), 2);
}

#[test]
fn test_auto_increment_last_insert_id() {
    let mut db = TestDb::with_users();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let first = repo.insert(&user("first")).unwrap();
    let second = repo.insert(&user("second")).unwrap();
    assert_eq!(second, first + 1);
    let loaded: User = repo.get(NOrmValue::I64(second)).unwrap();
    assert_eq!(loaded.name, "second");
    assert_eq!(<User as EntityTrait>::pk_value(&loaded), NOrmValue::I64(second));
}
