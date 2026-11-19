// 测试夹具被三个测试二进制分别编译，部分文件用不到 User / user。
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

use nothings_orm::NOrmEntity;
use nothings_orm_mysql::NOrmMysqlDriver;

static SEQ: AtomicUsize = AtomicUsize::new(0);

/// 集成测试强制连真实 MySQL：缺 DSN 直接 panic，不静默跳过。
pub fn base_dsn() -> String {
    std::env::var("NOTHINGS_MYSQL_DSN")
        .expect(
            "必须设置 NOTHINGS_MYSQL_DSN（如 mysql://root:pwd@127.0.0.1:3306）；MySQL 集成测试不允许跳过",
        )
        .trim_end_matches('/')
        .to_string()
}

#[derive(Debug, NOrmEntity)]
#[entity(table = "users")]
pub struct User {
    #[entity(primary_key, column = "id")]
    pub id: i64,
    #[entity(column = "name")]
    pub name: String,
    #[entity(column = "email")]
    pub email: Option<String>,
    #[entity(column = "active")]
    pub active: bool,
    #[entity(column = "payload")]
    pub payload: Option<Vec<u8>>,
}

pub fn user(name: &str) -> User {
    User {
        id: 0,
        name: name.to_string(),
        email: None,
        active: false,
        payload: None,
    }
}

/// 每个测试一个独立 database：`NOrmSchema::TABLE` 是编译期常量，无法动态改表名。
pub struct TestDb {
    pub driver: NOrmMysqlDriver,
    base: String,
    db: String,
}

impl TestDb {
    pub fn new() -> Self {
        let base = base_dsn();
        let db = format!(
            "nothings_orm_mysql_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        );
        let mut admin = NOrmMysqlDriver::connect(&base).unwrap();
        admin
            .execute_ddl(&format!("DROP DATABASE IF EXISTS `{db}`"))
            .unwrap();
        admin
            .execute_ddl(&format!("CREATE DATABASE `{db}`"))
            .unwrap();
        let driver = NOrmMysqlDriver::connect(&format!("{base}/{db}")).unwrap();
        Self { driver, base, db }
    }

    pub fn with_users() -> Self {
        let mut db = Self::new();
        db.create_users();
        db
    }

    pub fn create_users(&mut self) {
        self.driver
            .execute_ddl(
                "CREATE TABLE users (
                    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                    name VARCHAR(255) NOT NULL UNIQUE,
                    email VARCHAR(255) NULL,
                    active TINYINT NOT NULL DEFAULT 0,
                    payload BLOB NULL
                )",
            )
            .unwrap();
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if let Ok(mut admin) = NOrmMysqlDriver::connect(&self.base) {
            let _ = admin.execute_ddl(&format!("DROP DATABASE IF EXISTS `{}`", self.db));
        }
    }
}
