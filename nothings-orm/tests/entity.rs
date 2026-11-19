use nothings_orm::entity::{NOrmEntity as EntityTrait, NOrmPreload};
use nothings_orm::row::NOrmOwnedRow;
use nothings_orm::schema::{NOrmRelationKind, NOrmSchema};
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

#[derive(Debug, NOrmEntity)]
#[entity(table = "authors")]
struct Author {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "name")]
    name: String,
    #[entity(has_many = "books", target = "Book", foreign_key = "author_id")]
    books: Vec<Book>,
}

#[derive(Debug, NOrmEntity)]
#[entity(table = "books")]
struct Book {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "author_id")]
    author_id: i64,
    #[entity(column = "title")]
    title: String,
    #[entity(belongs_to = "author", target = "Author", foreign_key = "author_id")]
    author: Option<Author>,
}

#[test]
fn test_relation_metadata_and_defaults() {
    // 关联字段不占列
    assert_eq!(Author::COLUMNS.len(), 2);
    assert_eq!(Book::COLUMNS.len(), 3);

    assert_eq!(Author::RELATIONS.len(), 1);
    let rel = &Author::RELATIONS[0];
    assert_eq!(rel.name, "books");
    assert_eq!(rel.kind, NOrmRelationKind::HasMany);
    assert_eq!(rel.foreign_key, "author_id");
    assert_eq!(rel.target_table, "books");
    assert_eq!(rel.target_columns.len(), 3);

    let rel2 = &Book::RELATIONS[0];
    assert_eq!(rel2.name, "author");
    assert_eq!(rel2.kind, NOrmRelationKind::BelongsTo);
    assert_eq!(rel2.target_table, "authors");

    // 无关联的实体 RELATIONS 为空（默认值生效）
    assert!(User::RELATIONS.is_empty());

    // from_row 给关联字段空初值
    let row = NOrmOwnedRow::new(
        vec!["id".into(), "name".into()],
        vec![NOrmValue::I64(1), NOrmValue::Text("A".into())],
    );
    let author = <Author as EntityTrait>::from_row(&row).unwrap();
    assert!(author.books.is_empty());
}

#[test]
fn test_fill_relation() {
    let row = NOrmOwnedRow::new(
        vec!["id".into(), "name".into()],
        vec![NOrmValue::I64(1), NOrmValue::Text("A".into())],
    );
    let book_row = NOrmOwnedRow::new(
        vec!["id".into(), "author_id".into(), "title".into()],
        vec![
            NOrmValue::I64(10),
            NOrmValue::I64(1),
            NOrmValue::Text("one".into()),
        ],
    );

    let mut author = <Author as EntityTrait>::from_row(&row).unwrap();
    author.fill_relation("books", &[book_row]).unwrap();
    assert_eq!(author.books.len(), 1);
    assert_eq!(author.books[0].title, "one");

    let mut book = <Book as EntityTrait>::from_row(&NOrmOwnedRow::new(
        vec!["id".into(), "author_id".into(), "title".into()],
        vec![
            NOrmValue::I64(10),
            NOrmValue::I64(1),
            NOrmValue::Text("one".into()),
        ],
    ))
    .unwrap();
    book.fill_relation("author", &[row]).unwrap();
    assert_eq!(book.author.as_ref().unwrap().name, "A");

    // 未知名 → InvalidInput
    assert!(author.fill_relation("comments", &[]).is_err());
}
