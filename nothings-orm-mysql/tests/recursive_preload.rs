//! 递归 Preload 与懒加载的 MySQL 集成测试。
//! 强制真库，需 NOTHINGS_MYSQL_DSN。

#![allow(dead_code)]

use nothings_orm::repository::NOrmRepository;
use nothings_orm::NOrmEntity;
mod common;

use common::TestDb;

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
    #[entity(has_many = "comments", target = "Comment", foreign_key = "book_id")]
    comments: Vec<Comment>,
}

#[derive(Debug, NOrmEntity)]
#[entity(table = "comments")]
struct Comment {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "book_id")]
    book_id: i64,
    #[entity(column = "body")]
    body: String,
}

fn setup() -> TestDb {
    let mut db = TestDb::new();
    db.driver
        .execute_ddl(
            "CREATE TABLE authors (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL);
             CREATE TABLE books (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, author_id BIGINT NULL, title VARCHAR(255) NOT NULL);
             CREATE TABLE comments (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, book_id BIGINT NOT NULL, body VARCHAR(255) NOT NULL);",
        )
        .unwrap();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        repo.insert(&Author { id: 0, name: "A".into(), books: Vec::new() }).unwrap();
        repo.insert(&Author { id: 0, name: "B".into(), books: Vec::new() }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(1), title: "one".into(), author: None, comments: Vec::new() }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(1), title: "two".into(), author: None, comments: Vec::new() }).unwrap();
        repo.insert(&Book { id: 0, author_id: Some(2), title: "three".into(), author: None, comments: Vec::new() }).unwrap();
        repo.insert(&Comment { id: 0, book_id: 1, body: "c1".into() }).unwrap();
        repo.insert(&Comment { id: 0, book_id: 1, body: "c2".into() }).unwrap();
        repo.insert(&Comment { id: 0, book_id: 2, body: "c3".into() }).unwrap();
        repo.insert(&Comment { id: 0, book_id: 3, body: "c4".into() }).unwrap();
    }
    db
}

#[test]
fn test_two_levels() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let authors = repo.query::<Author>().preload("books.author").all().unwrap();
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0].books.len(), 2);
    assert_eq!(authors[0].books[0].author.as_ref().unwrap().name, "A");
    assert_eq!(authors[1].books[0].author.as_ref().unwrap().name, "B");
}

#[test]
fn test_three_levels() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let authors = repo.query::<Author>().preload("books.comments").all().unwrap();
    assert_eq!(authors[0].books.len(), 2);
    assert_eq!(authors[0].books[0].comments.len(), 2);
    assert_eq!(authors[0].books[1].comments.len(), 1);
    assert_eq!(authors[1].books[0].comments.len(), 1);
}

#[test]
fn test_bidirectional_cycle_bounded() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let authors = repo.query::<Author>().preload("books.author.books").all().unwrap();
    // 每层都重新解码：最深一层的 Author.books 由第 3 段填充
    assert_eq!(authors[0].books[0].author.as_ref().unwrap().books.len(), 2);
}

#[test]
fn test_prefix_merge() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let authors = repo
        .query::<Author>()
        .preload("books")
        .preload("books.comments")
        .all()
        .unwrap();
    assert_eq!(authors[0].books[0].comments.len(), 2);
}

#[test]
fn test_load_relation_single() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let mut authors = repo.query::<Author>().all().unwrap();
    assert!(authors[0].books.is_empty());
    repo.load_relation(&mut authors[0], "books").unwrap();
    assert_eq!(authors[0].books.len(), 2);
    repo.load_relation(&mut authors[0], "books.comments").unwrap();
    assert_eq!(authors[0].books[0].comments.len(), 2);
}

#[test]
fn test_load_relation_unknown() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    let mut authors = repo.query::<Author>().all().unwrap();
    assert!(repo.load_relation(&mut authors[0], "nope").is_err());
}
