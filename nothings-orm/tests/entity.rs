use nothings_orm::entity::Entity as EntityTrait;
use nothings_orm::row::OwnedRow;
use nothings_orm::schema::Schema;
use nothings_orm::value::{SqlType, Value};
use nothings_orm::Entity;

#[derive(Debug, Entity)]
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
    assert_eq!(User::COLUMNS[1].sql_type, SqlType::Text);
    assert!(User::COLUMNS[2].nullable);
    assert_eq!(User::pk().name, "id");
}

#[test]
fn test_derive_from_row() {
    let row = OwnedRow::new(
        vec!["id".into(), "user_name".into(), "email".into()],
        vec![
            Value::I64(7),
            Value::Text("alice".into()),
            Value::Text("a@example.com".into()),
        ],
    );
    let user = <User as EntityTrait>::from_row(&row).unwrap();
    assert_eq!(user.id, 7);
    assert_eq!(user.name, "alice");
    assert_eq!(user.email.as_deref(), Some("a@example.com"));
    assert_eq!(user.pk_value(), Value::I64(7));
}

#[test]
fn test_derive_from_row_nullable() {
    let row = OwnedRow::new(
        vec!["id".into(), "user_name".into(), "email".into()],
        vec![Value::I64(8), Value::Text("bob".into()), Value::Null],
    );
    let user = <User as EntityTrait>::from_row(&row).unwrap();
    assert_eq!(user.email, None);
}

#[test]
fn test_derive_missing_column() {
    let row = OwnedRow::new(
        vec!["id".into(), "user_name".into()],
        vec![Value::I64(9), Value::Text("carol".into())],
    );
    let err = <User as EntityTrait>::from_row(&row).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("email"));
}
