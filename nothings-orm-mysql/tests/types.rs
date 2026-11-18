mod common;

use std::io::ErrorKind;

use common::TestDb;
use nothings_orm::driver::Driver;
use nothings_orm::value::{DateTime, FromValue, Value};

#[test]
fn test_type_downgrade() {
    let mut db = TestDb::new();
    db.driver
        .execute_ddl(
            "CREATE TABLE type_probe (
                dt DATETIME,
                dec_col DECIMAL(10,2),
                js JSON,
                flag TINYINT
            )",
        )
        .unwrap();
    db.driver
        .execute(
            "INSERT INTO type_probe (dt, dec_col, js, flag) VALUES (?, ?, ?, ?)",
            &[
                Value::DateTime(DateTime::parse("2026-01-02 03:04:05").unwrap()),
                Value::Text("12.34".into()),
                Value::Text("{\"a\":1}".into()),
                Value::Bool(true),
            ],
        )
        .unwrap();

    let rows = db
        .driver
        .query("SELECT dt, dec_col, js, flag FROM type_probe", &[])
        .unwrap();
    // DATETIME 统一读回 DateTime；DECIMAL / JSON 降级为 Text。
    assert_eq!(
        rows[0].values()[0],
        Value::DateTime(DateTime::parse("2026-01-02 03:04:05").unwrap())
    );
    assert!(matches!(rows[0].values()[1], Value::Text(_)));
    assert!(matches!(rows[0].values()[2], Value::Text(_)));
    // Bool 写入 TINYINT，读回 I64。
    assert_eq!(rows[0].values()[3], Value::I64(1));
    assert!(bool::from_value(&rows[0].values()[3]).unwrap());
}

#[test]
fn test_uint_overflow() {
    let mut db = TestDb::new();
    let err = db
        .driver
        .query("SELECT CAST(18446744073709551615 AS UNSIGNED) AS huge", &[])
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);
}

#[test]
fn test_utf8_and_blob_roundtrip() {
    let mut db = TestDb::with_users();
    // 含 ASCII 但整体非法 UTF-8：不能用 UTF-8 试探来区分 Text/Blob。
    let bytes = vec![0xffu8, 0x00, 0xfe, 0x61];
    db.driver
        .execute(
            "INSERT INTO users (name, email, active, payload) VALUES (?, ?, ?, ?)",
            &[
                Value::Text("中文 Alice".into()),
                Value::Text("a@example.com".into()),
                Value::Bool(false),
                Value::Blob(bytes.clone()),
            ],
        )
        .unwrap();

    let rows = db
        .driver
        .query("SELECT name, email, active, payload FROM users", &[])
        .unwrap();
    assert_eq!(rows[0].values()[0], Value::Text("中文 Alice".into()));
    assert_eq!(rows[0].values()[3], Value::Blob(bytes));
}
