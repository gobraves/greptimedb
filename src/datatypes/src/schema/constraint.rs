// Copyright 2023 Greptime Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt::{Display, Formatter};
use std::sync::Arc;

use common_time::util;
use serde::{Deserialize, Serialize};
use snafu::{ensure, ResultExt};

use crate::data_type::{ConcreteDataType, DataType};
use crate::error::{self, Result};
use crate::value::Value;
use crate::vectors::{Int64Vector, TimestampMillisecondVector, VectorRef};

const CURRENT_TIMESTAMP: &str = "current_timestamp()";

/// Column's default constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnDefaultConstraint {
    // A function invocation
    // TODO(dennis): we save the function expression here, maybe use a struct in future.
    Function(String),
    // A value
    Value(Value),
}

impl TryFrom<&[u8]> for ColumnDefaultConstraint {
    type Error = error::Error;

    fn try_from(bytes: &[u8]) -> Result<Self> {
        let json = String::from_utf8_lossy(bytes);
        serde_json::from_str(&json).context(error::DeserializeSnafu { json })
    }
}

impl TryFrom<ColumnDefaultConstraint> for Vec<u8> {
    type Error = error::Error;

    fn try_from(value: ColumnDefaultConstraint) -> std::result::Result<Self, Self::Error> {
        let s = serde_json::to_string(&value).context(error::SerializeSnafu)?;
        Ok(s.into_bytes())
    }
}

impl Display for ColumnDefaultConstraint {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ColumnDefaultConstraint::Function(expr) => write!(f, "{expr}"),
            ColumnDefaultConstraint::Value(v) => write!(f, "{v}"),
        }
    }
}

impl ColumnDefaultConstraint {
    /// Returns a default null constraint.
    pub fn null_value() -> ColumnDefaultConstraint {
        ColumnDefaultConstraint::Value(Value::Null)
    }

    /// Check whether the constraint is valid for columns with given `data_type`
    /// and `is_nullable` attributes.
    pub fn validate(&self, data_type: &ConcreteDataType, is_nullable: bool) -> Result<()> {
        ensure!(is_nullable || !self.maybe_null(), error::NullDefaultSnafu);

        match self {
            ColumnDefaultConstraint::Function(expr) => {
                ensure!(
                    expr == CURRENT_TIMESTAMP,
                    error::UnsupportedDefaultExprSnafu { expr }
                );
                ensure!(
                    data_type.is_timestamp_compatible(),
                    error::DefaultValueTypeSnafu {
                        reason: "return value of the function must has timestamp type",
                    }
                );
            }
            ColumnDefaultConstraint::Value(v) => {
                if !v.is_null() {
                    // Whether the value could be nullable has been checked before, only need
                    // to check the type compatibility here.
                    ensure!(
                        data_type.logical_type_id() == v.logical_type_id(),
                        error::DefaultValueTypeSnafu {
                            reason: format!(
                                "column has type {:?} but default value has type {:?}",
                                data_type.logical_type_id(),
                                v.logical_type_id()
                            ),
                        }
                    );
                }
            }
        }

        Ok(())
    }

    /// Create a vector that contains `num_rows` default values for given `data_type`.
    ///
    /// If `is_nullable` is `true`, then this method would returns error if the created
    /// default value is null.
    ///
    /// # Panics
    /// Panics if `num_rows == 0`.
    pub fn create_default_vector(
        &self,
        data_type: &ConcreteDataType,
        is_nullable: bool,
        num_rows: usize,
    ) -> Result<VectorRef> {
        assert!(num_rows > 0);

        match self {
            ColumnDefaultConstraint::Function(expr) => {
                // Functions should also ensure its return value is not null when
                // is_nullable is true.
                match &expr[..] {
                    // TODO(dennis): we only supports current_timestamp right now,
                    //   it's better to use a expression framework in future.
                    CURRENT_TIMESTAMP => create_current_timestamp_vector(data_type, num_rows),
                    _ => error::UnsupportedDefaultExprSnafu { expr }.fail(),
                }
            }
            ColumnDefaultConstraint::Value(v) => {
                ensure!(is_nullable || !v.is_null(), error::NullDefaultSnafu);

                // TODO(yingwen):
                // 1. For null value, we could use NullVector once it supports custom logical type.
                // 2. For non null value, we could use ConstantVector, but it would cause all codes
                //  attempt to downcast the vector fail if they don't check whether the vector is const
                //  first.
                let mut mutable_vector = data_type.create_mutable_vector(1);
                mutable_vector.try_push_value_ref(v.as_value_ref())?;
                let base_vector = mutable_vector.to_vector();
                Ok(base_vector.replicate(&[num_rows]))
            }
        }
    }

    /// Returns true if this constraint might creates NULL.
    fn maybe_null(&self) -> bool {
        // Once we support more functions, we may return true if given function
        // could return null.
        matches!(self, ColumnDefaultConstraint::Value(Value::Null))
    }
}

