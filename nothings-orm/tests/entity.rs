use nothings_orm::entity::NOrmEntity as EntityTrait;
use nothings_orm::row::NOrmOwnedRow;
use nothings_orm::schema::NOrmSchema;
use nothings_orm::value::{NOrmSqlType, NOrmValue};
use nothings_orm::NOrmEntity;

#[derive(Debug, NOrmEntity)]
#[entity(table = "users")]
struct User {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "user_name")]
    name: String,
    #[entity(nullable)]
    email: Option<String>,
}

#[test]
fn test_derive_schema() {
    assert_eq!(User::TABLE, "users");
    assert_eq!(User::COLUMNS.len(), 3);
    assert_eq!(User::COLUMNS[0].name, "id");
    assert_eq!(User::COLUMNS[0].field, "id");
    assert!(User::COLUMNS[0].primary_key);
    assert_eq!(User::COLUMNS[1].name, "user_name");
    assert_eq!(User::COLUMNS[1].field, "name");
    assert_eq!(User::COLUMNS[1].sql_type, NOrmSqlType::Text);
    assert!(User::COLUMNS[2].nullable);
    assert_eq!(User::pk().name, "id");
}

#[test]
fn test_derive_from_row() {
    let row = NOrmOwnedRow::new(
        vec!["id".into(), "user_name".into(), "email".into()],
        vec![
            NOrmValue::I64(7),
            NOrmValue::Text("alice".into()),
            NOrmValue::Text("a@example.com".into()),
        ],
    );
    let user = <User as EntityTrait>::from_row(&row).unwrap();
    assert_eq!(user.id, 7);
    assert_eq!(user.name, "alice");
    assert_eq!(user.email.as_deref(), Some("a@example.com"));
    assert_eq!(user.pk_value(), NOrmValue::I64(7));
}

#[test]
fn test_derive_from_row_nullable() {
    let row = NOrmOwnedRow::new(
        vec!["id".into(), "user_name".into(), "email".into()],
        vec![NOrmValue::I64(8), NOrmValue::Text("bob".into()), NOrmValue::Null],
    );
    let user = <User as EntityTrait>::from_row(&row).unwrap();
    assert_eq!(user.email, None);
}

#[test]
fn test_derive_missing_column() {
    let row = NOrmOwnedRow::new(
        vec!["id".into(), "user_name".into()],
        vec![NOrmValue::I64(9), NOrmValue::Text("carol".into())],
    );
    let err = <User as EntityTrait>::from_row(&row).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("email"));
}
