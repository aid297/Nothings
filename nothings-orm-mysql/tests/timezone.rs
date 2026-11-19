use nothings_orm::driver::NOrmDriver;
use nothings_orm::value::{NOrmDateTime, NOrmFromValue, NOrmUtcOffset, NOrmValue};
use nothings_orm_mysql::NOrmMysqlDriver;

use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn dsn() -> String {
    std::env::var("NOTHINGS_MYSQL_DSN")
        .expect("必须设置 NOTHINGS_MYSQL_DSN；时区测试强制连接真实 MySQL")
        .trim_end_matches('/')
        .to_string()
}

struct TzDb {
    driver: NOrmMysqlDriver,
    base: String,
    db: String,
}

impl TzDb {
    fn new(orm_tz: NOrmUtcOffset, db_tz: NOrmUtcOffset) -> Self {
        let base = dsn();
        let db = format!(
            "nothings_tz_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        );
        let mut admin = NOrmMysqlDriver::connect(&base).unwrap();
        let _ = admin.execute_ddl(&format!("DROP DATABASE IF EXISTS `{db}`"));
        admin
            .execute_ddl(&format!("CREATE DATABASE `{db}`"))
            .unwrap();
        let driver = NOrmMysqlDriver::connect_with_tz(&format!("{base}/{db}"), orm_tz, db_tz).unwrap();
        Self { driver, base, db }
    }
}

impl Drop for TzDb {
    fn drop(&mut self) {
        if let Ok(mut admin) = NOrmMysqlDriver::connect(&self.base) {
            let _ = admin.execute_ddl(&format!("DROP DATABASE IF EXISTS `{}`", self.db));
        }
    }
}

fn dt(s: &str) -> NOrmDateTime {
    NOrmDateTime::parse(s).unwrap()
}

fn setup(db: &mut TzDb) {
    db.driver
        .execute_ddl(
            "CREATE TABLE events (
                id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                name VARCHAR(64) NOT NULL,
                at_dt DATETIME(6) NULL,
                at_ts TIMESTAMP NULL,
                seq BIGINT NULL
            )",
        )
        .unwrap();
}

#[test]
fn test_write_datetime_orm_utc_db_plus8() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse("+08:00").unwrap());
    setup(&mut db);
    db.driver
        .execute(
            "INSERT INTO events (name, at_dt) VALUES (?, ?)",
            &[NOrmValue::Text("a".into()), NOrmValue::NOrmDateTime(dt("2026-01-02 09:00:00"))],
        )
        .unwrap();

    // ORM 读回仍为 09:00（DB → ORM 反转换）
    let rows = db.driver.query("SELECT at_dt FROM events", &[]).unwrap();
    assert_eq!(
        rows[0].values()[0],
        NOrmValue::NOrmDateTime(dt("2026-01-02 09:00:00"))
    );

    // 裸 SQL 绕过驱动，看到的是库内墙钟 17:00
    let admin_url = format!("{}/{}", db.base, db.db);
    let mut raw = NOrmMysqlDriver::connect(&admin_url).unwrap();
    let raw_rows = raw.query("SELECT at_dt FROM events", &[]).unwrap();
    assert_eq!(
        NOrmDateTime::from_value(&raw_rows[0].values()[0]).unwrap(),
        dt("2026-01-02 17:00:00")
    );
}

#[test]
fn test_write_timestamp_and_roundtrip() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse("+08:00").unwrap());
    setup(&mut db);
    db.driver
        .execute(
            "INSERT INTO events (name, at_ts) VALUES (?, ?)",
            &[NOrmValue::Text("b".into()), NOrmValue::NOrmDateTime(dt("2026-01-02 09:00:00"))],
        )
        .unwrap();
    let rows = db.driver.query("SELECT at_ts FROM events", &[]).unwrap();
    assert_eq!(
        rows[0].values()[0],
        NOrmValue::NOrmDateTime(dt("2026-01-02 09:00:00"))
    );
}

