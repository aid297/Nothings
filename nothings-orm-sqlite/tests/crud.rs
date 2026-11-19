use std::io::ErrorKind;

use nothings_orm::entity::NOrmEntity as EntityTrait;
use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmValue;
use nothings_orm::NOrmEntity;
use nothings_orm_sqlite::NOrmSqliteDriver;

#[derive(Debug, NOrmEntity)]
#[entity(table = "users")]
struct User {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "name")]
    name: String,
    #[entity(nullable)]
    email: Option<String>,
}

fn setup() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                email TEXT
            )",
        )
        .unwrap();
    driver
}

#[test]
fn test_crud_roundtrip() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);

    let alice = User {
        id: 0,
        name: "alice".into(),
        email: Some("a@example.com".into()),
    };
    let id = repo.insert(&alice).unwrap();
    assert_eq!(id, 1);

    let loaded: User = repo.get(NOrmValue::I64(id)).unwrap();
    assert_eq!(loaded.name, "alice");
    assert_eq!(loaded.email.as_deref(), Some("a@example.com"));
    assert_eq!(<User as EntityTrait>::pk_value(&loaded), NOrmValue::I64(1));

    let mut updated = loaded;
    updated.name = "alice2".into();
    updated.email = None;
    assert_eq!(repo.update(&updated).unwrap(), 1);

    let loaded: User = repo.get(NOrmValue::I64(1)).unwrap();
    assert_eq!(loaded.name, "alice2");
    assert_eq!(loaded.email, None);

    assert_eq!(repo.delete::<User>(NOrmValue::I64(1)).unwrap(), 1);
    assert!(repo.find_one::<User>(NOrmValue::I64(1)).unwrap().is_none());
    assert_eq!(
        repo.get::<User>(NOrmValue::I64(1)).unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn test_query_builder() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        for name in ["alice", "bob", "carol"] {
            repo.insert(&User {
                id: 0,
                name: name.into(),
                email: None,
            })
            .unwrap();
        }
    }

    let mut repo = NOrmRepository::new(&mut driver);
    let mut query = repo.query::<User>();
    let rows = query
        .r#where("name", "bob")
        .order_by("id", false)
        .all()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "bob");

    let count = repo.query::<User>().count().unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_query_builder_invalid_column() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    // 列名不在 schema 白名单 → InvalidInput，且不生成 SQL
    let err = repo.query::<User>().r#where("name; DROP TABLE users", "x").all().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    // 表仍在
    assert_eq!(repo.find_all::<User>().unwrap().len(), 0);
}

#[test]
fn test_constraint_conflict() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let user = User {
        id: 0,
        name: "dup".into(),
        email: None,
    };
    repo.insert(&user).unwrap();
    let err = repo.insert(&user).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
}

#[test]
fn test_transaction_commit_and_rollback() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);

    repo.transaction(|repo| {
        repo.insert(&User {
            id: 0,
            name: "committed".into(),
            email: None,
        })?;
        Ok(())
    })
    .unwrap();
    assert_eq!(repo.find_all::<User>().unwrap().len(), 1);

    let result: Result<(), std::io::Error> = repo.transaction(|repo| {
        repo.insert(&User {
            id: 0,
            name: "rolled_back".into(),
            email: None,
        })?;
        Err(std::io::Error::other("boom"))
    });
    assert!(result.is_err());
    assert_eq!(repo.find_all::<User>().unwrap().len(), 1);
}

#[test]
fn test_nested_transaction_rejected() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let err = repo
        .transaction(|repo| repo.transaction(|_| Ok(())))
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
}
