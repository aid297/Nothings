//! WHERE 条件增强的 SQLite 集成测试：OR / 嵌套 / NOT / 比较运算。

#![allow(dead_code)]

use nothings_orm::condition::{NOrmCondition, NOrmOperator};
use nothings_orm::repository::NOrmRepository;
use nothings_orm::NOrmEntity;
use nothings_orm_sqlite::NOrmSqliteDriver;

#[derive(Debug, NOrmEntity)]
#[entity(table = "cond_users")]
struct User {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "name")]
    name: String,
    #[entity(column = "age")]
    age: i64,
    #[entity(nullable, column = "email")]
    email: Option<String>,
}

fn setup() -> NOrmSqliteDriver {
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver
        .execute_batch(
            "CREATE TABLE cond_users (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                age INTEGER NOT NULL,
                email TEXT
            )",
        )
        .unwrap();
    let mut repo = NOrmRepository::new(&mut driver);
    for (name, age, email) in [
        ("alice", 30, Some("a@example.com")),
        ("bob", 12, None),
        ("carol", 25, None),
        ("dave", 40, Some("d@example.com")),
    ] {
        repo.insert(&User {
            id: 0,
            name: name.into(),
            age,
            email: email.map(String::from),
        })
        .unwrap();
    }
    driver
}

fn names(rows: &[User]) -> Vec<&str> {
    let mut result: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    result.sort();
    result
}

#[test]
fn test_or_basic() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let rows = repo
        .query::<User>()
        .r#where("name", "alice")
        .or_where("name", "bob")
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice", "bob"]);
}

#[test]
fn test_and_or_left_assoc() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    // 左折叠：(name = alice OR name = bob) AND age = 30 → 仅 alice
    let rows = repo
        .query::<User>()
        .r#where("name", "alice")
        .or_where("name", "bob")
        .r#where("age", 30i64)
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice"]);
}

#[test]
fn test_not_where() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let rows = repo
        .query::<User>()
        .not_where("age", 12i64)
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice", "carol", "dave"]);
}

#[test]
fn test_comparison_ops() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);

    let rows = repo
        .query::<User>()
        .where_op("age", NOrmOperator::Ge, 25i64)
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice", "carol", "dave"]);

    let rows = repo
        .query::<User>()
        .where_op("age", NOrmOperator::Lt, 25i64)
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["bob"]);
}

#[test]
fn test_in_and_not_in() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);

    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::in_list("name", ["alice", "dave"]))
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice", "dave"]);

    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::not_in_list("name", ["alice", "dave"]))
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["bob", "carol"]);
}

#[test]
fn test_null_checks() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);

    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::is_null("email"))
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["bob", "carol"]);

    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::is_not_null("email"))
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice", "dave"]);
}

#[test]
fn test_like_and_not_like() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);

    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::like("name", "%o%"))
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["bob", "carol"]);

    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::not_like("name", "%o%"))
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice", "dave"]);
}

#[test]
fn test_nested_condition_and_not() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    // (age >= 25 OR name = bob) AND NOT (email IS NULL) → alice, dave
    let cond = NOrmCondition::<User>::ge("age", 25i64)
        .or(NOrmCondition::eq("name", "bob"))
        .and(!NOrmCondition::<User>::is_null("email"));
    let rows = repo.query::<User>().where_condition(cond).all().unwrap();
    assert_eq!(names(&rows), ["alice", "dave"]);
}

#[test]
fn test_count_with_condition() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let cond = NOrmCondition::<User>::ge("age", 25i64).or(NOrmCondition::eq("name", "bob"));
    let count = repo.query::<User>().where_condition(cond).count().unwrap();
    assert_eq!(count, 4);

    let count = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::in_list("name", ["alice", "dave"]))
        .count()
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn test_empty_in_returns_no_rows() {
    let mut driver = setup();
    let mut repo = NOrmRepository::new(&mut driver);
    let empty: [&str; 0] = [];
    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::in_list("name", empty))
        .all()
        .unwrap();
    assert!(rows.is_empty());
}