fn create_current_timestamp_vector(
    data_type: &ConcreteDataType,
    num_rows: usize,
) -> Result<VectorRef> {
    // FIXME(yingwen): We should implements cast in VectorOp so we could cast the millisecond vector
    // to other data type and avoid this match.
    match data_type {
        ConcreteDataType::Timestamp(_) => Ok(Arc::new(TimestampMillisecondVector::from_values(
            std::iter::repeat(util::current_time_millis()).take(num_rows),
        ))),
        ConcreteDataType::Int64(_) => Ok(Arc::new(Int64Vector::from_values(
            std::iter::repeat(util::current_time_millis()).take(num_rows),
        ))),
        _ => error::DefaultValueTypeSnafu {
            reason: format!("Not support to assign current timestamp to {data_type:?} type",),
        }
        .fail(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::vectors::Int32Vector;

    #[test]
    fn test_null_default_constraint() {
        let constraint = ColumnDefaultConstraint::null_value();
        assert!(constraint.maybe_null());
        let constraint = ColumnDefaultConstraint::Value(Value::Int32(10));
        assert!(!constraint.maybe_null());
    }

    #[test]
    fn test_validate_null_constraint() {
        let constraint = ColumnDefaultConstraint::null_value();
        let data_type = ConcreteDataType::int32_datatype();
        assert!(constraint.validate(&data_type, false).is_err());
        constraint.validate(&data_type, true).unwrap();
    }

    #[test]
    fn test_validate_value_constraint() {
        let constraint = ColumnDefaultConstraint::Value(Value::Int32(10));
        let data_type = ConcreteDataType::int32_datatype();
        constraint.validate(&data_type, false).unwrap();
        constraint.validate(&data_type, true).unwrap();

        assert!(constraint
            .validate(&ConcreteDataType::uint32_datatype(), true)
            .is_err());
    }

    #[test]
    fn test_validate_function_constraint() {
        let constraint = ColumnDefaultConstraint::Function(CURRENT_TIMESTAMP.to_string());
        constraint
            .validate(&ConcreteDataType::timestamp_millisecond_datatype(), false)
            .unwrap();
        assert!(constraint
            .validate(&ConcreteDataType::boolean_datatype(), false)
            .is_err());

        let constraint = ColumnDefaultConstraint::Function("hello()".to_string());
        assert!(constraint
            .validate(&ConcreteDataType::timestamp_millisecond_datatype(), false)
            .is_err());
    }

    #[test]
    fn test_create_default_vector_by_null() {
        let constraint = ColumnDefaultConstraint::null_value();
        let data_type = ConcreteDataType::int32_datatype();
        assert!(constraint
            .create_default_vector(&data_type, false, 10)
            .is_err());

        let constraint = ColumnDefaultConstraint::null_value();
        let v = constraint
            .create_default_vector(&data_type, true, 3)
            .unwrap();
        assert_eq!(3, v.len());
        for i in 0..v.len() {
            assert_eq!(Value::Null, v.get(i));
        }
    }

    #[test]
    fn test_create_default_vector_by_value() {
        let constraint = ColumnDefaultConstraint::Value(Value::Int32(10));
        let data_type = ConcreteDataType::int32_datatype();
        let v = constraint
            .create_default_vector(&data_type, false, 4)
            .unwrap();
        let expect: VectorRef = Arc::new(Int32Vector::from_values(vec![10; 4]));
        assert_eq!(expect, v);
    }

    #[test]
    fn test_create_default_vector_by_func() {
        let constraint = ColumnDefaultConstraint::Function(CURRENT_TIMESTAMP.to_string());
        // Timestamp type.
        let data_type = ConcreteDataType::timestamp_millisecond_datatype();
        let v = constraint
            .create_default_vector(&data_type, false, 4)
            .unwrap();
        assert_eq!(4, v.len());
        assert!(
            matches!(v.get(0), Value::Timestamp(_)),
            "v {:?} is not timestamp",
            v.get(0)
        );

        // Int64 type.
        let data_type = ConcreteDataType::int64_datatype();
        let v = constraint
            .create_default_vector(&data_type, false, 4)
            .unwrap();
        assert_eq!(4, v.len());
        assert!(
            matches!(v.get(0), Value::Int64(_)),
            "v {:?} is not timestamp",
            v.get(0)
        );

        let constraint = ColumnDefaultConstraint::Function("no".to_string());
        let data_type = ConcreteDataType::timestamp_millisecond_datatype();
        assert!(constraint
            .create_default_vector(&data_type, false, 4)
            .is_err());
    }

    #[test]
    fn test_create_by_func_and_invalid_type() {
        let constraint = ColumnDefaultConstraint::Function(CURRENT_TIMESTAMP.to_string());
        let data_type = ConcreteDataType::boolean_datatype();
        let err = constraint
            .create_default_vector(&data_type, false, 4)
            .unwrap_err();
        assert!(matches!(err, Error::DefaultValueType { .. }), "{err:?}");
    }
}
