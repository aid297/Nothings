//! Repository：CRUD 与事务。

use std::io::{Error, ErrorKind};

use crate::driver::Driver;
use crate::entity::Entity;
use crate::query::QueryBuilder;
use crate::value::Value;

/// 泛型仓储，持有驱动的可变借用。
pub struct Repository<'a, D: Driver> {
    driver: &'a mut D,
    in_transaction: bool,
}

impl<'a, D: Driver> Repository<'a, D> {
    pub fn new(driver: &'a mut D) -> Self {
        Self {
            driver,
            in_transaction: false,
        }
    }

    fn pk_column<E: Entity>() -> &'static str {
        E::pk().name
    }

    /// 插入一行，返回主键。
    pub fn insert<E: Entity>(&mut self, entity: &E) -> Result<i64, Error> {
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
        let values = columns
            .iter()
            .map(|name| entity.column_value(name))
            .collect::<Result<Vec<_>, _>>()?;
        self.driver.execute(&sql, &values)?;
        self.driver.last_insert_id()
    }

    /// 按主键查找，未找到返回 NotFound。
    pub fn get<E: Entity>(&mut self, pk: Value) -> Result<E, Error> {
        match self.find_one::<E>(pk.clone())? {
            Some(entity) => Ok(entity),
            None => Err(Error::new(
                ErrorKind::NotFound,
                format!("{} 中未找到主键 {}", E::TABLE, pk),
            )),
        }
    }

    /// 按主键查找，未找到返回 Ok(None)。
    pub fn find_one<E: Entity>(&mut self, pk: Value) -> Result<Option<E>, Error> {
        let mut query = self.query::<E>();
        query.r#where(Self::pk_column::<E>(), pk);
        query.one()
    }

    pub fn find_all<E: Entity>(&mut self) -> Result<Vec<E>, Error> {
        self.query::<E>().all()
    }

    /// 按主键更新，返回受影响行数。
    pub fn update<E: Entity>(&mut self, entity: &E) -> Result<usize, Error> {
        let pk_name = Self::pk_column::<E>();
        let columns = E::COLUMNS
            .iter()
            .filter(|c| !c.primary_key)
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
        let sql = format!(
            "UPDATE {} SET {} WHERE {} = ?",
            E::TABLE,
            assignments.join(", "),
            pk_name
        );
        let mut values = columns
            .iter()
            .map(|name| entity.column_value(name))
            .collect::<Result<Vec<_>, _>>()?;
        values.push(entity.pk_value());
        self.driver.execute(&sql, &values)
    }

    /// 按主键删除，返回受影响行数。
    pub fn delete<E: Entity>(&mut self, pk: Value) -> Result<usize, Error> {
        let sql = format!(
            "DELETE FROM {} WHERE {} = ?",
            E::TABLE,
            Self::pk_column::<E>()
        );
        self.driver.execute(&sql, &[pk])
    }

    pub fn query<E: Entity>(&mut self) -> QueryBuilder<'_, E, D> {
        QueryBuilder::new(self.driver)
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
