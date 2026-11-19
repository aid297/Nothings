//! 结果集行的抽象与 owned 实现。
//!
//! `NOrmDriver::query` 必须即时把数据库的 borrowed 行物化为 `NOrmOwnedRow`，
//! 这样行数据才不会受语句生命周期约束，可以安全地交给 `NOrmEntity::from_row`。

use std::io::{Error, ErrorKind};

use crate::value::NOrmValue;

/// 结果集行的只读视图。
pub trait NOrmRow {
    fn column_count(&self) -> usize;
    fn column_name(&self, index: usize) -> &str;
    fn value(&self, index: usize) -> Result<NOrmValue, Error>;
}

/// 拥有所有权的行，列名与列值都是克隆出来的。
#[derive(Clone, Debug, PartialEq)]
pub struct NOrmOwnedRow {
    names: Vec<String>,
    values: Vec<NOrmValue>,
}

impl NOrmOwnedRow {
    pub fn new(names: Vec<String>, values: Vec<NOrmValue>) -> Self {
        assert_eq!(
            names.len(),
            values.len(),
            "NOrmOwnedRow: 列名与列值数量必须一致"
        );
        Self { names, values }
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn values(&self) -> &[NOrmValue] {
        &self.values
    }
}

impl NOrmRow for NOrmOwnedRow {
    fn column_count(&self) -> usize {
        self.names.len()
    }

    fn column_name(&self, index: usize) -> &str {
        &self.names[index]
    }

    fn value(&self, index: usize) -> Result<NOrmValue, Error> {
        self.values.get(index).cloned().ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidInput,
                format!("列索引 {index} 越界（共 {} 列）", self.values.len()),
            )
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_owned_row() {
        let row = NOrmOwnedRow::new(
            vec!["id".into(), "name".into()],
            vec![NOrmValue::I64(1), NOrmValue::Text("alice".into())],
        );
        assert_eq!(row.column_count(), 2);
        assert_eq!(row.column_name(1), "name");
        assert_eq!(row.value(0).unwrap(), NOrmValue::I64(1));
        assert_eq!(row.value(9).unwrap_err().kind(), ErrorKind::InvalidInput);
    }
}
