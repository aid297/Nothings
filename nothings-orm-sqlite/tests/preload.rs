//! Preload（单层关联聚合）的 SQLite 集成测试。

#![allow(dead_code)]

use nothings_orm::repository::NOrmRepository;
use nothings_orm::NOrmEntity;
use nothings_orm_sqlite::NOrmSqliteDriver;

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
    #[entity(nullable, column = "author_id")]
    author_id: Option<i64>,
    #[entity(column = "title")]
    title: String,
    #[entity(belongs_to = "author", target = "Author", foreign_key = "author_id")]
    author: Option<Author>,
}

fn setup() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE books (id INTEGER PRIMARY KEY, author_id INTEGER, title TEXT NOT NULL);",
        )
        .unwrap();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert(&Author { id: 0, name: "A".into(), books: Vec::new() }).unwrap();
        repo.insert(&Author { id: 0, name: "B".into(), books: Vec::new() }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(1), title: "one".into(), author: None }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(1), title: "two".into(), author: None }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(2), title: "three".into(), author: None }).unwrap();
        // 一本无主的书，测 NULL 外键
        repo.insert(&Book { id: 0, author_id: None, title: "orphan".into(), author: None }).unwrap();
    }
    driver
}

#[test]
fn test_preload_has_many() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let mut authors = repo.query::<Author>().order_by("id", true).preload("books").all().unwrap();
    authors.sort_by_key(|a| a.id);
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0].name, "A");
    assert_eq!(authors[0].books.len(), 2);
    assert_eq!(authors[1].books.len(), 1);
    assert_eq!(authors[1].books[0].title, "three");
}

#[test]
fn test_preload_belongs_to() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let mut books = repo.query::<Book>().order_by("id", true).preload("author").all().unwrap();
    books.sort_by_key(|b| b.id);
    assert_eq!(books.len(), 4);
    assert_eq!(books[0].author.as_ref().unwrap().name, "A");
    assert_eq!(books[2].author.as_ref().unwrap().name, "B");
    // 无主的书 → None
    assert!(books[3].author.is_none());
}

#[test]
fn test_preload_empty_relation() {
    let mut driver = setup();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        repo.insert(&Author { id: 0, name: "C".into(), books: Vec::new() }).unwrap();
    }
    let mut repo = NOrmRepository::new(&mut driver);
    let authors = repo
        .query::<Author>()
        .preload("books")
        .r#where("name", "C")
        .all()
        .unwrap();
    assert_eq!(authors.len(), 1);
    assert!(authors[0].books.is_empty());
}

#[test]
fn test_preload_both_directions() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let authors = repo.query::<Author>().preload("books").all().unwrap();
    assert!(authors.iter().any(|a| a.books.len() == 2));

    let books = repo.query::<Book>().preload("author").all().unwrap();
    assert_eq!(books.iter().filter(|b| b.author.is_some()).count(), 3);
}

#[test]
fn test_preload_empty_result() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let authors = repo
        .query::<Author>()
        .preload("books")
        .r#where("id", 999i64)
        .all()
        .unwrap();
    assert!(authors.is_empty());
}

#[test]
fn test_preload_chunk_500() {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE books (id INTEGER PRIMARY KEY, author_id INTEGER, title TEXT NOT NULL);",
        )
        .unwrap();
    {
        let mut repo = NOrmRepository::new(&mut driver);
        for i in 1..=501 {
            repo.insert(&Author { id: 0, name: format!("A{i}"), books: Vec::new() }).unwrap();
            repo.insert(&Book {
                id: 0,
                author_id: Some(i),
                title: format!("b{i}"),
                author: None,
            })
            .unwrap();
        }
    }
    let mut repo = NOrmRepository::new(&mut driver);
    let authors = repo.query::<Author>().preload("books").all().unwrap();
    assert_eq!(authors.len(), 501);
    assert!(authors.iter().all(|a| a.books.len() == 1));
    assert!(authors.iter().all(|a| a.books[0].author_id == Some(a.id)));
}
