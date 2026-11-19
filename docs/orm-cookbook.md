# ORM 完整演示：nothings-orm + SQLite / MySQL

一份可以直接抄的实战文档。覆盖从依赖配置到高级查询、事务、自定义驱动的**全部公开能力**。所有示例都经过真实编译与运行验证（SQLite 内存库 + MySQL 8.0.46）。

三个 crate 各司其职：

| crate | 角色 | 是否必需 |
| --- | --- | --- |
| `nothings-orm` | 核心：`NOrmEntity` / `NOrmSchema` / `NOrmDriver` / `NOrmRepository` / `NOrmQueryBuilder`，零 DB 依赖 | 必需 |
| `nothings-orm-sqlite` | SQLite 驱动（rusqlite bundled，纯 C 内置） | 二选一 |
| `nothings-orm-mysql` | MySQL 驱动（同步 `mysql` crate，`minimal-rust` 纯 Rust） | 二选一 |

设计背景见 [`ai/spec/50-orm-design.md`](ai/spec/50-orm-design.md) 与 [`ai/spec/51-mysql-driver-design.md`](ai/spec/51-mysql-driver-design.md)。

---

## 目录

1. [依赖配置](#1-依赖配置)
2. [定义实体](#2-定义实体)
3. [NOrmSchema 元数据](#3-schema-元数据)
4. [建表与连接](#4-建表与连接)
5. [CRUD：增删改查](#5-crud增删改查)
6. [NOrmQueryBuilder：条件 / 排序 / 分页 / 计数](#6-querybuilder条件--排序--分页--计数)
7. [事务](#7-事务)
8. [错误处理](#8-错误处理)
9. [值类型与映射](#9-值类型与映射)
10. [直接使用 NOrmDriver 与 NOrmRow](#10-直接使用-driver-与-row)
11. [两个驱动的差异](#11-两个驱动的差异)
12. [完整可运行示例](#12-完整可运行示例)
13. [自定义一个 NOrmDriver](#13-自定义一个-driver)
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

用 `#[derive(NOrmEntity)]` 声明一张表。**这是唯一需要写的结构性代码**——NOrmSchema、行解码、按列名取值全部由宏生成。

```rust
use nothings_orm::NOrmEntity;

#[derive(Debug, Clone, PartialEq, NOrmEntity)]
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
| 字段 | `#[entity(has_many = "books", target = "Book", foreign_key = "author_id")]` | 否 | 一对多关联，字段类型 `Vec<T>`，外键在目标表 |
| 字段 | `#[entity(belongs_to = "author", target = "Author", foreign_key = "author_id")]` | 否 | 多对一关联，字段类型 `Option<T>`，外键在本表 |

支持的字段类型：`i8/i16/i32/i64/u8/u16/u32`、`f32/f64`、`bool`、`String`、`Vec<u8>`、`NOrmDateTime`、`Option<T>`（T 为上述类型）。**不支持** `u64`、`&str`、Decimal、JSON 作为实体字段类型（见第 9 节）。时间列请用 `NOrmDateTime`，不要用 `String`（读回会得到 `NOrmValue::NOrmDateTime`，`String` 会报 `InvalidData`）。

> ⚠️ `entity` 属性里写的 `column` 是 **DB 列名**。后面所有 `where` / `order_by` 传的都是这个名字，不是 Rust 字段名。上面 `name` 字段对应 `user_name`，写 `r#where("name", ...)` 会得到 `InvalidInput`。这是最容易踩的坑。

主键设计假设：一个整数自增主键。`NOrmRepository::insert` 会把主键列从 INSERT 里排除，靠 `last_insert_id()` 回读。DDL 必须写成 `INTEGER PRIMARY KEY`（SQLite）或 `BIGINT AUTO_INCREMENT PRIMARY KEY`（MySQL）。

---

## 3. NOrmSchema 元数据

派生宏会为实体生成 `NOrmSchema` 实现，可以在编译期拿到表结构：

```rust
use nothings_orm::schema::NOrmSchema;

assert_eq!(User::TABLE, "users");
assert_eq!(User::COLUMNS.len(), 4);
assert_eq!(User::pk().name, "id");

for col in User::COLUMNS {
    println!("{} -> {} ({:?}), pk={}, nullable={}",
        col.field, col.name, col.sql_type, col.primary_key, col.nullable);
}
```

`NOrmColumn` 字段：`field`（Rust 字段名）、`name`（DB 列名）、`sql_type`（`Integer`/`Real`/`Text`/`Blob`）、`nullable`、`primary_key`。

`validate_column::<User>("user_name")` 会做白名单校验，非法列名返回 `InvalidInput`。

---

## 4. 建表与连接

核心库不生成 DDL——建表由驱动提供原始 SQL 入口。

**SQLite**（内存库，测试首选）：

```rust
use nothings_orm_sqlite::NOrmSqliteDriver;

let mut driver = NOrmSqliteDriver::in_memory().unwrap();
driver.execute_batch(
    "CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        user_name TEXT NOT NULL UNIQUE,
        email TEXT,
        active INTEGER NOT NULL DEFAULT 0
    )",
).unwrap();

// 持久化到文件
// let mut driver = NOrmSqliteDriver::open("app.db").unwrap();
```

**MySQL**：

```rust
use nothings_orm_mysql::NOrmMysqlDriver;

let mut driver = NOrmMysqlDriver::connect("mysql://root:pwd@127.0.0.1:3306/app").unwrap();
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

全部入口是 `NOrmRepository::new(&mut driver)`。它持有驱动的可变借用，因此同一时刻只能有一个 repo；需要直接用 driver 时，让 repo 的作用域先结束。

```rust
use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmValue;

let mut repo = NOrmRepository::new(&mut driver);

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
let got: User = repo.get(NOrmValue::I64(id)).unwrap();

// 按主键取 —— 找不到返回 Ok(None)，不算错误
let maybe = repo.find_one::<User>(NOrmValue::I64(999)).unwrap();
assert!(maybe.is_none());

// 取全部
let all = repo.find_all::<User>().unwrap();

// 更新 —— 按主键更新所有非主键列，返回受影响行数
let mut updated = got;
updated.email = Some("new@example.com".into());
assert_eq!(repo.update(&updated).unwrap(), 1);

// 删除 —— 按主键删，返回受影响行数
assert_eq!(repo.delete::<User>(NOrmValue::I64(id)).unwrap(), 1);
```

方法清单：

| 方法 | 签名要点 | 返回 |
| --- | --- | --- |
| `insert::<E>` | `&E` | `Result<i64>` 新主键 |
| `get::<E>` | `NOrmValue` 主键 | `Result<E>`，未找到 `NotFound` |
| `find_one::<E>` | `NOrmValue` 主键 | `Result<Option<E>>` |
| `find_all::<E>` | 无 | `Result<Vec<E>>` |
| `update::<E>` | `&E` | `Result<usize>` 受影响行数 |
| `delete::<E>` | `NOrmValue` 主键 | `Result<usize>` 受影响行数 |

`NOrmValue` 是运行时列值：`Null` / `Bool` / `I64` / `F64` / `Text` / `Blob`。主键传 `NOrmValue::I64(id)`。

Trait 方法 `from_row` / `pk_value` / `column_value` 需要显式导入 trait，因为 crate 根的同名 `NOrmEntity` 是派生宏：

```rust
use nothings_orm::entity::NOrmEntity as EntityTrait;

assert_eq!(<User as EntityTrait>::pk_value(&got), NOrmValue::I64(1));
assert_eq!(
    <User as EntityTrait>::column_value(&got, "user_name").unwrap(),
    NOrmValue::Text("alice".into()),
);
```

---

## 6. NOrmQueryBuilder：条件 / 排序 / 分页 / 计数

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

```rust
use nothings_orm::condition::{NOrmCondition, NOrmOperator};

// OR + 比较运算：链式入口
let rows = repo
    .query::<User>()
    .r#where("active", true)
    .or_where("user_name", "alice")
    .where_op("id", NOrmOperator::Gt, 10i64)
    .all()
    .unwrap();

// 嵌套 + NOT：(id > ? OR user_name = ?) AND NOT (email IS NULL)
let cond = NOrmCondition::<User>::gt("id", 10i64)
    .or(NOrmCondition::eq("user_name", "alice"))
    .and(!NOrmCondition::is_null("email"));
let rows = repo.query::<User>().where_condition(cond).all().unwrap();
```

- 链式条件按调用顺序**从左到右**结合（左折叠）：每个方法用自身携带的连接词接到已累积条件上，构建器不按 SQL 优先级重排，渲染器用最小括号还原该顺序。
- 链式入口：`r#where` / `and_where` / `or_where` / `not_where`（等值），`where_op` / `or_where_op`（比较运算）。首项 `or_where` 退化为裸首项。
- `NOrmCondition` 树通过 `where_condition` / `or_where_condition` / `not_where_condition` 注入，可表达嵌套与 NOT 取反；构造器 `eq/ne/gt/ge/lt/le/like/not_like/cmp`、`in_list/not_in_list`、`is_null/is_not_null`，组合器 `and/or/group`，取反用 `!` 运算符（实现 `std::ops::Not`）。
- `IN` 空列表折叠为 `0 = 1` / `1 = 1`；LIKE 通配符由调用方在 pattern 里提供；`NOrmCondition` 独立构造时需写 `NOrmCondition::<User>::...`（参数里不含实体类型）。
- 第二个参数接受任何 `NOrmToValue`（含 `&str` / `String` / 数字 / `bool`）。
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

### 6.1 JOIN 与子查询

```rust
use nothings_orm::join::{NOrmJoinKind, NOrmSubquery};

// INNER JOIN + 多实体映射：主表 t0，JOIN 表按声明顺序 t1、t2...
let rows = repo
    .query::<Author>()
    .join::<Book>(NOrmJoinKind::Inner, &[("id", "author_id")])
    .all_with::<(Author, Book)>()
    .unwrap();
// rows: Vec<(Author, Book)>，元组元素按 JOIN 声明顺序对应

// JOIN 后只取主表实体（关联列被忽略，JOIN 仅用于过滤）
let authors = repo
    .query::<Author>()
    .join::<Book>(NOrmJoinKind::Left, &[("id", "author_id")])
    .all()
    .unwrap();

// JOIN 表上的条件必须用 where_qualified 限定别名
let rows = repo
    .query::<Author>()
    .join::<Book>(NOrmJoinKind::Inner, &[("id", "author_id")])
    .where_qualified::<Book>("t1", "title", "two")
    .all()
    .unwrap();

// 子查询：IN / EXISTS
let sub = NOrmSubquery::with_params(
    "SELECT author_id FROM books WHERE year = ?",
    vec![NOrmValue::I64(2022)],
);
let rows = repo.query::<Author>().where_in_subquery("id", sub).all().unwrap();
let rows = repo
    .query::<Author>()
    .where_exists(NOrmSubquery::new("SELECT 1 FROM books WHERE books.author_id = authors.id"))
    .all()
    .unwrap();
```

规则：
- `join::<T>(kind, on)` 的 `on` 是 `[(左列, 右列)]`，左列按主表 `E` 校验、右列按 `T` 校验；ON 列只含标识符字符，非法延后报 `InvalidInput`。
- JOIN 查询把列渲染为 `t0.id AS t0_id`；`all_with::<(A, B)>()` 按前缀拆成元组（支持 1~4 元），元组元素个数须与 JOIN 数 + 1 一致。
- JOIN 模式下 `r#where` 的裸列自动补主表前缀 `t0.`；JOIN 表的列用 `where_qualified::<T>("t1", col, value)`。
- 子查询 SQL 是调用方提供的固定字面量（禁止拼用户输入），值走 `params` 参数化；CROSS JOIN 传空 `on` 即可。
- `count()` 在 JOIN 下是 `COUNT(*)`，一对多会按行数计数，需要去重时自己写裸 SQL。

### 6.2 Preload：单层关联聚合

派生宏用字段属性声明关联，查询时用 `preload(name)` 填充：

```rust
#[derive(NOrmEntity)]
#[entity(table = "authors")]
struct Author {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(column = "name")]
    name: String,
    #[entity(has_many = "books", target = "Book", foreign_key = "author_id")]
    books: Vec<Book>,
}

#[derive(NOrmEntity)]
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
}

// has_many：主表主键 → 目标表外键
let authors = repo.query::<Author>().preload("books").all().unwrap();
assert!(!authors[0].books.is_empty());

// belongs_to：本表外键 → 目标表主键
let books = repo.query::<Book>().preload("author").all().unwrap();
assert_eq!(books[0].author.as_ref().unwrap().name, "A");
```

规则：
- `has_many = "名"` 填 `Vec<T>`（外键在目标表）；`belongs_to = "名"` 填 `Option<T>`（外键在本表）。`target` 是 Rust 类型路径，`foreign_key` 是 DB 列名。三者必须同时出现，关联字段不得再标 `primary_key` / `nullable` / `column`。
- `preload(name)` 支持点分路径（`books.comments`），每层批量查询、非 N+1；首段拼错在终结方法报 `InvalidInput` 且**不产生任何 SQL**，后续段在执行期逐层校验。重叠路径自动保留最长，避免重复查询。
- 两阶段查询：先查主表，再用主键批量查关联表（`WHERE author_id IN (?, ...)`），按外键分组填回。不是 JOIN，也不会产生 N+1。
- 单次 IN 最多 `NORM_PRELOAD_CHUNK_SIZE`（500）个键，自动分批；超限时多批查询后合并。
- **递归**：`preload("books.comments")` 逐层收集同层子实体成连续切片后批量查询，再回填各父实体——层数等于路径段数，不会 N+1。重复类型路径（如 `books.author.books`）每层由 `from_row` 重新解码为独立实例，关联字段为空初值、仅由更深路径填充，不形成对象图环。
- **结果去重**：按目标实体主键对查询结果去重（保留首次），跨父实体仍各自独立解码。
- **懒加载**：已取出的单个实体可用 `repo.load_relation(&mut entity, "books.comments")` 原地补关联；每次调用只查一个实体，逐个调用产生 N+1，批量请用 `preload`。
- belongs_to 的外键必须是本表**真实标量列**（如 `author_id`）；外键为 NULL 时关联字段保持 `None`，不进 IN 列表。
- 双向关联（Author.books + Book.author）必须用 `Vec<T>` / `Option<T>` 打断尺寸递归；直接互含会编译失败。
- 关联字段不进 `COLUMNS`、不进 `SELECT`、不进 `column_value()`；`from_row` 给空初值，由 Preload 层填回。

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
match repo.get::<User>(NOrmValue::I64(999)) {
    Ok(u) => println!("找到 {u:?}"),
    Err(e) if e.kind() == ErrorKind::NotFound => println!("没有这个用户"),
    Err(e) => eprintln!("其它错误: {e}"),
}
```

---

## 9. 值类型与映射

### 支持的 Rust 类型

| Rust 类型 | `NOrmSqlType` | 说明 |
| --- | --- | --- |
| `i8`/`i16`/`i32`/`i64`/`u8`/`u16`/`u32` | `Integer` | 统一走 `NOrmValue::I64` |
| `f32`/`f64` | `Real` | 统一走 `NOrmValue::F64` |
| `bool` | `Integer` | 0/1 |
| `String` | `Text` | |
| `Vec<u8>` | `Blob` | |
| `NOrmDateTime` | `Text` | 墙钟时间，参与 MySQL 双时区自动转换 |
| `Option<T>` | 同 T | `None` ↔ `NOrmValue::Null` |

`&str` 实现了 `NOrmToValue`（可直接当查询参数），但**不能作实体字段**（没有 `NOrmFromValue` / `NOrmHasSqlType`）。

缺失：`u64`、`i128`、`usize`、Decimal、UUID、JSON 的原生支持。日期时间用 `NOrmDateTime`（见 9.1），或 `i64` Unix 时间戳。

### MySQL 特有类型的降级

`NOrmValue` 只有六种变体，MySQL 的其它类型按下面规则降级（无类型保真）：

| MySQL 类型 | 映射为 | 说明 |
| --- | --- | --- |
| `DATETIME` / `DATE` / `TIME` / `TIMESTAMP` | `NOrmValue::Text` | 格式化字符串，如 `2026-01-02 03:04:05` |
| `DECIMAL` / `NEWDECIMAL` | `NOrmValue::Text` | 字符串，避免精度丢失 |
| `JSON` | `NOrmValue::Text` | 原始 JSON 文本 |
| `TINYINT` | `NOrmValue::I64` | `bool::from_value` 会把非 0 当 true |
| `BIGINT UNSIGNED` ≤ `i64::MAX` | `NOrmValue::I64` | |
| `BIGINT UNSIGNED` > `i64::MAX` | `Err(InvalidData)` | 放不下就报错，不静默截断 |

Text 与 Blob 的区分**靠列元数据**（`ColumnType` + `BINARY_FLAG` + charset），不靠 UTF-8 试探——BLOB 内容可能恰好是合法 UTF-8，而 JSON/DECIMAL 自带 binary charset。

### 9.1 时区与日期时间（MySQL）

core 提供零依赖的 `NOrmUtcOffset`（固定偏移）与 `NOrmDateTime`（墙钟）。**自动转换只对 `NOrmValue::NOrmDateTime` 生效**，`Text` / `I64` 一律不转——避免误伤 DECIMAL / JSON / 普通字符串。

```rust
use nothings_orm::value::{NOrmDateTime, NOrmUtcOffset};
use nothings_orm_mysql::NOrmMysqlDriver;

// 旧入口：UTC/UTC，不做转换（既有代码零改动）
let mut driver = NOrmMysqlDriver::connect(dsn)?;

// 双时区：ORM=东八区墙钟，DB 中时间按 UTC 表达
let orm_tz = NOrmUtcOffset::parse("+08:00")?;
let db_tz = NOrmUtcOffset::UTC;
let mut driver = NOrmMysqlDriver::connect_with_tz(dsn, orm_tz, db_tz)?;
```

语义：

- **写入**：`NOrmValue::NOrmDateTime(09:00 +08:00)` → `01:00 UTC` 写库。查询条件 `r#where("at", dt)` 走**同一条路径**，无需额外处理。
- **读取**：`TIMESTAMP` / `DATETIME` 列 → 按 DB→ORM 换算 → `NOrmValue::NOrmDateTime`。`DATE` / `TIME` / `YEAR` → `Text`（无完整墙钟）。
- **语义统一**：所有时间列读回 `NOrmValue::NOrmDateTime`，不分入口。`String` 字段会读到 `InvalidData`，**必须改用 `NOrmDateTime` 字段**。
- `i64` Unix 时间戳不受影响，原样存取。
- 会话时区通过 `OptsBuilder::init` 下发 `SET time_zone`（DSN 不接受 `?timezone=`），连接后校验；`init` 在连接重置时自动重放。
- 固定偏移**不含 DST**，不支持 `Asia/Shanghai` 这类命名时区的自动夏令时。
- **SQLite 驱动不做时区转换**：`NOrmDateTime` 按墙钟原样写 TEXT，读回走 `NOrmFromValue` 的 Text 解析兜底。

微秒需要列声明为 `DATETIME(6)` / `TIMESTAMP(6)`，MySQL 默认精度是 0 位小数。外部程序绕过 ORM 直写 DATETIME 字面量的数据，不保证转换语义正确。

---

## 10. 直接使用 NOrmDriver 与 NOrmRow

绕过 NOrmRepository，直接发参数化 SQL。`NOrmDriver` trait 需要显式导入才能调方法。

```rust
use nothings_orm::driver::NOrmDriver;
use nothings_orm::row::NOrmRow;
use nothings_orm::value::NOrmValue;

let rows = driver.query("SELECT id, user_name FROM users", &[]).unwrap();
assert_eq!(NOrmRow::column_name(&rows[0], 1), "user_name");
assert_eq!(NOrmRow::value(&rows[0], 1).unwrap(), NOrmValue::Text("alice".into()));

let affected = driver
    .execute(
        "UPDATE users SET active = ? WHERE id = ?",
        &[NOrmValue::Bool(false), NOrmValue::I64(1)],
    )
    .unwrap();

let last_id = driver.last_insert_id().unwrap();
```

`NOrmDriver` 六个方法：

| 方法 | 说明 |
| --- | --- |
| `execute(&mut self, sql, &[NOrmValue]) -> Result<usize>` | 非查询，返回受影响行数 |
| `query(&mut self, sql, &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>>` | 查询，**全量物化**为 owned 行 |
| `last_insert_id(&self) -> Result<i64>` | 最近插入主键 |
| `begin` / `commit` / `rollback(&mut self)` | 事务 |

`NOrmOwnedRow` 提供 `names()` / `values()`，也实现 `NOrmRow` trait。驱动必须保证 `query` 返回的列名与实体 `#[entity(column = ...)]` **大小写敏感精确匹配**。

手工解码一行：

```rust
use nothings_orm::entity::NOrmEntity as EntityTrait;

let rows = driver.query("SELECT id, user_name, email, active FROM users", &[]).unwrap();
let u = <User as EntityTrait>::from_row(&rows[0]).unwrap();
```

---

## 11. 两个驱动的差异

| 项 | `NOrmSqliteDriver` | `NOrmMysqlDriver` |
| --- | --- | --- |
| 构造 | `open(path)` / `in_memory()` | `connect(url)` |
| DDL 入口 | `execute_batch(sql)` | `execute_ddl(sql)` |
| 主键 DDL | `INTEGER PRIMARY KEY` | `BIGINT AUTO_INCREMENT PRIMARY KEY` |
| 占位符 | `?` | `?`（原生兼容） |
| 事务语句 | `BEGIN` / `COMMIT` / `ROLLBACK` | `START TRANSACTION` / `COMMIT` / `ROLLBACK` |
| 多语句 | 支持 | 实测也支持（客户端开启 `CLIENT_MULTI_STATEMENTS`），但不承诺 |
| 连接 | 一条连接，非 `Sync` | 一条连接，非 `Sync` |
| 连接字符串 | 文件路径 | `mysql://user:pass@host:port/db` |
| 时区自动转换 | **不支持**（按墙钟原样写 TEXT） | `connect_with_tz(orm_tz, db_tz)` 双向转换，仅对 `NOrmValue::NOrmDateTime` 生效 |
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

use nothings_orm::NOrmEntity;
use nothings_orm::entity::NOrmEntity as EntityTrait;
use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmValue;
use nothings_orm_sqlite::NOrmSqliteDriver;

#[derive(Debug, Clone, PartialEq, NOrmEntity)]
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
    let mut driver = NOrmSqliteDriver::in_memory().unwrap();
    driver.execute_batch(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            user_name TEXT NOT NULL UNIQUE,
            email TEXT,
            active INTEGER NOT NULL DEFAULT 0
        )",
    ).unwrap();

    // 插入 + 回读主键
    let mut repo = NOrmRepository::new(&mut driver);
    let id = repo.insert(&User {
        id: 0,
        name: "alice".into(),
        email: Some("a@example.com".into()),
        active: true,
    }).unwrap();
    assert_eq!(id, 1);

    let got: User = repo.get(NOrmValue::I64(id)).unwrap();
    assert_eq!(<User as EntityTrait>::pk_value(&got), NOrmValue::I64(1));

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

把 `NOrmSqliteDriver::in_memory()` 换成 `NOrmMysqlDriver::connect(&dsn)`、`execute_batch` 换成 `execute_ddl`，DDL 换成 MySQL 方言，其余代码**完全不变**：

```rust
use nothings_orm_mysql::NOrmMysqlDriver;

let mut driver = NOrmMysqlDriver::connect("mysql://root:pwd@127.0.0.1:3306/app").unwrap();
driver.execute_ddl(
    "CREATE TABLE users (
        id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
        user_name VARCHAR(255) NOT NULL UNIQUE,
        email VARCHAR(255) NULL,
        active TINYINT NOT NULL DEFAULT 0
    )",
).unwrap();
// 后续 NOrmRepository 代码与 SQLite 版一致
```

---

## 13. 自定义一个 NOrmDriver

任何数据库都能接入：实现 `nothings_orm::driver::NOrmDriver` 的六个方法即可。核心库零 DB 依赖，SQL 模板由 `NOrmRepository` / `NOrmQueryBuilder` 生成，驱动只负责执行和物化行。

trait 定义（这是你要实现的契约，不是可直接运行的代码）：

```rust
pub trait NOrmDriver {
    fn execute(&mut self, sql: &str, params: &[NOrmValue]) -> Result<usize, Error>;
    fn query(&mut self, sql: &str, params: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error>;
    fn last_insert_id(&self) -> Result<i64, Error>;
    fn begin(&mut self) -> Result<(), Error>;
    fn commit(&mut self) -> Result<(), Error>;
    fn rollback(&mut self) -> Result<(), Error>;
}
```

实现骨架（伪代码，把 `unimplemented!()` 换成你的数据库调用）：

```rust
use std::io::{Error, ErrorKind};
use nothings_orm::driver::NOrmDriver;
use nothings_orm::row::NOrmOwnedRow;
use nothings_orm::value::NOrmValue;

struct MyDriver { /* 你的连接 */ }

impl NOrmDriver for MyDriver {
    fn execute(&mut self, sql: &str, params: &[NOrmValue]) -> Result<usize, Error> {
        // 1. 用 ? 占位符 + params 绑定，绝不拼接值
        // 2. 返回受影响行数
        unimplemented!()
    }

    fn query(&mut self, sql: &str, params: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error> {
        // 1. 用 ? 占位符 + params 绑定
        // 2. 全量物化：每行构造 NOrmOwnedRow::new(names, values)
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
3. `execute` 返回前要准备好 `last_insert_id`，因为 `NOrmRepository::insert` 紧接着就调它。
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
| 时间列用 `String` 接 | TIMESTAMP/DATETIME 读回 `NOrmValue::NOrmDateTime`，`String` 字段报 `InvalidData` | 字段改用 `NOrmDateTime`，需要字符串时 `dt.to_string()` |
| 时区不生效 | 传 `NOrmValue::Text` 存时间字符串 | 只有 `NOrmValue::NOrmDateTime` 参与自动转换；`Text` / `I64` 原样存取 |
| 微秒丢失 | 列声明 `DATETIME`（默认 0 位小数） | 改用 `DATETIME(6)` / `TIMESTAMP(6)` |
| 列名大小写 | 宏按列名精确匹配，大小写不同会报缺列 | DDL 与 `#[entity(column = ...)]` 字面完全一致 |
| 嵌套事务 | `InvalidInput` | 只有一层，无 savepoint |
| 借用冲突 | 同时持有两个 `NOrmRepository` 编译不过 | `NOrmRepository` 独占 `&mut driver`；分开作用域 |
| 并发 | 驱动非 `Sync`、无连接池 | 每线程一个驱动实例 |
| 大结果集 | `query` 全量物化到内存 | 分页查询，别一次拉全表 |
| DDL 在事务里 | 隐式提交破坏事务 | 事务块里只做 DML |
| `execute_ddl` / `execute_batch` | 不参数化，绝不接用户输入 | 只传固定字面量 |

---

## 15. 能力清单：不做的事

明确不支持，别按其它 ORM 的习惯预期：

- async / tokio（全同步 API）
- 连接池、缓存
- 递归/多层关联预加载与懒加载均已支持，见 6.2
- Migration（建表靠驱动 DDL）
- 复合主键、UUID 主键
- 自定义错误类型（统一 `std::io::Error`）
- SQL 日志、metrics

软删除、时间戳钩子与自动时间、批量插入/upsert 均已支持，见下方新增章节；其余在 Phase 2 路线里只规划未实现，见 [`ai/spec/50-orm-design.md`](ai/spec/50-orm-design.md)。

---

## 16. 软删除、时间戳钩子与自动时间

```rust
#[derive(NOrmEntity)]
#[entity(table = "notes")]
struct Note {
    #[entity(primary_key, column = "id")]
    id: i64,
    #[entity(soft_delete, column = "deleted_at")]
    deleted_at: Option<NOrmDateTime>,
    #[entity(created_at)]
    created_at: NOrmDateTime,
    #[entity(updated_at)]
    updated_at: NOrmDateTime,
}
```

- `soft_delete` 字段必须是 `Option<NOrmDateTime>`；其它两个必须是 `NOrmDateTime` 或 `Option<NOrmDateTime>`；三者互斥、不能是主键或关联字段。
- 读：所有查询自动加 `deleted_at IS NULL`；`repo.query::<Note>().unscoped().all()` 可见已删行（仅影响读）。
- 写：`repo.delete(pk)` 改 `UPDATE ... SET deleted_at=now()`；`repo.hard_delete(pk)` 才是物理删除；`update()` 不会修改已删行。
- 时间：`insert` 注入 created_at/updated_at（同一时刻），`update` 只刷新 updated_at。**总覆盖实体自带值、不写回内存实体**，需重新查询才见新值。
- `NOrmDateTime::now_utc()` 取 UTC 墙钟（微秒为 0，零新依赖）。**MySQL 使用时间钩子必须用 `connect_with_tz(NOrmUtcOffset::UTC, db_tz)`**，否则注入值会偏移；SQLite 无此问题。
- DDL 示例：`deleted_at DATETIME(6) NULL, created_at DATETIME(6) NOT NULL, updated_at DATETIME(6) NOT NULL`。

---

## 17. 批量插入 / upsert

```rust
// 批量插入：返回插入总行数；排除主键依赖自增；托管列语义同 insert。
let rows = vec![Item { id: 0, name: "a".into(), qty: 1 }, /* ... */];
let n = repo.insert_batch(&rows)?;

// upsert：按主键/唯一键冲突时更新，否则插入。
// 注意：必须为每个实体填真实主键值（INSERT 会写入主键列）。
let rows = vec![Item { id: 1, name: "new".into(), qty: 9 }, /* ... */];
let n = repo.upsert_batch(&rows)?;   // 未实现 upsert_sql 的驱动返回 InvalidInput
```

- `insert_batch(&[E]) -> usize`：空切片返回 0 且不发 SQL；软删列强制 NULL、时间列注入同一 now。
- `upsert_batch(&[E]) -> usize`：**须提供真实主键**（`pk_value()`），否则冲突判断失效；更新列排除主键/软删列/created_at，保留 updated_at。MySQL 受影响行数「新增计 1、更新计 2」，只宜判非零。
- 方言由驱动实现：SQLite `ON CONFLICT(pk) DO UPDATE SET c = excluded.c`；MySQL `ON DUPLICATE KEY UPDATE c = VALUES(c)`。手写驱动可用 `NOrmDriver::upsert_sql` 默认实现（返回 `InvalidInput`）。
- 分块：单条 INSERT 参数不超过 900，自动分块、**非原子**；要整体回滚请用 `repo.transaction(|repo| { ... })` 包裹。
- 需整体一致性的大批量写入推荐包一层事务；upsert 要求目标表有主键/唯一键。

---

## 附：验收记录

本文所有示例在以下环境实际编译并运行通过：

- `rustc` / `cargo` 1.96.0，`aarch64-apple-darwin`
- SQLite：`NOrmSqliteDriver::in_memory()`，覆盖 NOrmSchema 元数据、值转换、CRUD、NOrmQueryBuilder、注入防护、约束冲突、事务、裸 NOrmDriver、手工 `from_row`
- MySQL：`NOrmMysqlDriver::connect`，MySQL 8.0.46（docker `mysql:8.0`），覆盖建库建表、insert/get、等值查询、count、UNIQUE 冲突、事务提交

MSRV 1.89+ 已实测通过：在当前工作区代码上用 `cargo +1.89.0 check --all-targets` 逐 crate 验证，5 个 manifest（根、`nothings-macros`、`nothings-orm`、`nothings-orm-sqlite`、`nothings-orm-mysql`）全部编译通过。本文示例的编译运行环境仍是 `rustc` / `cargo` 1.96.0（见上方）。详情见 [`ai/spec/51-mysql-driver-design.md`](ai/spec/51-mysql-driver-design.md)。
