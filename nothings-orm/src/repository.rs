//! NOrmRepository：CRUD 与事务。

use std::io::{Error, ErrorKind};

use crate::driver::NOrmDriver;
use crate::entity::{NOrmEntity, NOrmPreload};
use crate::query::NOrmQueryBuilder;
use crate::schema::validate_column;
use crate::value::{NOrmDateTime, NOrmValue};

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
        let columns = E::COLUMNS
            .iter()
            .filter(|c| !c.primary_key)
            .map(|c| c.name)
            .collect::<Vec<_>>();
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
            .map(|name| {
                if Some(*name) == E::SOFT_DELETE_COL {
                    return Ok(NOrmValue::Null);
                }
                if Some(*name) == E::CREATED_AT_COL || Some(*name) == E::UPDATED_AT_COL {
                    return Ok(now.clone());
                }
                entity.column_value(name)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.driver.execute(&sql, &values)?;
        self.driver.last_insert_id()
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
