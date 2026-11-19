//! NOrmRepository：CRUD 与事务。

use std::io::{Error, ErrorKind};

use crate::driver::NOrmDriver;
use crate::entity::{NOrmEntity, NOrmPreload};
use crate::query::NOrmQueryBuilder;
use crate::schema::validate_column;
use crate::value::{NOrmDateTime, NOrmValue};

/// 批量 INSERT 单条语句的参数上限。SQLite 默认 999 个绑定变量、MySQL 也有
/// packet 限制，按 900 保守分块，避免多值 INSERT 因参数过多被拒。
const BATCH_MAX_PARAMS: usize = 900;

/// 计算某实体一次批量写入的列顺序（排除主键）。
fn batch_columns<E: NOrmEntity>() -> Vec<&'static str> {
    E::COLUMNS
        .iter()
        .filter(|c| !c.primary_key)
        .map(|c| c.name)
        .collect()
}

/// 取某列在插入时的值：软删列强制 NULL、时间列注入 now，其余取实体自带值。
fn managed_value<E: NOrmEntity>(
    name: &str,
    now: &NOrmValue,
    entity: &E,
) -> Result<NOrmValue, Error> {
    if Some(name) == E::SOFT_DELETE_COL {
        return Ok(NOrmValue::Null);
    }
    if Some(name) == E::CREATED_AT_COL || Some(name) == E::UPDATED_AT_COL {
        return Ok(now.clone());
    }
    entity.column_value(name)
}

/// 泛型仓储，持有驱动的可变借用。
pub struct NOrmRepository<'a, D: NOrmDriver> {
    driver: &'a mut D,
    in_transaction: bool,
}

impl<'a, D: NOrmDriver> NOrmRepository<'a, D> {
    pub fn new(driver: &'a mut D) -> Self {
        Self {
            driver,
            in_transaction: false,
        }
    }

