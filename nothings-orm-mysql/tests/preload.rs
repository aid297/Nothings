//! Preload（单层关联聚合）的 MySQL 集成测试。
//! 强制真库，需 NOTHINGS_MYSQL_DSN。

mod common;

use common::TestDb;
use nothings_orm::repository::NOrmRepository;
use nothings_orm::NOrmEntity;

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

fn setup() -> TestDb {
    let mut db = TestDb::new();
    db.driver
        .execute_ddl(
            "CREATE TABLE authors (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL);
             CREATE TABLE books (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, author_id BIGINT NULL, title VARCHAR(255) NOT NULL);",
        )
        .unwrap();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        repo.insert(&Author { id: 0, name: "A".into(), books: Vec::new() }).unwrap();
        repo.insert(&Author { id: 0, name: "B".into(), books: Vec::new() }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(1), title: "one".into(), author: None }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(1), title: "two".into(), author: None }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(2), title: "three".into(), author: None }).unwrap();
        repo.insert(&Book { id: 0, author_id: None, title: "orphan".into(), author: None }).unwrap();
    }
    db
}

#[test]
fn test_preload_has_many() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let mut authors = repo.query::<Author>().preload("books").all().unwrap();
    authors.sort_by_key(|a| a.id);
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0].books.len(), 2);
    assert_eq!(authors[1].books.len(), 1);
}

#[test]
fn test_preload_belongs_to() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let mut books = repo.query::<Book>().preload("author").all().unwrap();
    books.sort_by_key(|b| b.id);
    assert_eq!(books.len(), 4);
    assert_eq!(books.iter().filter(|b| b.author.is_some()).count(), 3);
    assert!(books[3].author.is_none());
}

#[test]
fn test_preload_empty_and_null() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let authors = repo.query::<Author>().preload("books").r#where("id", 999i64).all().unwrap();
    assert!(authors.is_empty());

    let books = repo
        .query::<Book>()
        .preload("author")
        .where_condition(nothings_orm::condition::NOrmCondition::<Book>::is_null("author_id"))
        .all()
        .unwrap();
    assert_eq!(books.len(), 1);
    assert!(books[0].author.is_none());
}

#[test]
fn test_preload_chunk_500() {
    let mut db = TestDb::new();
    db.driver
        .execute_ddl(
            "CREATE TABLE authors (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL);
             CREATE TABLE books (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, author_id BIGINT NULL, title VARCHAR(255) NOT NULL);",
        )
        .unwrap();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        for i in 1..=501 {
            repo.insert(&Author { id: 0, name: format!("A{i}"), books: Vec::new() }).unwrap();
            repo.insert(&Book { id: 0, author_id: Some(i), title: format!("b{i}"), author: None })
                .unwrap();
        }
    }
    let mut repo = NOrmRepository::new(&mut db.driver);
    let authors = repo.query::<Author>().preload("books").all().unwrap();
    assert_eq!(authors.len(), 501);
    assert!(authors.iter().all(|a| a.books.len() == 1));
}
