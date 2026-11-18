# ORM 完整演示：nothings-orm + SQLite / MySQL

一份可以直接抄的实战文档。覆盖从依赖配置到高级查询、事务、自定义驱动的**全部公开能力**。所有示例都经过真实编译与运行验证（SQLite 内存库 + MySQL 8.0.46）。

三个 crate 各司其职：

| crate | 角色 | 是否必需 |
| --- | --- | --- |
| `nothings-orm` | 核心：`Entity` / `Schema` / `Driver` / `Repository` / `QueryBuilder`，零 DB 依赖 | 必需 |
| `nothings-orm-sqlite` | SQLite 驱动（rusqlite bundled，纯 C 内置） | 二选一 |
| `nothings-orm-mysql` | MySQL 驱动（同步 `mysql` crate，`minimal-rust` 纯 Rust） | 二选一 |

设计背景见 [`ai/spec/50-orm-design.md`](ai/spec/50-orm-design.md) 与 [`ai/spec/51-mysql-driver-design.md`](ai/spec/51-mysql-driver-design.md)。

---

## 目录

1. [依赖配置](#1-依赖配置)
2. [定义实体](#2-定义实体)
3. [Schema 元数据](#3-schema-元数据)
4. [建表与连接](#4-建表与连接)
5. [CRUD：增删改查](#5-crud增删改查)
6. [QueryBuilder：条件 / 排序 / 分页 / 计数](#6-querybuilder条件--排序--分页--计数)
7. [事务](#7-事务)
8. [错误处理](#8-错误处理)
9. [值类型与映射](#9-值类型与映射)
10. [直接使用 Driver 与 Row](#10-直接使用-driver-与-row)
11. [两个驱动的差异](#11-两个驱动的差异)
12. [完整可运行示例](#12-完整可运行示例)
13. [自定义一个 Driver](#13-自定义一个-driver)
14. [已知边界与常见坑](#14-已知边界与常见坑)
15. [能力清单：不做的事](#15-能力清单不做的事)

---

## 1. 依赖配置

按需选驱动，两个都要也行。MySQL 驱动**不会**把 C 依赖或 tokio 带进你的构建。

```toml
[dependencies]
nothings-orm = "0.0.1"          # 核心，必需

# 二选一或两个都要
nothings-orm-sqlite = "0.0.1"   # SQLite
nothings-orm-mysql = "0.0.1"    # MySQL
```

MSRV：Rust 1.89+（跟随主仓库）。edition 2024。

如果只是在本仓库里跑示例，用路径依赖：

```toml
[dependencies]
nothings-orm = { path = "../nothings-orm" }
nothings-orm-sqlite = { path = "../nothings-orm-sqlite" }
nothings-orm-mysql = { path = "../nothings-orm-mysql" }
```

---

## 2. 定义实体

用 `#[derive(Entity)]` 声明一张表。**这是唯一需要写的结构性代码**——Schema、行解码、按列名取值全部由宏生成。

```rust
use nothings_orm::Entity;

#[derive(Debug, Clone, PartialEq, Entity)]
#[entity(table = "users")]
struct User {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "user_name")]
    name: String,
    #[entity(nullable)]
    email: Option<String>,
    #[entity(column = "active")]
    active: bool,
}
```

属性语法：

| 位置 | 属性 | 必填 | 说明 |
| --- | --- | --- | --- |
| 结构体 | `#[entity(table = "users")]` | 是 | 表名，原样拼进 SQL |
| 字段 | `#[entity(primary_key)]` | 是 | **必须有且只有一个**主键 |
| 字段 | `#[entity(column = "user_name")]` | 否 | DB 列名；不写则用 Rust 字段名 |
| 字段 | `#[entity(nullable)]` | 否 | 可空；`Option<T>` 会自动推断，无需手写 |

支持的字段类型：`i8/i16/i32/i64/u8/u16/u32`、`f32/f64`、`bool`、`String`、`Vec<u8>`、`DateTime`、`Option<T>`（T 为上述类型）。**不支持** `u64`、`&str`、Decimal、JSON 作为实体字段类型（见第 9 节）。时间列请用 `DateTime`，不要用 `String`（读回会得到 `Value::DateTime`，`String` 会报 `InvalidData`）。

> ⚠️ `entity` 属性里写的 `column` 是 **DB 列名**。后面所有 `where` / `order_by` 传的都是这个名字，不是 Rust 字段名。上面 `name` 字段对应 `user_name`，写 `r#where("name", ...)` 会得到 `InvalidInput`。这是最容易踩的坑。

主键设计假设：一个整数自增主键。`Repository::insert` 会把主键列从 INSERT 里排除，靠 `last_insert_id()` 回读。DDL 必须写成 `INTEGER PRIMARY KEY`（SQLite）或 `BIGINT AUTO_INCREMENT PRIMARY KEY`（MySQL）。

---

## 3. Schema 元数据

派生宏会为实体生成 `Schema` 实现，可以在编译期拿到表结构：

```rust
use nothings_orm::schema::Schema;

assert_eq!(User::TABLE, "users");
assert_eq!(User::COLUMNS.len(), 4);
assert_eq!(User::pk().name, "id");

for col in User::COLUMNS {
    println!("{} -> {} ({:?}), pk={}, nullable={}",
        col.field, col.name, col.sql_type, col.primary_key, col.nullable);
}
```

`Column` 字段：`field`（Rust 字段名）、`name`（DB 列名）、`sql_type`（`Integer`/`Real`/`Text`/`Blob`）、`nullable`、`primary_key`。

`validate_column::<User>("user_name")` 会做白名单校验，非法列名返回 `InvalidInput`。

---

## 4. 建表与连接

核心库不生成 DDL——建表由驱动提供原始 SQL 入口。

**SQLite**（内存库，测试首选）：

```rust
use nothings_orm_sqlite::SqliteDriver;

let mut driver = SqliteDriver::in_memory().unwrap();
driver.execute_batch(
    "CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        user_name TEXT NOT NULL UNIQUE,
        email TEXT,
        active INTEGER NOT NULL DEFAULT 0
    )",
).unwrap();

// 持久化到文件
// let mut driver = SqliteDriver::open("app.db").unwrap();
```

**MySQL**：

```rust
use nothings_orm_mysql::MysqlDriver;

let mut driver = MysqlDriver::connect("mysql://root:pwd@127.0.0.1:3306/app").unwrap();
driver.execute_ddl(
    "CREATE TABLE users (
        id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
        user_name VARCHAR(255) NOT NULL UNIQUE,
        email VARCHAR(255) NULL,
        active TINYINT NOT NULL DEFAULT 0
    )",
).unwrap();
```

两个 DDL 入口都**不参数化、不接用户输入、只执行原始 SQL**。MySQL 的 `execute_ddl` 实测允许分号拼接多语句，更要只传固定字面量。DDL 会隐式提交，不要放进事务块里。

---

## 5. CRUD：增删改查

全部入口是 `Repository::new(&mut driver)`。它持有驱动的可变借用，因此同一时刻只能有一个 repo；需要直接用 driver 时，让 repo 的作用域先结束。

```rust
use nothings_orm::repository::Repository;
use nothings_orm::value::Value;

let mut repo = Repository::new(&mut driver);

// 插入 —— 返回自增主键，实体里的 id 会被忽略
let alice = User {
    id: 0,
    name: "alice".into(),
    email: Some("a@example.com".into()),
    active: true,
};
let id = repo.insert(&alice).unwrap();
assert_eq!(id, 1);	

// 按主键取 —— 找不到返回 Err(NotFound)
let got: User = repo.get(Value::I64(id)).unwrap();

// 按主键取 —— 找不到返回 Ok(None)，不算错误
let maybe = repo.find_one::<User>(Value::I64(999)).unwrap();
assert!(maybe.is_none());

// 取全部
let all = repo.find_all::<User>().unwrap();

// 更新 —— 按主键更新所有非主键列，返回受影响行数
let mut updated = got;
updated.email = Some("new@example.com".into());
assert_eq!(repo.update(&updated).unwrap(), 1);

// 删除 —— 按主键删，返回受影响行数
assert_eq!(repo.delete::<User>(Value::I64(id)).unwrap(), 1);
```

方法清单：

| 方法 | 签名要点 | 返回 |
| --- | --- | --- |
| `insert::<E>` | `&E` | `Result<i64>` 新主键 |
| `get::<E>` | `Value` 主键 | `Result<E>`，未找到 `NotFound` |
| `find_one::<E>` | `Value` 主键 | `Result<Option<E>>` |
| `find_all::<E>` | 无 | `Result<Vec<E>>` |
| `update::<E>` | `&E` | `Result<usize>` 受影响行数 |
| `delete::<E>` | `Value` 主键 | `Result<usize>` 受影响行数 |

`Value` 是运行时列值：`Null` / `Bool` / `I64` / `F64` / `Text` / `Blob`。主键传 `Value::I64(id)`。

Trait 方法 `from_row` / `pk_value` / `column_value` 需要显式导入 trait，因为 crate 根的同名 `Entity` 是派生宏：

```rust
use nothings_orm::entity::Entity as EntityTrait;

assert_eq!(<User as EntityTrait>::pk_value(&got), Value::I64(1));
assert_eq!(
    <User as EntityTrait>::column_value(&got, "user_name").unwrap(),
    Value::Text("alice".into()),
);
```

---

## 6. QueryBuilder：条件 / 排序 / 分页 / 计数

`repo.query::<E>()` 返回链式构建器，方法返回 `&mut Self`。

```rust
// 等值 WHERE + 排序
let rows = repo
    .query::<User>()
    .r#where("active", true)
    .order_by("id", false)   // false = DESC
    .all()
    .unwrap();

// 取一条（内部自动 limit(1)）
let one = repo
    .query::<User>()
    .r#where("user_name", "alice")
    .one()
    .unwrap();

// 分页 —— MySQL 下 offset() 必须搭配 limit()
let page = repo
    .query::<User>()
    .order_by("id", true)
    .limit(10)
    .offset(20)
    .all()
    .unwrap();

// 计数
let n = repo.query::<User>().count().unwrap();
```

- 多个 `r#where` 用 `AND` 串联，**只支持等值**，没有 OR / LIKE / IN / 比较运算。
- `r#where` 的第二个参数接受任何 `ToValue`（含 `&str` / `String` / 数字 / `bool`）。
- `order_by(column, asc)` 的 `asc` 为 `true` 升序、`false` 降序。
- 列名必须在 `E::COLUMNS` 的 **DB 列名**白名单内。非法列名不会在构造时报错，而是延后到 `all()` / `one()` / `count()` 抛 `InvalidInput`：

```rust
let err = repo
    .query::<User>()
    .r#where("user_name; DROP TABLE users", "x")
    .all()
    .unwrap_err();
assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
// 表还在，SQL 根本没生成
```

---

## 7. 事务

单层事务，闭包返回 `Err` 自动回滚；嵌套直接拒绝；没有 savepoint。

```rust
// 提交
repo.transaction(|r| {
    r.insert(&User { id: 0, name: "committed".into(), email: None, active: false })?;
    Ok(())
}).unwrap();

// 回滚：闭包返回 Err，驱动自动 rollback，原始错误优先
let result: Result<(), std::io::Error> = repo.transaction(|r| {
    r.insert(&User { id: 0, name: "rolled_back".into(), email: None, active: false })?;
    Err(std::io::Error::other("boom"))
});
assert!(result.is_err());

// 嵌套事务 → InvalidInput
let nested = repo.transaction(|r| r.transaction(|_| Ok(()))).unwrap_err();
assert_eq!(nested.kind(), ErrorKind::InvalidInput);
```

事务里**只做 DML**。SQLite 的 DDL 和 MySQL 的 DDL 都会隐式提交，破坏事务边界。

---

## 8. 错误处理

所有公开 API 返回 `Result<_, std::io::Error>`，按 `ErrorKind` 分支，没有自定义错误类型。

| ErrorKind | 场景 |
| --- | --- |
| `NotFound` | `get` 未找到主键；驱动打开不存在的数据库/库表 |
| `AlreadyExists` | UNIQUE / 主键冲突（MySQL 1062） |
| `InvalidData` | 类型转换失败、缺列、非法 UTF-8、COUNT 别名缺失、MySQL 1064 语法错误、UInt 溢出 |
| `InvalidInput` | 列名不在白名单、嵌套事务、未知列、非法 DSN、参数个数不匹配 |
| `PermissionDenied` | MySQL 1045 认证失败 |
| 其它 | 保留 `std::io::Error` 原有的 kind（如连接 `ConnectionRefused`） |

```rust
match repo.get::<User>(Value::I64(999)) {
    Ok(u) => println!("找到 {u:?}"),
    Err(e) if e.kind() == ErrorKind::NotFound => println!("没有这个用户"),
    Err(e) => eprintln!("其它错误: {e}"),
}
```

---

## 9. 值类型与映射

### 支持的 Rust 类型

| Rust 类型 | `SqlType` | 说明 |
| --- | --- | --- |
| `i8`/`i16`/`i32`/`i64`/`u8`/`u16`/`u32` | `Integer` | 统一走 `Value::I64` |
| `f32`/`f64` | `Real` | 统一走 `Value::F64` |
| `bool` | `Integer` | 0/1 |
| `String` | `Text` | |
| `Vec<u8>` | `Blob` | |
| `DateTime` | `Text` | 墙钟时间，参与 MySQL 双时区自动转换 |
| `Option<T>` | 同 T | `None` ↔ `Value::Null` |

`&str` 实现了 `ToValue`（可直接当查询参数），但**不能作实体字段**（没有 `FromValue` / `HasSqlType`）。

缺失：`u64`、`i128`、`usize`、Decimal、UUID、JSON 的原生支持。日期时间用 `DateTime`（见 9.1），或 `i64` Unix 时间戳。

### MySQL 特有类型的降级

`Value` 只有六种变体，MySQL 的其它类型按下面规则降级（无类型保真）：

| MySQL 类型 | 映射为 | 说明 |
| --- | --- | --- |
| `DATETIME` / `DATE` / `TIME` / `TIMESTAMP` | `Value::Text` | 格式化字符串，如 `2026-01-02 03:04:05` |
| `DECIMAL` / `NEWDECIMAL` | `Value::Text` | 字符串，避免精度丢失 |
| `JSON` | `Value::Text` | 原始 JSON 文本 |
| `TINYINT` | `Value::I64` | `bool::from_value` 会把非 0 当 true |
| `BIGINT UNSIGNED` ≤ `i64::MAX` | `Value::I64` | |
| `BIGINT UNSIGNED` > `i64::MAX` | `Err(InvalidData)` | 放不下就报错，不静默截断 |

Text 与 Blob 的区分**靠列元数据**（`ColumnType` + `BINARY_FLAG` + charset），不靠 UTF-8 试探——BLOB 内容可能恰好是合法 UTF-8，而 JSON/DECIMAL 自带 binary charset。

### 9.1 时区与日期时间（MySQL）

core 提供零依赖的 `UtcOffset`（固定偏移）与 `DateTime`（墙钟）。**自动转换只对 `Value::DateTime` 生效**，`Text` / `I64` 一律不转——避免误伤 DECIMAL / JSON / 普通字符串。

```rust
use nothings_orm::value::{DateTime, UtcOffset};
use nothings_orm_mysql::MysqlDriver;

// 旧入口：UTC/UTC，不做转换（既有代码零改动）
let mut driver = MysqlDriver::connect(dsn)?;

// 双时区：ORM=东八区墙钟，DB 中时间按 UTC 表达
let orm_tz = UtcOffset::parse("+08:00")?;
let db_tz = UtcOffset::UTC;
let mut driver = MysqlDriver::connect_with_tz(dsn, orm_tz, db_tz)?;
```

语义：

- **写入**：`Value::DateTime(09:00 +08:00)` → `01:00 UTC` 写库。查询条件 `r#where("at", dt)` 走**同一条路径**，无需额外处理。
- **读取**：`TIMESTAMP` / `DATETIME` 列 → 按 DB→ORM 换算 → `Value::DateTime`。`DATE` / `TIME` / `YEAR` → `Text`（无完整墙钟）。
- **语义统一**：所有时间列读回 `Value::DateTime`，不分入口。`String` 字段会读到 `InvalidData`，**必须改用 `DateTime` 字段**。
- `i64` Unix 时间戳不受影响，原样存取。
- 会话时区通过 `OptsBuilder::init` 下发 `SET time_zone`（DSN 不接受 `?timezone=`），连接后校验；`init` 在连接重置时自动重放。
- 固定偏移**不含 DST**，不支持 `Asia/Shanghai` 这类命名时区的自动夏令时。
- **SQLite 驱动不做时区转换**：`DateTime` 按墙钟原样写 TEXT，读回走 `FromValue` 的 Text 解析兜底。

微秒需要列声明为 `DATETIME(6)` / `TIMESTAMP(6)`，MySQL 默认精度是 0 位小数。外部程序绕过 ORM 直写 DATETIME 字面量的数据，不保证转换语义正确。

---

## 10. 直接使用 Driver 与 Row

绕过 Repository，直接发参数化 SQL。`Driver` trait 需要显式导入才能调方法。

```rust
use nothings_orm::driver::Driver;
use nothings_orm::row::Row;
use nothings_orm::value::Value;

let rows = driver.query("SELECT id, user_name FROM users", &[]).unwrap();
assert_eq!(Row::column_name(&rows[0], 1), "user_name");
assert_eq!(Row::value(&rows[0], 1).unwrap(), Value::Text("alice".into()));

let affected = driver
    .execute(
        "UPDATE users SET active = ? WHERE id = ?",
        &[Value::Bool(false), Value::I64(1)],
    )
    .unwrap();

let last_id = driver.last_insert_id().unwrap();
```

`Driver` 六个方法：

| 方法 | 说明 |
| --- | --- |
| `execute(&mut self, sql, &[Value]) -> Result<usize>` | 非查询，返回受影响行数 |
| `query(&mut self, sql, &[Value]) -> Result<Vec<OwnedRow>>` | 查询，**全量物化**为 owned 行 |
| `last_insert_id(&self) -> Result<i64>` | 最近插入主键 |
| `begin` / `commit` / `rollback(&mut self)` | 事务 |

`OwnedRow` 提供 `names()` / `values()`，也实现 `Row` trait。驱动必须保证 `query` 返回的列名与实体 `#[entity(column = ...)]` **大小写敏感精确匹配**。

手工解码一行：

```rust
use nothings_orm::entity::Entity as EntityTrait;

let rows = driver.query("SELECT id, user_name, email, active FROM users", &[]).unwrap();
let u = <User as EntityTrait>::from_row(&rows[0]).unwrap();
```

---

## 11. 两个驱动的差异

| 项 | `SqliteDriver` | `MysqlDriver` |
| --- | --- | --- |
| 构造 | `open(path)` / `in_memory()` | `connect(url)` |
| DDL 入口 | `execute_batch(sql)` | `execute_ddl(sql)` |
| 主键 DDL | `INTEGER PRIMARY KEY` | `BIGINT AUTO_INCREMENT PRIMARY KEY` |
| 占位符 | `?` | `?`（原生兼容） |
| 事务语句 | `BEGIN` / `COMMIT` / `ROLLBACK` | `START TRANSACTION` / `COMMIT` / `ROLLBACK` |
| 多语句 | 支持 | 实测也支持（客户端开启 `CLIENT_MULTI_STATEMENTS`），但不承诺 |
| 连接 | 一条连接，非 `Sync` | 一条连接，非 `Sync` |
| 连接字符串 | 文件路径 | `mysql://user:pass@host:port/db` |
| 时区自动转换 | **不支持**（按墙钟原样写 TEXT） | `connect_with_tz(orm_tz, db_tz)` 双向转换，仅对 `Value::DateTime` 生效 |
| 测试 | 内存库，零外部依赖 | **必须连真实 MySQL**，缺 `NOTHINGS_MYSQL_DSN` 直接失败 |

MySQL 集成测试的 DSN 不带 database，账号需有 `CREATE/DROP DATABASE` 权限：

```bash
export NOTHINGS_MYSQL_DSN='mysql://root:pwd@127.0.0.1:3306'
cargo test --manifest-path nothings-orm-mysql/Cargo.toml
```

---

## 12. 完整可运行示例

### SQLite 版（零外部依赖）

```rust
use std::io::ErrorKind;

use nothings_orm::Entity;
use nothings_orm::entity::Entity as EntityTrait;
use nothings_orm::repository::Repository;
use nothings_orm::value::Value;
use nothings_orm_sqlite::SqliteDriver;

#[derive(Debug, Clone, PartialEq, Entity)]
#[entity(table = "users")]
struct User {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "user_name")]
    name: String,
    #[entity(nullable)]
    email: Option<String>,
    #[entity(column = "active")]
    active: bool,
}

fn main() {
    let mut driver = SqliteDriver::in_memory().unwrap();
    driver.execute_batch(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            user_name TEXT NOT NULL UNIQUE,
            email TEXT,
            active INTEGER NOT NULL DEFAULT 0
        )",
    ).unwrap();

    // 插入 + 回读主键
    let mut repo = Repository::new(&mut driver);
    let id = repo.insert(&User {
        id: 0,
        name: "alice".into(),
        email: Some("a@example.com".into()),
        active: true,
    }).unwrap();
    assert_eq!(id, 1);

    let got: User = repo.get(Value::I64(id)).unwrap();
    assert_eq!(<User as EntityTrait>::pk_value(&got), Value::I64(1));

    // 查询构建
    let rows = repo.query::<User>()
        .r#where("active", true)
        .order_by("id", false)
        .all()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(repo.query::<User>().count().unwrap(), 1);

    // 注入防护
    let bad = repo.query::<User>()
        .r#where("user_name; DROP TABLE users", "x")
        .all()
        .unwrap_err();
    assert_eq!(bad.kind(), ErrorKind::InvalidInput);
    assert_eq!(repo.find_all::<User>().unwrap().len(), 1); // 表还在

    // 事务
    repo.transaction(|r| {
        r.insert(&User { id: 0, name: "bob".into(), email: None, active: false })?;
        Ok(())
    }).unwrap();
    assert_eq!(repo.find_all::<User>().unwrap().len(), 2);
    println!("done");
}
```

### MySQL 版

把 `SqliteDriver::in_memory()` 换成 `MysqlDriver::connect(&dsn)`、`execute_batch` 换成 `execute_ddl`，DDL 换成 MySQL 方言，其余代码**完全不变**：

```rust
use nothings_orm_mysql::MysqlDriver;

let mut driver = MysqlDriver::connect("mysql://root:pwd@127.0.0.1:3306/app").unwrap();
driver.execute_ddl(
    "CREATE TABLE users (
        id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
        user_name VARCHAR(255) NOT NULL UNIQUE,
        email VARCHAR(255) NULL,
        active TINYINT NOT NULL DEFAULT 0
    )",
).unwrap();
// 后续 Repository 代码与 SQLite 版一致
```

---

## 13. 自定义一个 Driver

任何数据库都能接入：实现 `nothings_orm::driver::Driver` 的六个方法即可。核心库零 DB 依赖，SQL 模板由 `Repository` / `QueryBuilder` 生成，驱动只负责执行和物化行。

trait 定义（这是你要实现的契约，不是可直接运行的代码）：

```rust
pub trait Driver {
    fn execute(&mut self, sql: &str, params: &[Value]) -> Result<usize, Error>;
    fn query(&mut self, sql: &str, params: &[Value]) -> Result<Vec<OwnedRow>, Error>;
    fn last_insert_id(&self) -> Result<i64, Error>;
    fn begin(&mut self) -> Result<(), Error>;
    fn commit(&mut self) -> Result<(), Error>;
    fn rollback(&mut self) -> Result<(), Error>;
}
```

实现骨架（伪代码，把 `unimplemented!()` 换成你的数据库调用）：

```rust
use std::io::{Error, ErrorKind};
use nothings_orm::driver::Driver;
use nothings_orm::row::OwnedRow;
use nothings_orm::value::Value;

struct MyDriver { /* 你的连接 */ }

impl Driver for MyDriver {
    fn execute(&mut self, sql: &str, params: &[Value]) -> Result<usize, Error> {
        // 1. 用 ? 占位符 + params 绑定，绝不拼接值
        // 2. 返回受影响行数
        unimplemented!()
    }

    fn query(&mut self, sql: &str, params: &[Value]) -> Result<Vec<OwnedRow>, Error> {
        // 1. 用 ? 占位符 + params 绑定
        // 2. 全量物化：每行构造 OwnedRow::new(names, values)
        // 3. 列名原样保留，不改大小写、不加前缀
        unimplemented!()
    }

    fn last_insert_id(&self) -> Result<i64, Error> { unimplemented!() }
    fn begin(&mut self) -> Result<(), Error> { unimplemented!() }
    fn commit(&mut self) -> Result<(), Error> { unimplemented!() }
    fn rollback(&mut self) -> Result<(), Error> { unimplemented!() }
}
```

必须满足的隐含契约：

1. **占位符是 `?`**，值走 `params` 绑定。核心生成的 SQL 就是裸 `?`，不能改成 `$1` 之类的风格。
2. `query` 返回的列名必须与实体 `#[entity(column = ...)]` 精确一致（大小写敏感）。`COUNT(*) AS __count` 的别名必须原样保留——`count()` 靠列名 `__count` 找值。
3. `execute` 返回前要准备好 `last_insert_id`，因为 `Repository::insert` 紧接着就调它。
4. 错误用 `std::io::Error` + `ErrorKind`。orphan rule 下不能 `impl From<第三方错误> for io::Error`，用固有函数 `to_io` 映射。
5. 值映射：`Null` / `Bool`（→0/1）/ `I64` / `F64` / `Text` / `Blob` 六种。

参考实现：[`nothings-orm-sqlite/src/lib.rs`](../nothings-orm-sqlite/src/lib.rs)（159 行）与 [`nothings-orm-mysql/src/lib.rs`](../nothings-orm-mysql/src/lib.rs)。

---

## 14. 已知边界与常见坑

**编号与正式清单**：[`ai/spec/30-known-issues.md`](ai/spec/30-known-issues.md)（含 `MYSQL1`-`MYSQL6`）。

| 坑 | 现象 | 处置 |
| --- | --- | --- |
| 列名写错 | `r#where("name", ...)` 报 `InvalidInput`，即使实体字段叫 `name` | 传 **DB 列名**（`user_name`），即 `#[entity(column = ...)]` 的值 |
| 保留字列名 | MySQL 报 1064；core 裸拼接标识符、不加反引号 | 表名/列名避开 `order`/`key`/`group`/`desc`/`rank` 等保留字 |
| MySQL `offset()` | 单独调用生成 `SELECT ... OFFSET n`，MySQL 报 1064 | 必须搭配 `limit()`；`one()` 自带 limit，`get`/`find_one` 不受影响 |
| 主键不自增 | `insert` 返回的主键不对 | DDL 写 `INTEGER PRIMARY KEY` 或 `AUTO_INCREMENT PRIMARY KEY` |
| 类型缺失 | DATE/TIME/Decimal/JSON 读回是 `Text`，`u64` 编译不过 | 用 `String` / `i64` 承接，按第 9 节降级规则处理 |
| 时间列用 `String` 接 | TIMESTAMP/DATETIME 读回 `Value::DateTime`，`String` 字段报 `InvalidData` | 字段改用 `DateTime`，需要字符串时 `dt.to_string()` |
| 时区不生效 | 传 `Value::Text` 存时间字符串 | 只有 `Value::DateTime` 参与自动转换；`Text` / `I64` 原样存取 |
| 微秒丢失 | 列声明 `DATETIME`（默认 0 位小数） | 改用 `DATETIME(6)` / `TIMESTAMP(6)` |
| 列名大小写 | 宏按列名精确匹配，大小写不同会报缺列 | DDL 与 `#[entity(column = ...)]` 字面完全一致 |
| 嵌套事务 | `InvalidInput` | 只有一层，无 savepoint |
| 借用冲突 | 同时持有两个 `Repository` 编译不过 | `Repository` 独占 `&mut driver`；分开作用域 |
| 并发 | 驱动非 `Sync`、无连接池 | 每线程一个驱动实例 |
| 大结果集 | `query` 全量物化到内存 | 分页查询，别一次拉全表 |
| DDL 在事务里 | 隐式提交破坏事务 | 事务块里只做 DML |
| `execute_ddl` / `execute_batch` | 不参数化，绝不接用户输入 | 只传固定字面量 |

---

## 15. 能力清单：不做的事

明确不支持，别按其它 ORM 的习惯预期：

- async / tokio（全同步 API）
- 连接池、缓存、懒加载
- 关联加载（has_many / belongs_to）、JOIN 查询
- Migration（建表靠驱动 DDL）
- 软删除、时间戳钩子
- 批量插入 / upsert
- OR / LIKE / IN / 比较运算（只有等值 AND）
- 复合主键、UUID 主键
- 自定义错误类型（统一 `std::io::Error`）
- SQL 日志、metrics

这些在 Phase 2 路线里只规划未实现，见 [`ai/spec/50-orm-design.md`](ai/spec/50-orm-design.md)。

---

## 附：验收记录

本文所有示例在以下环境实际编译并运行通过：

- `rustc` / `cargo` 1.96.0，`aarch64-apple-darwin`
- SQLite：`SqliteDriver::in_memory()`，覆盖 Schema 元数据、值转换、CRUD、QueryBuilder、注入防护、约束冲突、事务、裸 Driver、手工 `from_row`
- MySQL：`MysqlDriver::connect`，MySQL 8.0.46（docker `mysql:8.0`），覆盖建库建表、insert/get、等值查询、count、UNIQUE 冲突、事务提交

MSRV 1.89+ 未单独实测（工具链下载受限），当前用 1.96 验证。详情见 [`ai/spec/51-mysql-driver-design.md`](ai/spec/51-mysql-driver-design.md)。