    fn pk_column<E: NOrmEntity>() -> &'static str {
        E::pk().name
    }

    /// 插入一行，返回主键。
    pub fn insert<E: NOrmEntity>(&mut self, entity: &E) -> Result<i64, Error> {
        let columns = batch_columns::<E>();
        if columns.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{} 没有可插入的列", E::TABLE),
            ));
        }
        let placeholders = vec!["?"; columns.len()];
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            E::TABLE,
            columns.join(", "),
            placeholders.join(", ")
        );
        // 托管列：软删列强制 NULL（新行必须未删除）；时间列注入当前 UTC 时间。
        // 总覆盖实体自带值——钩子是权威的创建/更新时间。
        let now = NOrmValue::NOrmDateTime(NOrmDateTime::now_utc());
        let values = columns
            .iter()
            .map(|name| managed_value::<E>(name, &now, entity))
            .collect::<Result<Vec<_>, _>>()?;
        self.driver.execute(&sql, &values)?;
        self.driver.last_insert_id()
    }

    /// 批量插入，返回插入的总行数。空切片返回 0（不执行 SQL）。
    ///
    /// 与 [`insert`](Self::insert) 共享托管列语义：软删列强制 NULL、时间列注入
    /// 同一 now。一次多值 INSERT 的参数数不超过 900（`BATCH_MAX_PARAMS`），超出时
    /// 自动分块；每块独立执行，因此本方法**不是原子操作**，需要整体事务请用
    /// [`transaction`](Self::transaction) 包裹。
    pub fn insert_batch<E: NOrmEntity>(&mut self, entities: &[E]) -> Result<usize, Error> {
        if entities.is_empty() {
            return Ok(0);
        }
        let columns = batch_columns::<E>();
        if columns.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{} 没有可插入的列", E::TABLE),
            ));
        }
        let per_row = columns.len();
        let chunk_size = (BATCH_MAX_PARAMS / per_row).max(1);
        let now = NOrmValue::NOrmDateTime(NOrmDateTime::now_utc());
        let mut total = 0usize;
        for chunk in entities.chunks(chunk_size) {
            let one_row = format!("({})", vec!["?"; per_row].join(", "));
            let sql = format!(
                "INSERT INTO {} ({}) VALUES {}",
                E::TABLE,
                columns.join(", "),
                vec![one_row; chunk.len()].join(", ")
            );
            let mut values = Vec::with_capacity(chunk.len() * per_row);
            for entity in chunk {
                for name in &columns {
                    values.push(managed_value::<E>(name, &now, entity)?);
                }
            }
            total += self.driver.execute(&sql, &values)?;
        }
        Ok(total)
    }

    /// 批量 upsert：主键/唯一键冲突时更新，否则插入，返回受影响的总行数。
    ///
    /// SQL 由驱动的 [`NOrmDriver::upsert_sql`] 生成方言尾句：SQLite 用
    /// `ON CONFLICT ... DO UPDATE`，MySQL 用 `ON DUPLICATE KEY UPDATE`。
    /// 未实现该方法的驱动返回 `InvalidInput`。
    ///
    /// 与 [`insert_batch`](Self::insert_batch) 不同，upsert 的 INSERT 写入主键列
    /// （值取 `pk_value()`）——唯一键冲突检测必须有主键值；而纯插入依赖自增、
    /// 排除主键。因此调用方须为待 upsert 的实体提供真实主键值。
    ///
    /// 冲突更新列 = 全部非主键列，但排除软删列与 created_at：避免把已软删的行
    /// 复活、也不覆盖原始创建时间；updated_at 保留在更新列中，其 excluded 值
    /// 是本批注入的 now。空切片返回 0；参数过多自动分块（非原子，见
    /// [`insert_batch`](Self::insert_batch)）。
    pub fn upsert_batch<E: NOrmEntity>(&mut self, entities: &[E]) -> Result<usize, Error> {
        if entities.is_empty() {
            return Ok(0);
        }
        let pk = Self::pk_column::<E>();
        let columns = E::COLUMNS.iter().map(|c| c.name).collect::<Vec<_>>();
        if columns.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{} 没有可更新的列", E::TABLE),
            ));
        }
        // 更新列排除主键、软删列、created_at
        let update_cols = columns
            .iter()
            .copied()
            .filter(|name| *name != pk)
            .filter(|name| Some(*name) != E::SOFT_DELETE_COL)
            .filter(|name| Some(*name) != E::CREATED_AT_COL)
            .collect::<Vec<_>>();
        let suffix = self.driver.upsert_sql(E::TABLE, pk, &update_cols)?;
        if !suffix.starts_with(' ') {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "upsert_sql 返回的方言尾句必须以空格开头",
            ));
        }
        let per_row = columns.len();
        let chunk_size = (BATCH_MAX_PARAMS / per_row).max(1);
        let now = NOrmValue::NOrmDateTime(NOrmDateTime::now_utc());
        let mut total = 0usize;
        for chunk in entities.chunks(chunk_size) {
            let one_row = format!("({})", vec!["?"; per_row].join(", "));
            let sql = format!(
                "INSERT INTO {} ({}) VALUES {}{}",
                E::TABLE,
                columns.join(", "),
                vec![one_row; chunk.len()].join(", "),
                suffix
            );
            let mut values = Vec::with_capacity(chunk.len() * per_row);
            for entity in chunk {
                for name in &columns {
                    // 主键值取 pk_value()，其余列走托管注入（软删 NULL、时间 now）。
                    if *name == pk {
                        values.push(entity.pk_value());
                    } else {
                        values.push(managed_value::<E>(name, &now, entity)?);
                    }
                }
            }
            total += self.driver.execute(&sql, &values)?;
        }
        Ok(total)
    }

    /// 按主键查找，未找到返回 NotFound。
    pub fn get<E: NOrmEntity>(&mut self, pk: NOrmValue) -> Result<E, Error> {
        match self.find_one::<E>(pk.clone())? {
            Some(entity) => Ok(entity),
            None => Err(Error::new(
                ErrorKind::NotFound,
                format!("{} 中未找到主键 {}", E::TABLE, pk),
            )),
        }
    }

    /// 按主键查找，未找到返回 Ok(None)。
    pub fn find_one<E: NOrmEntity>(&mut self, pk: NOrmValue) -> Result<Option<E>, Error> {
        let mut query = self.query::<E>();
        query.r#where(Self::pk_column::<E>(), pk);
        query.one()
    }

    pub fn find_all<E: NOrmEntity>(&mut self) -> Result<Vec<E>, Error> {
        self.query::<E>().all()
    }

    /// 按主键更新，返回受影响行数。
    ///
    /// 声明 `created_at` 时该列从 SET 中排除（保护数据库里的原始创建时间）；
    /// 声明 `updated_at` 时注入当前 UTC 时间，覆盖实体自带值。
    /// 启用软删除时只影响尚未删除的行。
    pub fn update<E: NOrmEntity>(&mut self, entity: &E) -> Result<usize, Error> {
        let pk_name = Self::pk_column::<E>();
        let columns = E::COLUMNS
            .iter()
            .filter(|c| !c.primary_key)
            // created_at 不参与 SET，避免把库中原值清成 Null
            .filter(|c| Some(c.name) != E::CREATED_AT_COL)
            .map(|c| c.name)
            .collect::<Vec<_>>();
        if columns.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{} 没有可更新的列", E::TABLE),
            ));
        }
        let assignments = columns
            .iter()
            .map(|c| format!("{c} = ?"))
            .collect::<Vec<_>>();
        let soft_guard = match E::SOFT_DELETE_COL {
            Some(name) => Some(validate_column::<E>(name)?),
            None => None,
        };
        let where_sql = match &soft_guard {
            Some(c) => format!("{pk_name} = ? AND {} IS NULL", c.name),
            None => format!("{pk_name} = ?"),
        };
        let sql = format!(
            "UPDATE {} SET {} WHERE {}",
            E::TABLE,
            assignments.join(", "),
            where_sql
        );
        let now = NOrmValue::NOrmDateTime(NOrmDateTime::now_utc());
        let mut values = columns
            .iter()
            .map(|name| {
                if Some(*name) == E::UPDATED_AT_COL {
                    return Ok(now.clone());
                }
                entity.column_value(name)
            })
            .collect::<Result<Vec<_>, _>>()?;
        values.push(entity.pk_value());
        self.driver.execute(&sql, &values)
    }

    /// 按主键删除，返回受影响行数。
    ///
    /// 实体声明 `soft_delete` 时改执行软删除（`UPDATE ... SET deleted_at = now()`，
    /// 只影响尚未删除的行）；未声明时走物理删除。
    pub fn delete<E: NOrmEntity>(&mut self, pk: NOrmValue) -> Result<usize, Error> {
        if let Some(name) = E::SOFT_DELETE_COL {
            let col = validate_column::<E>(name)?;
            let sql = format!(
                "UPDATE {} SET {} = ? WHERE {} = ? AND {} IS NULL",
                E::TABLE,
                col.name,
                Self::pk_column::<E>(),
                col.name
            );
            let now = NOrmValue::NOrmDateTime(NOrmDateTime::now_utc());
            return self.driver.execute(&sql, &[now, pk]);
        }
        self.hard_delete::<E>(pk)
    }

    /// 物理删除，绕过软删除，不可恢复。
    pub fn hard_delete<E: NOrmEntity>(&mut self, pk: NOrmValue) -> Result<usize, Error> {
        let sql = format!(
            "DELETE FROM {} WHERE {} = ?",
            E::TABLE,
            Self::pk_column::<E>()
        );
        self.driver.execute(&sql, &[pk])
    }

    pub fn query<E: NOrmEntity>(&mut self) -> NOrmQueryBuilder<'_, E, D> {
        NOrmQueryBuilder::new(self.driver)
    }

    /// 懒加载：为已取出的单个实体按需填充关联字段，支持点分路径（`books.comments`）。
    ///
    /// 复用预加载的两阶段查询与目标主键去重。每个实体一次关联查询，逐个调用会
    /// 产生 N+1——适合「已取出的单个实体补关联」，批量场景请用 `query().preload(..)`。
    pub fn load_relation<E: NOrmPreload>(
        &mut self,
        entity: &mut E,
        name: &str,
    ) -> Result<(), Error> {
        let path: Vec<&str> = name.split('.').collect();
        if path.iter().any(|s| s.is_empty()) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("预加载路径 {name} 含空段"),
            ));
        }
        E::preload_path(std::slice::from_mut(entity), &path, &mut *self.driver)
    }

    /// 单层事务。嵌套调用返回 InvalidInput；Err 路径必 rollback。
    pub fn transaction<F, T>(&mut self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&mut Self) -> Result<T, Error>,
    {
        if self.in_transaction {
            return Err(Error::new(ErrorKind::InvalidInput, "不支持嵌套事务"));
        }
        self.driver.begin()?;
        self.in_transaction = true;
        match f(self) {
            Ok(value) => {
                let result = self.driver.commit();
                self.in_transaction = false;
                result?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.driver.rollback(); // 忽略 rollback 自身的错误，原始错误优先
                self.in_transaction = false;
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::row::NOrmOwnedRow;
    use crate::NOrmEntity;

    #[allow(dead_code)]
    #[derive(Debug, NOrmEntity)]
    #[entity(table = "items")]
    struct Item {
        #[entity(primary_key, column = "id")]
        id: i64,
        #[entity(column = "name")]
        name: String,
        #[entity(column = "qty")]
        qty: i64,
    }

    #[allow(dead_code)]
    #[derive(Debug, NOrmEntity)]
    #[entity(table = "notes")]
    struct Note {
        #[entity(primary_key, column = "id")]
        id: i64,
        #[entity(column = "body")]
        body: String,
        #[entity(soft_delete, column = "deleted_at")]
        deleted_at: Option<NOrmDateTime>,
        #[entity(created_at)]
        created_at: NOrmDateTime,
        #[entity(updated_at)]
        updated_at: NOrmDateTime,
    }

    /// 记录 execute 的假驱动，upsert_sql 可切换以支持/不支持方言。
    #[derive(Default)]
    struct Spy {
        calls: Vec<(String, Vec<NOrmValue>)>,
        upsert: bool,
    }

    impl NOrmDriver for Spy {
        fn execute(&mut self, sql: &str, params: &[NOrmValue]) -> Result<usize, Error> {
            self.calls.push((sql.to_string(), params.to_vec()));
            Ok(params.len() / 2) // 每“行”约等于参数数/列数的近似，够测试用
        }
        fn query(&mut self, _sql: &str, _params: &[NOrmValue]) -> Result<Vec<NOrmOwnedRow>, Error> {
            Ok(Vec::new())
        }
        fn last_insert_id(&self) -> Result<i64, Error> {
            Ok(1)
        }
        fn begin(&mut self) -> Result<(), Error> {
            Ok(())
        }
        fn commit(&mut self) -> Result<(), Error> {
            Ok(())
        }
        fn rollback(&mut self) -> Result<(), Error> {
            Ok(())
        }
        fn upsert_sql(
            &self,
            _table: &str,
            _pk: &str,
            update_cols: &[&str],
        ) -> Result<String, Error> {
            if !self.upsert {
                return Err(Error::new(ErrorKind::InvalidInput, "unsupported"));
            }
            let sets = update_cols
                .iter()
                .map(|c| format!("{c} = excluded.{c}"))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!(" ON CONFLICT(id) DO UPDATE SET {sets}"))
        }
    }

    fn item(id: i64, name: &str, qty: i64) -> Item {
        Item { id, name: name.into(), qty }
    }

    #[test]
    fn insert_batch_empty_is_noop() {
        let mut drv = Spy::default();
        {
            let mut repo = NOrmRepository::new(&mut drv);
            assert_eq!(repo.insert_batch::<Item>(&[]).unwrap(), 0);
        }
        assert!(drv.calls.is_empty());
    }

    #[test]
    fn insert_batch_single_row_sql_and_params() {
        let mut drv = Spy::default();
        {
            let mut repo = NOrmRepository::new(&mut drv);
            assert_eq!(repo.insert_batch(&[item(9, "a", 3)]).unwrap(), 1);
        }
        let (sql, params) = &drv.calls[0];
        assert_eq!(sql, "INSERT INTO items (name, qty) VALUES (?, ?)");
        assert_eq!(params, &vec![NOrmValue::Text("a".into()), NOrmValue::I64(3)]);
        assert_eq!(sql.matches('?').count(), params.len());
    }

    #[test]
    fn insert_batch_multi_row_and_chunks() {
        let mut drv = Spy::default();
        let rows = (0..3).map(|i| item(i, "x", i)).collect::<Vec<_>>();
        {
            let mut repo = NOrmRepository::new(&mut drv);
            repo.insert_batch(&rows).unwrap();
        }
        assert_eq!(drv.calls.len(), 1);
        let (sql, params) = &drv.calls[0];
        assert_eq!(sql, "INSERT INTO items (name, qty) VALUES (?, ?), (?, ?), (?, ?)");
        assert_eq!(params.len(), 6);
        assert_eq!(sql.matches('?').count(), params.len());
    }

    #[test]
    fn insert_batch_respects_param_limit() {
        // 每行 2 列，BATCH_MAX_PARAMS=900 → 每块 450 行；901 行分两块。
        let mut drv = Spy::default();
        let rows = (0..451).map(|i| item(i, "x", i)).collect::<Vec<_>>();
        {
            let mut repo = NOrmRepository::new(&mut drv);
            repo.insert_batch(&rows).unwrap();
        }
        assert_eq!(drv.calls.len(), 2);
        for (sql, params) in &drv.calls {
            assert!(params.len() <= BATCH_MAX_PARAMS);
            assert_eq!(sql.matches('?').count(), params.len());
        }
    }

    #[test]
    fn insert_batch_managed_columns_injected() {
        let mut drv = Spy::default();
        let note = Note {
            id: 0,
            body: "hi".into(),
            deleted_at: Some(NOrmDateTime::new(2000, 1, 1, 0, 0, 0, 0).unwrap()),
            created_at: NOrmDateTime::new(2000, 1, 1, 0, 0, 0, 0).unwrap(),
            updated_at: NOrmDateTime::new(2000, 1, 1, 0, 0, 0, 0).unwrap(),
        };
        {
            let mut repo = NOrmRepository::new(&mut drv);
            repo.insert_batch(&[note]).unwrap();
        }
        let (sql, params) = &drv.calls[0];
        assert_eq!(
            sql,
            "INSERT INTO notes (body, deleted_at, created_at, updated_at) VALUES (?, ?, ?, ?)"
        );
        assert_eq!(params[0], NOrmValue::Text("hi".into()));
        assert_eq!(params[1], NOrmValue::Null); // 软删列强制 NULL
        assert!(matches!(params[2], NOrmValue::NOrmDateTime(_))); // created_at 注入
        assert!(matches!(params[3], NOrmValue::NOrmDateTime(_))); // updated_at 注入
        assert_eq!(params[2], params[3]); // 同一 now
    }

    #[test]
    fn upsert_batch_uses_dialect_suffix_and_excludes_managed() {
        let mut drv = Spy { upsert: true, ..Default::default() };
        {
            let mut repo = NOrmRepository::new(&mut drv);
            repo.upsert_batch(&[item(1, "a", 2), item(2, "b", 3)]).unwrap();
        }
        let (sql, params) = &drv.calls[0];
        assert_eq!(
            sql,
            "INSERT INTO items (id, name, qty) VALUES (?, ?, ?), (?, ?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name, qty = excluded.qty"
        );
        assert_eq!(params.len(), 6);
    }

    #[test]
    fn upsert_batch_note_excludes_soft_delete_and_created_at() {
        let mut drv = Spy { upsert: true, ..Default::default() };
        let note = Note {
            id: 0,
            body: "hi".into(),
            deleted_at: None,
            created_at: NOrmDateTime::new(2000, 1, 1, 0, 0, 0, 0).unwrap(),
            updated_at: NOrmDateTime::new(2000, 1, 1, 0, 0, 0, 0).unwrap(),
        };
        {
            let mut repo = NOrmRepository::new(&mut drv);
            repo.upsert_batch(&[note]).unwrap();
        }
        let (sql, _) = &drv.calls[0];
        // 更新列排除 deleted_at 与 created_at，保留 updated_at
        assert!(sql.contains("body = excluded.body"));
        assert!(sql.contains("updated_at = excluded.updated_at"));
        assert!(!sql.contains("deleted_at = excluded"));
        assert!(!sql.contains("created_at = excluded"));
    }

    #[test]
    fn upsert_batch_unsupported_driver_errors() {
        let mut drv = Spy::default(); // upsert=false
        let mut repo = NOrmRepository::new(&mut drv);
        let err = repo.upsert_batch(&[item(1, "a", 2)]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(drv.calls.is_empty(), "方言不支持时不应发出 INSERT");
    }
}