#[test]
fn test_query_condition_uses_same_conversion() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse("+08:00").unwrap());
    setup(&mut db);
    db.driver
        .execute(
            "INSERT INTO events (name, at_ts) VALUES (?, ?)",
            &[NOrmValue::Text("c".into()), NOrmValue::NOrmDateTime(dt("2026-01-02 09:00:00"))],
        )
        .unwrap();

    // 查询条件传 ORM 时区的 09:00，应命中库内 17:00 对应的行
    let rows = db
        .driver
        .query(
            "SELECT name FROM events WHERE at_ts = ?",
            &[NOrmValue::NOrmDateTime(dt("2026-01-02 09:00:00"))],
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].values()[0], NOrmValue::Text("c".into()));
}

#[test]
fn test_timestamp_and_datetime_consistent() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse("+08:00").unwrap());
    setup(&mut db);
    let t = NOrmValue::NOrmDateTime(dt("2026-01-02 09:00:00"));
    db.driver
        .execute(
            "INSERT INTO events (name, at_dt, at_ts) VALUES (?, ?, ?)",
            &[NOrmValue::Text("d".into()), t.clone(), t],
        )
        .unwrap();
    let rows = db
        .driver
        .query("SELECT at_dt, at_ts FROM events", &[])
        .unwrap();
    assert_eq!(rows[0].values()[0], rows[0].values()[1]);
}

#[test]
fn test_negative_and_quarter_offsets() {
    for spec in ["-05:00", "+05:45", "-03:30"] {
        let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse(spec).unwrap());
        setup(&mut db);
        let expected = dt("2026-01-02 09:00:00");
        db.driver
            .execute(
                "INSERT INTO events (name, at_dt) VALUES (?, ?)",
                &[NOrmValue::Text("e".into()), NOrmValue::NOrmDateTime(expected.clone())],
            )
            .unwrap();
        let rows = db.driver.query("SELECT at_dt FROM events", &[]).unwrap();
        assert_eq!(rows[0].values()[0], NOrmValue::NOrmDateTime(expected), "offset {spec}");
    }
}

#[test]
fn test_i64_timestamp_untouched() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse("+08:00").unwrap());
    setup(&mut db);
    db.driver
        .execute(
            "INSERT INTO events (name, seq) VALUES (?, ?)",
            &[NOrmValue::Text("f".into()), NOrmValue::I64(1_700_000_000)],
        )
        .unwrap();
    let rows = db.driver.query("SELECT seq FROM events", &[]).unwrap();
    assert_eq!(rows[0].values()[0], NOrmValue::I64(1_700_000_000));
}

#[test]
fn test_session_timezone_and_bad_offset() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse("+08:00").unwrap());
    let actual = db
        .driver
        .query("SELECT @@session.time_zone", &[])
        .unwrap();
    assert_eq!(actual[0].values()[0], NOrmValue::Text("+08:00".into()));
}

#[test]
fn test_micros_roundtrip() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::parse("+08:00").unwrap());
    setup(&mut db);
    let t = dt("2026-01-02 09:00:00.123456");
    db.driver
        .execute(
            "INSERT INTO events (name, at_dt) VALUES (?, ?)",
            &[NOrmValue::Text("g".into()), NOrmValue::NOrmDateTime(t.clone())],
        )
        .unwrap();
    let rows = db.driver.query("SELECT at_dt FROM events", &[]).unwrap();
    assert_eq!(rows[0].values()[0], NOrmValue::NOrmDateTime(t));
}

#[test]
fn test_legacy_connect_reads_datetime() {
    let mut db = TzDb::new(NOrmUtcOffset::UTC, NOrmUtcOffset::UTC);
    setup(&mut db);
    db.driver
        .execute(
            "INSERT INTO events (name, at_dt) VALUES (?, ?)",
            &[NOrmValue::Text("h".into()), NOrmValue::NOrmDateTime(dt("2026-01-02 03:04:05"))],
        )
        .unwrap();
    let rows = db.driver.query("SELECT at_dt FROM events", &[]).unwrap();
    assert_eq!(
        rows[0].values()[0],
        NOrmValue::NOrmDateTime(dt("2026-01-02 03:04:05"))
    );
}

#[test]
fn test_invalid_offset_rejected() {
    assert!(NOrmUtcOffset::parse("bad").is_err());
    assert!(NOrmUtcOffset::parse("+99:00").is_err());
    assert!(NOrmUtcOffset::parse("+08:60").is_err());
}
