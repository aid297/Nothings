//! 递归 Preload 与懒加载的 SQLite 集成测试。

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

fn setup() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE books (id INTEGER PRIMARY KEY, author_id INTEGER, title TEXT NOT NULL);
             CREATE TABLE comments (id INTEGER PRIMARY KEY, book_id INTEGER NOT NULL, body TEXT NOT NULL);",
        )
        .unwrap();
    {
        let mut repo = NOrmRepository::new(&mut driver);
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
    driver
}

#[test]
fn test_two_levels() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let authors = repo.query::<Author>().preload("books.author").all().unwrap();
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0].books.len(), 2);
    assert_eq!(authors[0].books[0].author.as_ref().unwrap().name, "A");
    assert_eq!(authors[1].books[0].author.as_ref().unwrap().name, "B");
}

#[test]
fn test_three_levels() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let authors = repo.query::<Author>().preload("books.comments").all().unwrap();
    assert_eq!(authors[0].books.len(), 2);
    assert_eq!(authors[0].books[0].comments.len(), 2);
    assert_eq!(authors[0].books[1].comments.len(), 1);
    assert_eq!(authors[1].books[0].comments.len(), 1);
}

#[test]
fn test_bidirectional_cycle_bounded() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let authors = repo.query::<Author>().preload("books.author.books").all().unwrap();
    // 每层都重新解码：最深一层的 Author.books 由第 3 段填充
    assert_eq!(authors[0].books[0].author.as_ref().unwrap().books.len(), 2);
}

#[test]
fn test_prefix_merge() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
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
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let mut authors = repo.query::<Author>().all().unwrap();
    assert!(authors[0].books.is_empty());
    repo.load_relation(&mut authors[0], "books").unwrap();
    assert_eq!(authors[0].books.len(), 2);
    repo.load_relation(&mut authors[0], "books.comments").unwrap();
    assert_eq!(authors[0].books[0].comments.len(), 2);
}

#[test]
fn test_load_relation_unknown() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let mut authors = repo.query::<Author>().all().unwrap();
    assert!(repo.load_relation(&mut authors[0], "nope").is_err());
}
