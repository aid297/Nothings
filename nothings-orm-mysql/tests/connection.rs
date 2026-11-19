mod common;

use std::io::ErrorKind;

use common::TestDb;
use nothings_orm::driver::NOrmDriver;
use nothings_orm::value::NOrmValue;
use nothings_orm_mysql::NOrmMysqlDriver;

#[test]
fn test_connect_failure() {
    let err = NOrmMysqlDriver::connect("mysql://root:definitely_wrong@127.0.0.1:1/nope")
        .err()
        .expect("端口 1 上不该有 MySQL 服务");
    assert!(matches!(
        err.kind(),
        ErrorKind::Other
            | ErrorKind::PermissionDenied
            | ErrorKind::NotFound
            | ErrorKind::ConnectionRefused
            | ErrorKind::TimedOut
            | ErrorKind::InvalidInput
    ));
    assert!(!err.to_string().is_empty());
}

#[test]
fn test_execute_ddl() {
    let mut db = TestDb::new();
    db.driver.execute_ddl("CREATE TABLE ddl_probe (id INT)").unwrap();
    // 重复建表返回错误。
    assert!(db
        .driver
        .execute_ddl("CREATE TABLE ddl_probe (id INT)")
        .is_err());
    let rows = db
        .driver
        .query(
            "SELECT COUNT(*) AS __count FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_name = 'ddl_probe'",
            &[],
        )
        .unwrap();
    assert_eq!(rows[0].values()[0], NOrmValue::I64(1));
    db.driver.execute_ddl("DROP TABLE ddl_probe").unwrap();
}

#[test]
fn test_multi_statement_behavior() {
    // 实测偏差：mysql 28.0.4 的 query_drop 允许分号拼接多语句（客户端开启
    // CLIENT_MULTI_STATEMENTS），与「默认禁用」的假设相反。execute_ddl 因此
    // 不主动拒绝多语句，必须只用于固定字面量 DDL、绝不接用户输入。
    let mut db = TestDb::new();
    db.driver
        .execute_ddl("CREATE TABLE ms_a (id INT); CREATE TABLE ms_b (id INT)")
        .unwrap();
    let rows = db
        .driver
        .query(
            "SELECT COUNT(*) AS __count FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_name IN ('ms_a', 'ms_b')",
            &[],
        )
        .unwrap();
    assert_eq!(rows[0].values()[0], NOrmValue::I64(2));
}
