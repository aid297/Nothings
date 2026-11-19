//! WHERE 条件增强的 MySQL 集成测试：OR / 嵌套 / NOT / 比较运算。
//! 本文件遵循项目约定：强制连真库，缺 NOTHINGS_MYSQL_DSN 直接 panic，不静默跳过。

mod common;

use common::{TestDb, User};
use nothings_orm::condition::{NOrmCondition, NOrmOperator};
use nothings_orm::repository::NOrmRepository;

fn user_with(name: &str, email: Option<&str>, active: bool) -> User {
    User {
        id: 0,
        name: name.to_string(),
        email: email.map(String::from),
        active,
        payload: None,
    }
}

fn setup() -> TestDb {
    let mut db = TestDb::with_users();
    {
        let mut repo = NOrmRepository::new(&mut db.driver);
        for user in [
            user_with("alice", Some("a@example.com"), true),
            user_with("bob", None, false),
            user_with("carol", None, false),
            user_with("dave", Some("d@example.com"), true),
        ] {
            repo.insert(&user).unwrap();
        }
    }
    db
}

fn names(rows: &[User]) -> Vec<&str> {
    let mut result: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    result.sort();
    result
}

#[test]
fn test_or_basic() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
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
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    // 左折叠：(name = alice OR name = bob) AND active = true → 仅 alice
    let rows = repo
        .query::<User>()
        .r#where("name", "alice")
        .or_where("name", "bob")
        .r#where("active", true)
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice"]);
}

#[test]
fn test_comparison_and_not_like() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);

    let rows = repo
        .query::<User>()
        .where_op("id", NOrmOperator::Gt, 0i64)
        .all()
        .unwrap();
    assert_eq!(rows.len(), 4);

    let rows = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::not_like("name", "%o%"))
        .all()
        .unwrap();
    assert_eq!(names(&rows), ["alice", "dave"]);
}

#[test]
fn test_in_and_not_in() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);

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
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);

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
fn test_nested_condition_and_count() {
    let mut db = setup();
    let mut repo = NOrmRepository::new(&mut db.driver);
    // (name = alice OR name = bob) AND NOT (email IS NULL) → alice
    let cond = NOrmCondition::<User>::eq("name", "alice")
        .or(NOrmCondition::eq("name", "bob"))
        .and(!NOrmCondition::<User>::is_null("email"));
    let rows = repo.query::<User>().where_condition(cond).all().unwrap();
    assert_eq!(names(&rows), ["alice"]);

    let count = repo
        .query::<User>()
        .where_condition(NOrmCondition::<User>::in_list("name", ["alice", "dave"]))
        .count()
        .unwrap();
    assert_eq!(count, 2);
}
