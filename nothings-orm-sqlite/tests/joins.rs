//! JOIN / 子查询的 SQLite 集成测试。

#![allow(dead_code)]

use nothings_orm::join::{NOrmJoinKind, NOrmSubquery};
use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmValue;
use nothings_orm::NOrmEntity;
use nothings_orm_sqlite::NOrmSqliteDriver;

#[derive(Debug, NOrmEntity)]
#[entity(table = "authors")]
struct Author {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "name")]
    name: String,
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
    #[entity(column = "year")]
    year: i64,
}

fn setup() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE books (id INTEGER PRIMARY KEY, author_id INTEGER NOT NULL,
                                 title TEXT NOT NULL, year INTEGER NOT NULL);",
        )
        .unwrap();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert(&Author { id: 0, name: "A".into() }).unwrap();
        repo.insert(&Author { id: 0, name: "B".into() }).unwrap();
        repo.insert(&Book { id: 0, author_id: 1, title: "one".into(), year: 2020 }).unwrap();
        repo.insert(&Book { id: 0, author_id: 1, title: "two".into(), year: 2022 }).unwrap();
        repo.insert(&Book { id: 0, author_id: 2, title: "three".into(), year: 2021 }).unwrap();
    }
    driver
}

#[test]
fn test_inner_join_tuple_mapping() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let rows = repo
        .query::<Author>()
        .join::<Book>(NOrmJoinKind::Inner, &[("id", "author_id")])
        .all_with::<(Author, Book)>()
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0.name, "A");
    assert_eq!(rows[0].1.title, "one");
    assert_eq!(rows[2].0.name, "B");
    assert_eq!(rows[2].1.title, "three");
}

#[test]
fn test_left_join_keeps_null_side() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert(&Author { id: 0, name: "C".into() }).unwrap();
    }
    let mut repo = NOrmRepository::new(&mut driver);
    let rows = repo
        .query::<Author>()
        .join::<Book>(NOrmJoinKind::Left, &[("id", "author_id")])
        .all()
        .unwrap();
    // 作者 C 没有书，LEFT JOIN 仍返回它（book 字段全 NULL，all() 只抽主表）。
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().any(|a| a.name == "C"));
}

#[test]
fn test_join_with_where_and_count() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let count = repo
        .query::<Author>()
        .join::<Book>(NOrmJoinKind::Inner, &[("id", "author_id")])
        .where_qualified::<Book>("t1", "year", 2021i64)
        .where_op("id", nothings_orm::condition::NOrmOperator::Gt, 0i64)
        .count()
        .unwrap();
    assert_eq!(count, 1); // year = 2021 → book "three"
}

#[test]
fn test_qualified_condition_on_joined_table() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let rows = repo
        .query::<Author>()
        .join::<Book>(NOrmJoinKind::Inner, &[("id", "author_id")])
        .where_qualified::<Book>("t1", "title", "two")
        .all()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "A");
}

#[test]
fn test_subquery_in_and_exists() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);

    // id IN (SELECT author_id FROM books WHERE year = 2022) → 作者 A
    let sub = NOrmSubquery::with_params(
        "SELECT author_id FROM books WHERE year = ?",
        vec![NOrmValue::I64(2022)],
    );
    let rows = repo
        .query::<Author>()
        .where_in_subquery("id", sub)
        .all()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "A");

    // EXISTS 子查询
    let rows = repo
        .query::<Author>()
        .where_exists(NOrmSubquery::new("SELECT 1 FROM books WHERE books.author_id = authors.id"))
        .all()
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_join_all_with_single_entity() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let rows = repo
        .query::<Author>()
        .join::<Book>(NOrmJoinKind::Inner, &[("id", "author_id")])
        .all_with::<(Author,)>()
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0.name, "A");
}
