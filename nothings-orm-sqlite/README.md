# nothings-orm-sqlite — SQLite 驱动

对标 Go 的 `gorm.io/driver/mysql`：实现 `nothings_orm::NOrmDriver`，把 SQLite 接到 ORM 核心上。

- 版本：0.0.1
- edition：2024
- MSRV：1.89+（rusqlite 0.32.1 bundled 实测通过）
- License：MIT

## 依赖取舍

`rusqlite = { version = "0.32", default-features = false, features = ["bundled"] }`

选 `bundled` 而非 `linked`：无需下游安装系统 SQLite，版本可复现。代价是构建机需要 C 编译器（macOS 装 Xcode CLT 即可），首次构建较慢。不开 FTS/JSON 等额外 feature。

`crate-type = ["lib"]`。SQLite 的 C 代码只在这个 crate 里，不进入其它产物。

## 用法

```rust
use nothings_orm::repository::NOrmRepository;
use nothings_orm::value::NOrmValue;
use nothings_orm_sqlite::NOrmSqliteDriver;

let mut driver = NOrmSqliteDriver::open("app.db").unwrap();
driver.execute_batch("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)").unwrap();

let mut repo = NOrmRepository::new(&mut driver);
// repo.insert::<User>(&user)?;
// let user = repo.get::<User>(NOrmValue::I64(1))?;
```

测试用内存库：`NOrmSqliteDriver::in_memory()`。

## 错误映射

`rusqlite::Error` → `std::io::Error`（orphan rule 不允许 `impl From`，用固有函数 `to_io`）：

| SQLite 错误 | ErrorKind |
| --- | --- |
| `ConstraintViolation` | `AlreadyExists` |
| `CannotOpen` / `NotFound` | `NotFound` |
| `ApiMisuse` / `ParameterOutOfRange` | `InvalidInput` |
| `NotADatabase` / `TypeMismatch` / `DatabaseCorrupt` | `InvalidData` |
| 其它 | `Other` |

## 已知边界

- `execute_batch` 直接执行原始 SQL，仅供 DDL 与测试使用，不经过参数化。不要传用户输入。
- 一条连接对应一个 `NOrmSqliteDriver`；并发场景需要每个线程一个实例（连接池在 YAGNI 清单）。
- `query` 会把结果全部物化为 `NOrmOwnedRow`，大结果集内存开销线性增长。
