mod common;

use std::io::ErrorKind;

use common::TestDb;
use nothings_orm::driver::NOrmDriver;
use nothings_orm::value::{NOrmDateTime, NOrmFromValue, NOrmValue};

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
                NOrmValue::NOrmDateTime(NOrmDateTime::parse("2026-01-02 03:04:05").unwrap()),
                NOrmValue::Text("12.34".into()),
                NOrmValue::Text("{\"a\":1}".into()),
                NOrmValue::Bool(true),
            ],
        )
        .unwrap();

    let rows = db
        .driver
        .query("SELECT dt, dec_col, js, flag FROM type_probe", &[])
        .unwrap();
    // DATETIME 统一读回 NOrmDateTime；DECIMAL / JSON 降级为 Text。
    assert_eq!(
        rows[0].values()[0],
        NOrmValue::NOrmDateTime(NOrmDateTime::parse("2026-01-02 03:04:05").unwrap())
    );
    assert!(matches!(rows[0].values()[1], NOrmValue::Text(_)));
    assert!(matches!(rows[0].values()[2], NOrmValue::Text(_)));
    // Bool 写入 TINYINT，读回 I64。
    assert_eq!(rows[0].values()[3], NOrmValue::I64(1));
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
                NOrmValue::Text("中文 Alice".into()),
                NOrmValue::Text("a@example.com".into()),
                NOrmValue::Bool(false),
                NOrmValue::Blob(bytes.clone()),
            ],
        )
        .unwrap();

    let rows = db
        .driver
        .query("SELECT name, email, active, payload FROM users", &[])
        .unwrap();
    assert_eq!(rows[0].values()[0], NOrmValue::Text("中文 Alice".into()));
    assert_eq!(rows[0].values()[3], NOrmValue::Blob(bytes));
}
