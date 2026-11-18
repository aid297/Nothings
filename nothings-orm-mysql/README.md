# nothings-orm-mysql — MySQL 驱动

对标 Go 的 `gorm.io/driver/mysql`：实现 `nothings_orm::Driver`，把 MySQL 接到 ORM 核心上。

- 版本：0.0.1
- edition：2024
- MSRV：1.89+（2026-11-18 用 `cargo +1.89.0 check` 实测通过，`mysql 28.0.4` 及全部依赖在 1.89.0 下编译无误；详见 `docs/ai/spec/51-mysql-driver-design.md`）
- License：MIT

## 依赖取舍

`mysql = { version = "28.0", default-features = false, features = ["minimal-rust"] }`

- 选同步 `mysql` crate：`Driver` 是同步 trait（`&mut self`），不需要 tokio runtime；`mysql_async` / `sqlx` 只会引入 `block_on` 与额外依赖。
- `default-features = false` 是硬要求：默认 `minimal` 带 `flate2/zlib`（C 依赖），TLS feature 会拉 native-tls / OpenSSL。`minimal-rust` 走纯 Rust `miniz_oxide`，实测依赖树里没有 native-tls / openssl / zlib-sys / tokio。
- `mysql_common` 是传递依赖，统一用 `mysql::consts::*` 与 `mysql::{Value, Column, Params, Conn, ...}` 重导出，不显式声明，避免跨版本类型分叉。
- `crate-type = ["lib"]`。**零改动 `nothings-orm`**：MySQL 原生支持 `?` 占位符、`LAST_INSERT_ID()`、`START TRANSACTION`，核心、宏、SQLite 驱动均不受影响。

## 用法

```rust
use nothings_orm::repository::Repository;
use nothings_orm::value::Value;
use nothings_orm_mysql::MysqlDriver;

let dsn = "mysql://root:pwd@127.0.0.1:3306/app";
let mut driver = MysqlDriver::connect(dsn).unwrap();
// 需要双时区自动转换时用：
// let orm_tz = UtcOffset::parse("+08:00").unwrap();   // 业务墙钟时区
// let db_tz = UtcOffset::UTC;                          // 库中时间表达的时区
// let mut driver = MysqlDriver::connect_with_tz(dsn, orm_tz, db_tz).unwrap();
driver.execute_ddl(
    "CREATE TABLE users (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255))"
).unwrap();

let mut repo = Repository::new(&mut driver);
// repo.insert::<User>(&user)?;
// let user = repo.get::<User>(Value::I64(1))?;
// let count = repo.query::<User>().count()?;
```

`execute_ddl` 只接固定字面量的 DDL，不参数化，**绝不传用户输入**。主键必须 `AUTO_INCREMENT`：`Repository::insert` 会排除主键列，并靠 `last_insert_id()` 回读。

### 测试

集成测试强制连真实 MySQL，缺 `NOTHINGS_MYSQL_DSN` 直接失败（不跳过）。DSN 不带 database，账号需有 `CREATE/DROP DATABASE` 权限；每个测试建独立 database，结束时自动清理。

```bash
docker run -d --name mysql8 -e MYSQL_ROOT_PASSWORD=<pwd> -p 3306:3306 mysql:8.0
export NOTHINGS_MYSQL_DSN='mysql://root:<pwd>@127.0.0.1:3306'
cargo test --manifest-path nothings-orm-mysql/Cargo.toml
```

## 错误映射

`mysql::Error` → `std::io::Error`（orphan rule 不允许 `impl From`，用固有函数 `to_io`）：

| MySQL 来源 | ErrorKind |
| --- | --- |
| 1062 `ER_DUP_ENTRY` | `AlreadyExists` |
| 1049 / 1146 库表不存在 | `NotFound` |
| 1054 未知列 | `InvalidInput` |
| 1064 语法错误 | `InvalidData` |
| 1045 认证失败 | `PermissionDenied` |
| 连接 URL 非法、占位符/参数不匹配 | `InvalidInput` |
| 非法 UTF-8、UInt 溢出、编解码失败 | `InvalidData` |
| `IoError` | 保留原 `io.kind()` |
| 其它服务端码 / 连接失败 | `Other` |

## 已知边界

- core 裸拼接标识符、不加反引号：`E::TABLE` / `E::COLUMNS` **不得使用 MySQL 保留字**（`order` / `key` / `group` / `desc` / `rank` …），否则 1064。DDL 列名与 `#[entity(column = ...)]` 必须字面一致（宏按列名大小写敏感精确匹配）。
- **`QueryBuilder::offset()` 必须搭配 `limit()`**：单独调用会生成 `SELECT ... OFFSET n`，MySQL 报 1064（SQLite 允许）。`one()` 自带 `limit(1)`，`get` / `find_one` 不受影响。
- 类型降级：`Value` 有七种变体（新增 `DateTime`）。DATETIME / DATE / TIME / DECIMAL / JSON 中，只有 TIMESTAMP / DATETIME 列读回 `Value::DateTime`，其余 → `Text`（无类型保真）；`Bool` 写为 TINYINT 0/1、读回 `I64`；`BIGINT UNSIGNED > i64::MAX` → `InvalidData`。Text / Blob 靠列元数据（`ColumnType` + `BINARY_FLAG` + charset）判定，不靠 UTF-8 试探。
- 时区自动转换只对 `Value::DateTime` 生效；`Text` / `I64` 不转。`connect_with_tz` 通过 `OptsBuilder::init` 下发 `SET time_zone`（DSN 不支持 timezone 参数），连接后校验会话时区。固定偏移无 DST，不支持命名时区；SQLite 驱动不做时区转换。所有时间列读回 `Value::DateTime`，`String` 字段会失败，须改用 `DateTime` 字段。
- `execute_ddl` 不参数化。实测 `mysql 28.0.4` 的 `query_drop` **允许分号拼接多语句**（客户端开启 `CLIENT_MULTI_STATEMENTS`），驱动不主动拒绝；因此它只能用于固定字面量 DDL。DDL 会隐式提交，事务内只做 DML。
- 一个 `MysqlDriver` 持一条连接，无连接池、非 `Sync`；并发每线程一个实例。
- `query` 全量物化为 `OwnedRow`，大结果集内存线性增长；`COUNT(*) AS __count` 的列名 / 别名原样保留，不可改写。
