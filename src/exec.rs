// Copyright 2018 Grove Enterprises LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Query execution

use std::cell::RefCell;
use std::clone::Clone;
use std::collections::HashMap;
use std::collections::HashSet;
use std::convert::*;
use std::fs::File;
use std::io::BufWriter;
use std::iter::Iterator;
use std::rc::Rc;
use std::str;
use std::string::String;

use arrow::array::*;
use arrow::builder::*;
use arrow::datatypes::*;

//use futures::{Future, Stream};
//use hyper::Client;
//use tokio_core::reactor::Core;
//use hyper::{Method, Request};
//use hyper::header::{ContentLength, ContentType};

use super::dataframe::*;
use super::datasources::common::*;
use super::datasources::csv::*;
use super::datasources::parquet::*;
use super::errors::*;
use super::logical::*;
use super::relations::aggregate::*;
use super::relations::filter::*;
use super::relations::projection::*;
use super::sqlast::ASTNode::*;
use super::sqlparser::*;
use super::sqlplanner::*;
use super::types::*;
//use super::cluster::*;

#[derive(Debug, Clone)]
pub enum DFConfig {
    Local,
    Remote { etcd: String },
}

//#[derive(Debug)]
//pub enum ExecutionError {
//    IoError(Error),
//    ExecError(ExecutionError),
//    InvalidColumn(String),
//    NotImplemented,
//}

//impl From<ExecutionError> for ExecutionError {
//    fn from(e: ExecutionError) -> Self {
//        ExecutionError::ExecError(e)
//    }
//}
//
//impl From<Error> for ExecutionError {
//    fn from(e: Error) -> Self {
//        ExecutionError::IoError(e)
//    }
//}


macro_rules! compare_arrays_inner {
    ($V1:ident, $V2:ident, $F:expr) => {
        match ($V1.data(), $V2.data()) {
            (&ArrayData::Float32(ref a), &ArrayData::Float32(ref b)) =>
                Ok(a.iter().zip(b.iter()).map($F).collect::<Vec<bool>>()),
            (&ArrayData::Float64(ref a), &ArrayData::Float64(ref b)) =>
                Ok(a.iter().zip(b.iter()).map($F).collect::<Vec<bool>>()),
            (&ArrayData::Int8(ref a), &ArrayData::Int8(ref b)) =>
                Ok(a.iter().zip(b.iter()).map($F).collect::<Vec<bool>>()),
            (&ArrayData::Int16(ref a), &ArrayData::Int16(ref b)) =>
                Ok(a.iter().zip(b.iter()).map($F).collect::<Vec<bool>>()),
            (&ArrayData::Int32(ref a), &ArrayData::Int32(ref b)) =>
                Ok(a.iter().zip(b.iter()).map($F).collect::<Vec<bool>>()),
            (&ArrayData::Int64(ref a), &ArrayData::Int64(ref b)) =>
                Ok(a.iter().zip(b.iter()).map($F).collect::<Vec<bool>>()),
            //(&ArrayData::Utf8(ref a), &ScalarValue::Utf8(ref b)) => a.iter().map(|n| n > b).collect(),
            _ => Err(ExecutionError::General("Unsupported types in compare_arrays_inner".to_string()))
        }
    }
}

macro_rules! compare_arrays {
    ($V1:ident, $V2:ident, $F:expr) => {
        Ok(Value::Column(Rc::new(Array::from(
            compare_arrays_inner!($V1, $V2, $F)?,
        ))))
    };
}

macro_rules! compare_array_with_scalar_inner {
    ($V1:ident, $V2:ident, $F:expr) => {
        match ($V1.data(), $V2.as_ref()) {
            (&ArrayData::Float32(ref a), &ScalarValue::Float32(b)) => {
                Ok(a.iter().map(|aa| (aa, b)).map($F).collect::<Vec<bool>>())
            }
            (&ArrayData::Float64(ref a), &ScalarValue::Float64(b)) => {
                Ok(a.iter().map(|aa| (aa, b)).map($F).collect::<Vec<bool>>())
            }
            _ => Err(ExecutionError::General(
                "Unsupported types in compare_array_with_scalar_inner".to_string(),
            )),
        }
    };
}

macro_rules! compare_array_with_scalar {
    ($V1:ident, $V2:ident, $F:expr) => {
        Ok(Value::Column(Rc::new(Array::from(
            compare_array_with_scalar_inner!($V1, $V2, $F)?,
        ))))
    };
}

impl Value {
    pub fn eq(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (&Value::Column(ref v1), &Value::Column(ref v2)) => {
                compare_arrays!(v1, v2, |(aa, bb)| aa == bb)
            }
            (&Value::Column(ref v1), &Value::Scalar(ref v2)) => match (v1.data(), v2.as_ref()) {
                (&ArrayData::Utf8(ref list), &ScalarValue::Utf8(ref b)) => {
                    let mut v: Vec<bool> = Vec::with_capacity(list.len() as usize);
                    for i in 0..list.len() as usize {
                        v.push(list.slice(i) == b.as_bytes());
                    }
                    Ok(Value::Column(Rc::new(Array::from(v))))
                }
                _ => compare_array_with_scalar!(v1, v2, |(aa, bb)| aa != bb),
            },
            (&Value::Scalar(ref v1), &Value::Column(ref v2)) => {
                compare_array_with_scalar!(v2, v1, |(aa, bb)| aa == bb)
            }
            (&Value::Scalar(ref _v1), &Value::Scalar(ref _v2)) => unimplemented!(),
        }
    }

    pub fn not_eq(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (&Value::Column(ref v1), &Value::Column(ref v2)) => {
                compare_arrays!(v1, v2, |(aa, bb)| aa != bb)
            }
            (&Value::Column(ref v1), &Value::Scalar(ref v2)) => match (v1.data(), v2.as_ref()) {
                (&ArrayData::Utf8(ref list), &ScalarValue::Utf8(ref b)) => {
                    let mut v: Vec<bool> = Vec::with_capacity(list.len() as usize);
                    for i in 0..list.len() as usize {
                        v.push(list.slice(i) != b.as_bytes());
                    }
                    Ok(Value::Column(Rc::new(Array::from(v))))
                }
                _ => compare_array_with_scalar!(v1, v2, |(aa, bb)| aa != bb),
            },
            (&Value::Scalar(ref v1), &Value::Column(ref v2)) => {
                compare_array_with_scalar!(v2, v1, |(aa, bb)| aa != bb)
            }
            (&Value::Scalar(ref _v1), &Value::Scalar(ref _v2)) => unimplemented!(),
        }
    }

    pub fn lt(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (&Value::Column(ref v1), &Value::Column(ref v2)) => {
                compare_arrays!(v1, v2, |(aa, bb)| aa < bb)
            }
            (&Value::Column(ref v1), &Value::Scalar(ref v2)) => {
                compare_array_with_scalar!(v1, v2, |(aa, bb)| aa < bb)
            }
            (&Value::Scalar(ref v1), &Value::Column(ref v2)) => {
                compare_array_with_scalar!(v2, v1, |(aa, bb)| aa < bb)
            }
            (&Value::Scalar(ref _v1), &Value::Scalar(ref _v2)) => unimplemented!(),
        }
    }

    pub fn lt_eq(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (&Value::Column(ref v1), &Value::Column(ref v2)) => {
                compare_arrays!(v1, v2, |(aa, bb)| aa <= bb)
            }
            (&Value::Column(ref v1), &Value::Scalar(ref v2)) => {
                compare_array_with_scalar!(v1, v2, |(aa, bb)| aa <= bb)
            }
            (&Value::Scalar(ref v1), &Value::Column(ref v2)) => {
                compare_array_with_scalar!(v2, v1, |(aa, bb)| aa <= bb)
            }
            (&Value::Scalar(ref _v1), &Value::Scalar(ref _v2)) => unimplemented!(),
        }
    }

    pub fn gt(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (&Value::Column(ref v1), &Value::Column(ref v2)) => {
                compare_arrays!(v1, v2, |(aa, bb)| aa >= bb)
            }
            (&Value::Column(ref v1), &Value::Scalar(ref v2)) => {
                compare_array_with_scalar!(v1, v2, |(aa, bb)| aa >= bb)
            }
            (&Value::Scalar(ref v1), &Value::Column(ref v2)) => {
                compare_array_with_scalar!(v2, v1, |(aa, bb)| aa >= bb)
            }
            (&Value::Scalar(ref _v1), &Value::Scalar(ref _v2)) => unimplemented!(),
        }
    }

    pub fn gt_eq(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (&Value::Column(ref v1), &Value::Column(ref v2)) => {
                compare_arrays!(v1, v2, |(aa, bb)| aa > bb)
            }
            (&Value::Column(ref v1), &Value::Scalar(ref v2)) => {
                compare_array_with_scalar!(v1, v2, |(aa, bb)| aa > bb)
            }
            (&Value::Scalar(ref v1), &Value::Column(ref v2)) => {
                compare_array_with_scalar!(v2, v1, |(aa, bb)| aa > bb)
            }
            (&Value::Scalar(ref _v1), &Value::Scalar(ref _v2)) => unimplemented!(),
        }
    }

    pub fn add(&self, _other: &Value) -> Result<Value> {
        unimplemented!()
    }
    pub fn subtract(&self, _other: &Value) -> Result<Value> {
        unimplemented!()
    }
    pub fn divide(&self, _other: &Value) -> Result<Value> {
        unimplemented!()
    }
    pub fn multiply(&self, _other: &Value) -> Result<Value> {
        unimplemented!()
    }

    pub fn and(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (&Value::Column(ref v1), &Value::Column(ref v2)) => {
                match (v1.data(), v2.data()) {
                    (ArrayData::Boolean(ref l), ArrayData::Boolean(ref r)) => {
                        let bools = l.iter()
                            .zip(r.iter())
                            .map(|(ll, rr)| ll && rr)
                            .collect::<Vec<bool>>();
                        //                        println!("AND: left = {:?}", l.iter().collect::<Vec<bool>>());
                        //                        println!("AND: right = {:?}", r.iter().collect::<Vec<bool>>());
                        //                        println!("AND: bools = {:?}", bools);
                        let bools = Array::from(bools);
                        Ok(Value::Column(Rc::new(bools)))
                    }
                    _ => panic!(),
                }
            }
            (&Value::Column(ref v1), &Value::Scalar(ref v2)) => match (v1.data(), v2.as_ref()) {
                (ArrayData::Boolean(ref l), ScalarValue::Boolean(r)) => {
                    let bools = Array::from(l.iter().map(|ll| ll && *r).collect::<Vec<bool>>());
                    Ok(Value::Column(Rc::new(bools)))
                }
                _ => panic!(),
            },
            //            (&Value::Scalar(ref v1), &Value::Column(ref v2)) => {
            //                compare_array_with_scalar!(v2, v1, |(aa, bb)| aa && bb)
            //            }
            //            (&Value::Scalar(ref _v1), &Value::Scalar(ref _v2)) => unimplemented!(),
            _ => panic!(),
        }
    }

    pub fn or(&self, _other: &Value) -> Result<Value> {
        unimplemented!()
    }
}

/// Compiled Expression (basically just a closure to evaluate the expression at runtime)
pub type CompiledExpr = Box<Fn(&RecordBatch) -> Result<Value>>;

pub type CompiledCastFunction = Box<Fn(&Value) -> Result<Value>>;

pub enum AggregateType {
    Min,
    Max,
    Sum,
    Count,
    Avg,
    //CountDistinct()
}

/// Runtime expression
pub enum RuntimeExpr {
    Compiled(CompiledExpr),
    AggregateFunction {
        func: AggregateType,
        args: Vec<CompiledExpr>,
        return_type: DataType,
    },
}

/// Compiles a scalar expression into a closure
pub fn compile_expr(
    ctx: &ExecutionContext,
    expr: &Expr,
    input_schema: &Schema,
) -> Result<RuntimeExpr> {
    match *expr {
        Expr::AggregateFunction { ref name, ref args } => {
            assert_eq!(1, args.len());

            let compiled_args: Result<Vec<CompiledExpr>> =
                args.iter().map(|e| compile_scalar_expr(ctx, e)).collect();

            let func = match name.to_lowercase().as_ref() {
                "min" => AggregateType::Min,
                "max" => AggregateType::Max,
                "count" => AggregateType::Count,
                _ => unimplemented!("Unsupported aggregate function '{}'", name),
            };

            //TODO: this is hacky
            let return_type = match func {
                AggregateType::Count => DataType::UInt64,
                AggregateType::Min | AggregateType::Max => match args[0] {
                    Expr::Column(i) => input_schema.columns()[i].data_type().clone(),
                    _ => {
                        //TODO: fix this hack
                        DataType::Float64
                        //panic!("Aggregate expressions currently only support simple arguments")
                    }
                }
                _ => panic!()
            };


            Ok(RuntimeExpr::AggregateFunction {
                func,
                args: compiled_args?,
                return_type,
            })
        }
        _ => Ok(RuntimeExpr::Compiled(compile_scalar_expr(ctx, expr)?)),
    }
}

macro_rules! cast_utf8_to {
    {$TY:ty, $LIST:expr} => {{
        let mut b: Builder<$TY> = Builder::with_capacity($LIST.len() as usize);
        for i in 0..$LIST.len() as usize {
            let x = str::from_utf8($LIST.slice(i)).unwrap();
            match x.parse::<$TY>() {
                Ok(v) => b.push(v),
                Err(_) => return Err(ExecutionError::General(format!(
                    "Cannot cast Utf8 value '{}' to {}", x, stringify!($TY))))
            }
        }
        Ok(Value::Column(Rc::new(Array::from(b.finish()))))
    }}
}

fn compile_cast_column(data_type: DataType) -> Result<CompiledCastFunction> {
    Ok(Box::new( move|v: &Value| {
        match v {
            Value::Column(ref array) => {
                match array.data() {
                    &ArrayData::Boolean(_) => unimplemented!("CAST from Boolean"),
                    &ArrayData::UInt8(_) => unimplemented!("CAST from UInt8"),
                    &ArrayData::UInt16(_) => unimplemented!("CAST from UInt16"),
                    &ArrayData::UInt32(_) => unimplemented!("CAST from UInt32"),
                    &ArrayData::UInt64(_) => unimplemented!("CAST from UInt64"),
                    &ArrayData::Int8(_) => unimplemented!("CAST from Unt8"),
                    &ArrayData::Int16(_) => unimplemented!("CAST from Unt16"),
                    &ArrayData::Int32(_) => unimplemented!("CAST from Unt32"),
                    &ArrayData::Int64(_) => unimplemented!("CAST from Unt64"),
                    &ArrayData::Float32(_) => unimplemented!("CAST from Float32"),
                    &ArrayData::Float64(_) => unimplemented!("CAST from Float64"),
                    &ArrayData::Struct(_) => unimplemented!("CAST from Struct"),
                    &ArrayData::Utf8(ref list) => {
                        match &data_type {
                            DataType::Boolean => cast_utf8_to!(bool, list),
                            DataType::Int8 => cast_utf8_to!(i8, list),
                            DataType::Int16 => cast_utf8_to!(i16, list),
                            DataType::Int32 => cast_utf8_to!(i32, list),
                            DataType::Int64 => cast_utf8_to!(i64, list),
                            DataType::UInt8 => cast_utf8_to!(u8, list),
                            DataType::UInt16 => cast_utf8_to!(u16, list),
                            DataType::UInt32 => cast_utf8_to!(u32, list),
                            DataType::UInt64 => cast_utf8_to!(u64, list),
                            DataType::Float32 => cast_utf8_to!(f32, list),
                            DataType::Float64 => cast_utf8_to!(f32, list),
                            _ => unimplemented!("CAST from Utf8 to {:?}", data_type)
                        }
                    }
                }
            }
            _ => unimplemented!("CAST from ScalarValue")
        }
    }))


//                    Err(ExecutionError::NotImplemented)
//                }
//                _ => Err(ExecutionError::NotImplemented)
//            }
//        }
//        _ => Err(ExecutionError::NotImplemented)
}

/// Compiles a scalar expression into a closure
pub fn compile_scalar_expr(ctx: &ExecutionContext, expr: &Expr) -> Result<CompiledExpr> {
    match expr {
        &Expr::Literal(ref lit) => {
            let literal_value = lit.clone();
            Ok(Box::new(move |_| {
                // literal values are a bit special - we don't repeat them in a vector
                // because it would be redundant, so we have a single value in a vector instead
                Ok(Value::Scalar(Rc::new(literal_value.clone())))
            }))
        }
        &Expr::Column(index) => Ok(Box::new(move |batch: &RecordBatch| {
            Ok((*batch.column(index)).clone())
        })),
        &Expr::Cast { ref expr, ref data_type } => {
            match expr.as_ref() {
                &Expr::Column(index) => {
                    let compiled_cast_expr = compile_cast_column(data_type.clone())?;
                    Ok(Box::new(move |batch: &RecordBatch| {
                            (compiled_cast_expr)(batch.column(index))
                        }))
                }
                _ => Err(ExecutionError::NotImplemented)
            }
        }
        &Expr::BinaryExpr {
            ref left,
            ref op,
            ref right,
        } => {
            let left_expr = compile_scalar_expr(ctx, left)?;
            let right_expr = compile_scalar_expr(ctx, right)?;
            match op {
                &Operator::Eq => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.eq(&right_values)
                })),
                &Operator::NotEq => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.not_eq(&right_values)
                })),
                &Operator::Lt => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.lt(&right_values)
                })),
                &Operator::LtEq => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.lt_eq(&right_values)
                })),
                &Operator::Gt => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.gt(&right_values)
                })),
                &Operator::GtEq => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.gt_eq(&right_values)
                })),
                &Operator::Plus => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.add(&right_values)
                })),
                &Operator::Minus => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.subtract(&right_values)
                })),
                &Operator::Divide => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.divide(&right_values)
                })),
                &Operator::Multiply => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.multiply(&right_values)
                })),
                &Operator::And => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.and(&right_values)
                })),
                &Operator::Or => Ok(Box::new(move |batch: &RecordBatch| {
                    let left_values = left_expr(batch)?;
                    let right_values = right_expr(batch)?;
                    left_values.or(&right_values)
                })),
                _ => {
                    return Err(ExecutionError::General(format!(
                        "Unsupported binary operator '{:?}'",
                        op
                    )))
                }
            }
        }
        &Expr::Sort { ref expr, .. } => {
            //NOTE sort order is ignored here and is handled during sort execution
            compile_scalar_expr(ctx, expr)
        }
        &Expr::ScalarFunction { ref name, ref args } => {
            ////println!("Executing function {}", name);

            // evaluate the arguments to the function
            let compiled_args: Result<Vec<CompiledExpr>> =
                args.iter().map(|e| compile_scalar_expr(ctx, e)).collect();

            let compiled_args_ok = compiled_args?;

            let func = ctx.load_scalar_function(name.as_ref())?;

            Ok(Box::new(move |batch| {
                let arg_values: Result<Vec<Value>> =
                    compiled_args_ok.iter().map(|expr| expr(batch)).collect();

                func.execute(arg_values?)
            }))
        }
        // aggregate functions don't fit this pattern .. will need to rework this ..
        &Expr::AggregateFunction { .. } => panic!("Aggregate expressions cannot be compiled yet"),
//        &Expr::AggregateFunction { ref name, ref args } => {
//
//            // evaluate the arguments to the function
//            let compiled_args: Result<Vec<CompiledExpr>> =
//                args.iter().map(|e| compile_expr(ctx, e)).collect();
//
//            let compiled_args_ok = compiled_args?;
//
//            Ok(Box::new(move |batch| {
//                let arg_values: Result<Vec<Value>> =
//                    compiled_args_ok.iter().map(|expr| expr(batch)).collect();
//
//                Ok(Rc::new(arg_values?))
//            }))
//        }
    }
}

///// Compiled Expression (basically just a closure to evaluate the expression at runtime)
//pub type CompiledAggregatateExpr = Box<Fn(&RecordBatch, ScalarValue) -> Result<ScalarValue>>;
//
///// Compiles an aggregate expression into a closure
//pub fn compile_aggregate_expr(ctx: &ExecutionContext, expr: &Expr) -> Result<CompiledExpr> {
//    match
//
//}

//pub struct SortRelation {
//    schema: Schema,
//    input: Box<SimpleRelation>,
//    sort_expr: Vec<CompiledExpr>,
//    sort_asc: Vec<bool>
//}

pub struct LimitRelation {
    schema: Rc<Schema>,
    input: Box<SimpleRelation>,
    _limit: usize,
}

/// trait for all relations (a relation is essentially just an iterator over rows with
/// a known schema)
pub trait SimpleRelation {
    /// scan all records in this relation
    fn scan<'a>(&'a mut self) -> Box<Iterator<Item = Result<Rc<RecordBatch>>> + 'a>;

    /// get the schema for this relation
    fn schema<'a>(&'a self) -> &'a Schema;
}

struct DataSourceRelation {
    schema: Schema,
    ds: Rc<RefCell<DataSource>>,
}

impl SimpleRelation for DataSourceRelation {
    fn scan<'a>(&'a mut self) -> Box<Iterator<Item = Result<Rc<RecordBatch>>> + 'a> {
        Box::new(DataSourceIterator::new(self.ds.clone()))
    }

    fn schema<'a>(&'a self) -> &'a Schema {
        &self.schema
    }
}

impl SimpleRelation for LimitRelation {
    fn scan<'a>(&'a mut self) -> Box<Iterator<Item = Result<Rc<RecordBatch>>> + 'a> {
        unimplemented!()
    }

    fn schema<'a>(&'a self) -> &'a Schema {
        self.schema.as_ref()
    }
}

/// Execution plans are sent to worker nodes for execution
#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    /// Run a query and return the results to the client
    Interactive {
        plan: Rc<LogicalPlan>,
    },
    /// Execute a logical plan and write the output to a file
    Write {
        plan: Rc<LogicalPlan>,
        filename: String,
    },
    Show {
        plan: Rc<LogicalPlan>,
        count: usize,
    },
}

#[derive(Debug, Clone)]
pub enum ExecutionResult {
    Unit,
    Count(usize),
}

#[derive(Clone)]
pub struct ExecutionContext {
    schemas: Rc<RefCell<HashMap<String, Rc<Schema>>>>,
    function_meta: Rc<RefCell<HashMap<String, FunctionMeta>>>,
    functions: Rc<RefCell<HashMap<String, Rc<ScalarFunction>>>>,
    aggregate_functions: Rc<RefCell<HashMap<String, Rc<AggregateFunction>>>>,
    config: Rc<DFConfig>,
    tables: Rc<RefCell<HashMap<String, Rc<DataFrame>>>>,
}

impl ExecutionContext {
    pub fn local() -> Self {
        ExecutionContext {
            schemas: Rc::new(RefCell::new(HashMap::new())),
            function_meta: Rc::new(RefCell::new(HashMap::new())),
            functions: Rc::new(RefCell::new(HashMap::new())),
            aggregate_functions: Rc::new(RefCell::new(HashMap::new())),
            tables: Rc::new(RefCell::new(HashMap::new())),
            config: Rc::new(DFConfig::Local),
        }
    }

    pub fn remote(_etcd: String) -> Self {
        unimplemented!("this feature is disabled at the moment")
        //        ExecutionContext {
        //            schemas: Rc::new(RefCell::new(HashMap::new())),
        //            function_meta: Rc::new(RefCell::new(HashMap::new())),
        //            tables: Rc::new(RefCell::new(HashMap::new())),
        //            config: Rc::new(DFConfig::Remote { etcd: etcd }),
        //        }
    }

    pub fn define_schema(&mut self, name: &str, schema: &Schema) {
        self.schemas
            .borrow_mut()
            .insert(name.to_string(), Rc::new(schema.clone()));
    }

    pub fn register_scalar_function(&mut self, func: Rc<ScalarFunction>) {
        let fm = FunctionMeta {
            name: func.name(),
            args: func.args(),
            return_type: func.return_type(),
            function_type: FunctionType::Scalar,
        };

        self.function_meta
            .borrow_mut()
            .insert(func.name().to_lowercase(), fm);

        self.functions
            .borrow_mut()
            .insert(func.name().to_lowercase(), func.clone());
    }

    pub fn register_aggregate_function(&mut self, func: Rc<AggregateFunction>) {
        let fm = FunctionMeta {
            name: func.name(),
            args: func.args(),
            return_type: func.return_type(),
            function_type: FunctionType::Aggregate,
        };

        self.function_meta
            .borrow_mut()
            .insert(func.name().to_lowercase(), fm);

        self.aggregate_functions
            .borrow_mut()
            .insert(func.name().to_lowercase(), func.clone());
    }

    pub fn create_logical_plan(&self, sql: &str) -> Result<Rc<LogicalPlan>> {
        // parse SQL into AST
        let ast = Parser::parse_sql(String::from(sql))?;

        // create a query planner
        let query_planner = SqlToRel::new(self.schemas.clone()); //TODO: pass reference to schemas

        // plan the query (create a logical relational plan)
        Ok(query_planner.sql_to_rel(&ast)?)
    }

    pub fn register(&mut self, table_name: &str, df: Rc<DataFrame>) {
        //println!("Registering table {}", table_name);
        self.tables
            .borrow_mut()
            .insert(table_name.to_string(), df.clone());

        // temp hack
        self.schemas
            .borrow_mut()
            .insert(table_name.to_string(), df.schema().clone());
    }

    pub fn sql(&mut self, sql: &str) -> Result<Rc<DataFrame>> {
        //println!("sql() {}", sql);

        // parse SQL into AST
        let ast = Parser::parse_sql(String::from(sql))?;
        println!("AST: {:?}", ast);

        match ast {
            SQLCreateTable { name, columns } => {
                let fields: Vec<Field> = columns
                    .iter()
                    .map(|c| Field::new(&c.name, convert_data_type(&c.data_type), c.allow_null))
                    .collect();
                let schema = Schema::new(fields);
                self.define_schema(&name, &schema);

                //TODO: not sure what to return here
                Ok(Rc::new(DF::new(self.clone(),
                    Rc::new(LogicalPlan::EmptyRelation {
                        schema: Rc::new(Schema::empty()),
                    })
                )))
            }
            _ => {
                // create a query planner
                let query_planner = SqlToRel::new(self.schemas.clone()); //TODO: pass reference to schemas

                // plan the query (create a logical relational plan)
                let plan = query_planner.sql_to_rel(&ast)?;
                println!("Logical plan: {:?}", plan);

                let new_plan = push_down_projection(&plan, HashSet::new());
                println!("Optimized logical plan: {:?}", new_plan);

                // return the DataFrame
                Ok(Rc::new(DF::new(
                    self.clone(),
                    new_plan,
                )))
            }
        }
    }

    /// Open a CSV file
    ///TODO: this is building a relational plan not an execution plan so shouldn't really be here
    pub fn load_csv(
        &self,
        filename: &str,
        schema: &Schema,
        has_header: bool,
        projection: Option<Vec<usize>>
    ) -> Result<Rc<DataFrame>> {
        let plan = LogicalPlan::CsvFile {
            filename: filename.to_string(),
            schema: Rc::new(schema.clone()),
            has_header,
            projection
        };
        Ok(Rc::new(DF::new(
            self.clone(),
            Rc::new(plan),
        )))
    }

    pub fn load_parquet(&self, filename: &str, projection: Option<Vec<usize>>) -> Result<Rc<DataFrame>> {
        //TODO: can only get schema by assuming file is local and opening it - need catalog!!
        let file = File::open(filename)?;
        let p = ParquetFile::open(file, None)?;

        let plan = LogicalPlan::ParquetFile {
            filename: filename.to_string(),
            schema: p.schema().clone(),
            projection
        };
        Ok(Rc::new(DF::new(
            self.clone(),
            Rc::new(plan),
        )))
    }

    pub fn register_table(&mut self, name: String, schema: Schema) {
        self.schemas
            .borrow_mut()
            .insert(name, Rc::new(schema.clone()));
    }

    pub fn create_execution_plan(&self, plan: &LogicalPlan) -> Result<Box<SimpleRelation>> {
        //println!("Logical plan: {:?}", plan);

        match *plan {
            LogicalPlan::EmptyRelation { .. } => Err(ExecutionError::General(String::from(
                "empty relation is not implemented yet",
            ))),

            LogicalPlan::Sort { .. } => unimplemented!(),

            LogicalPlan::TableScan { ref table_name, ref projection, .. } => {
                //println!("TableScan: {}", table_name);
                match self.tables.borrow().get(table_name) {
                    Some(df) => match projection {
                        Some(p) => {
                            let mut h: HashSet<usize> = HashSet::new();
                            p.iter().for_each(|i| {
                                h.insert(*i);
                            });
                            self.create_execution_plan(&push_down_projection(df.plan(), h))
                        },
                        None => self.create_execution_plan(df.plan())
                    },
                    _ => Err(ExecutionError::General(format!(
                        "No table registered as '{}'",
                        table_name
                    ))),
                }
            }

            LogicalPlan::CsvFile {
                ref filename,
                ref schema,
                ref has_header,
                ref projection
            } => {
                let file = File::open(filename)?;
                let ds = Rc::new(RefCell::new(CsvFile::open(
                    file,
                    schema.clone(),
                    *has_header,
                    projection.clone()
                )?)) as Rc<RefCell<DataSource>>;
                Ok(Box::new(DataSourceRelation {
                    schema: schema.as_ref().clone(),
                    ds,
                }))
            }

            LogicalPlan::ParquetFile {
                ref filename,
                ref schema,
                ref projection
            } => {
                let file = File::open(filename)?;
                let ds = Rc::new(RefCell::new(ParquetFile::open(file, projection.clone())?)) as Rc<RefCell<DataSource>>;
                Ok(Box::new(DataSourceRelation {
                    schema: schema.as_ref().clone(),
                    ds,
                }))
            }

            LogicalPlan::Selection {
                ref expr,
                ref input,
            } => {
                let input_rel = self.create_execution_plan(input)?;
                let rel = FilterRelation::new(input_rel, compile_scalar_expr(&self, expr)?);
                Ok(Box::new(rel))
            }

            LogicalPlan::Projection {
                ref expr,
                ref input,
                ..
            } => {
                let input_rel = self.create_execution_plan(&input)?;

                let project_columns: Vec<Field> = expr_to_field(&expr, input_rel.schema());

                let project_schema = Rc::new(Schema::new(project_columns));

                let compiled_expr: Result<Vec<CompiledExpr>> =
                    expr.iter().map(|e| compile_scalar_expr(&self, e)).collect();

                let rel = ProjectRelation::new(input_rel, compiled_expr?, project_schema);

                Ok(Box::new(rel))
            }

            LogicalPlan::Aggregate {
                ref input,
                ref group_expr,
                ref aggr_expr,
                ..
            } => {
                let input_rel = self.create_execution_plan(&input)?;

                let compiled_group_expr_result: Result<Vec<CompiledExpr>> = group_expr
                    .iter()
                    .map(|e| compile_scalar_expr(&self, e))
                    .collect();
                let compiled_group_expr = compiled_group_expr_result?;

                let compiled_aggr_expr_result: Result<Vec<RuntimeExpr>> = aggr_expr
                    .iter()
                    .map(|e| compile_expr(&self, e, input.schema()))
                    .collect();
                let compiled_aggr_expr = compiled_aggr_expr_result?;

                let rel = AggregateRelation::new(
                    Rc::new(Schema::empty()), //(expr_to_field(&compiled_group_expr, &input_schema))),
                    input_rel,
                    compiled_group_expr,
                    compiled_aggr_expr);

                Ok(Box::new(rel))
            }
            //LogicalPlan::Sort { .. /*ref expr, ref input, ref schema*/ } => {

            //                let input_rel = self.create_execution_plan(data_dir, input)?;
            //
            //                let compiled_expr : Result<Vec<CompiledExpr>> = expr.iter()
            //                    .map(|e| compile_expr(&self,e))
            //                    .collect();
            //
            //                let sort_asc : Vec<bool> = expr.iter()
            //                    .map(|e| match e {
            //                        &Expr::Sort { asc, .. } => asc,
            //                        _ => panic!()
            //                    })
            //                    .collect();
            //
            //                let rel = SortRelation {
            //                    input: input_rel,
            //                    sort_expr: compiled_expr?,
            //                    sort_asc: sort_asc,
            //                    schema: schema.clone()
            //                };
            //                Ok(Box::new(rel))
            //            },
            //}
            LogicalPlan::Limit {
                limit,
                ref input,
                ref schema,
                ..
            } => {
                let input_rel = self.create_execution_plan(input)?;
                let rel = LimitRelation {
                    input: input_rel,
                    _limit: limit,
                    schema: schema.clone(),
                };
                Ok(Box::new(rel))
            }
        }
    }

    /// load a scalar function implementation
    fn load_scalar_function(&self, function_name: &str) -> Result<Rc<ScalarFunction>> {
        match self.functions.borrow().get(&function_name.to_lowercase()) {
            Some(f) => Ok(f.clone()),
            _ => Err(ExecutionError::General(format!(
                "Unknown scalar function {}",
                function_name
            ))),
        }
    }

    /// load an aggregate function implementation
    //    fn load_aggregate_function(
    //        &self,
    //        function_name: &str,
    //    ) -> Result<Rc<AggregateFunction>> {
    //        match self.aggregate_functions.borrow().get(&function_name.to_lowercase()) {
    //            Some(f) => Ok(f.clone()),
    //            _ => Err(>ExecutionError::General(format!(
    //                "Unknown aggregate function {}",
    //                function_name
    //            ))),
    //        }
    //    }

    pub fn udf(&self, name: &str, args: Vec<Expr>) -> Expr {
        Expr::ScalarFunction {
            name: name.to_string(),
            args: args.clone(),
        }
    }

    pub fn show(&self, df: &DataFrame, count: usize) -> Result<usize> {
        //println!("show()");
        let physical_plan = PhysicalPlan::Show {
            plan: df.plan().clone(),
            count,
        };

        match self.execute(&physical_plan)? {
            ExecutionResult::Count(count) => Ok(count),
            _ => Err(ExecutionError::NotImplemented), //TODO better error
        }
    }

    pub fn write_csv(&self, df: Rc<DataFrame>, filename: &str) -> Result<usize> {
        let physical_plan = PhysicalPlan::Write {
            plan: df.plan().clone(),
            filename: filename.to_string(),
        };

        match self.execute(&physical_plan)? {
            ExecutionResult::Count(count) => Ok(count),
            _ => Err(ExecutionError::NotImplemented), //TODO better error
        }
    }

    pub fn execute(&self, physical_plan: &PhysicalPlan) -> Result<ExecutionResult> {
        //println!("execute()");
        match &self.config.as_ref() {
            &DFConfig::Local => {
                //TODO error handling
                match self.execute_local(physical_plan) {
                    Ok(r) => Ok(r),
                    Err(e) => Err(ExecutionError::General(format!(
                        "execution failed: {:?}",
                        e
                    ))),
                }
            }
            &DFConfig::Remote { ref etcd } => self.execute_remote(physical_plan, etcd.clone()),
        }
    }

    fn execute_local(&self, physical_plan: &PhysicalPlan) -> Result<ExecutionResult> {
        //println!("execute_local()");

        match physical_plan {
            &PhysicalPlan::Interactive { .. } => {
                Err(ExecutionError::General(format!("not implemented")))
            }
            &PhysicalPlan::Write {
                ref plan,
                ref filename,
            } => {
                // create output file
                // //println!("Writing csv to {}", filename);
                let file = File::create(filename)?;

                let mut w = CsvWriter {
                    w: BufWriter::with_capacity(8 * 1024 * 1024, file),
                };

                let mut execution_plan = self.create_execution_plan(plan)?;

                // implement execution here for now but should be a common method for processing a plan
                let it = execution_plan.scan();
                let mut count: usize = 0;
                it.for_each(|t| {
                    match t {
                        Ok(ref batch) => {
                            ////println!("Processing batch of {} rows", batch.row_count());
                            for i in 0..batch.num_rows() {
                                for j in 0..batch.num_columns() {
                                    if j > 0 {
                                        w.write_bytes(b",");
                                    }
                                    match *batch.column(j) {
                                        Value::Scalar(ref v) => w.write_scalar(v),
                                        Value::Column(ref v) => match v.data() {
                                            ArrayData::Boolean(ref v) => w.write_bool(v.get(i)),
                                            ArrayData::Float32(ref v) => w.write_f32(v.get(i)),
                                            ArrayData::Float64(ref v) => w.write_f64(v.get(i)),
                                            ArrayData::Int8(ref v) => w.write_i8(v.get(i)),
                                            ArrayData::Int16(ref v) => w.write_i16(v.get(i)),
                                            ArrayData::Int32(ref v) => w.write_i32(v.get(i)),
                                            ArrayData::Int64(ref v) => w.write_i64(v.get(i)),
                                            ArrayData::UInt8(ref v) => w.write_u8(v.get(i)),
                                            ArrayData::UInt16(ref v) => w.write_u16(v.get(i)),
                                            ArrayData::UInt32(ref v) => w.write_u32(v.get(i)),
                                            ArrayData::UInt64(ref v) => w.write_u64(v.get(i)),
                                            ArrayData::Utf8(ref data) => {
                                                w.write_bytes(data.slice(i))
                                            }
                                            ArrayData::Struct(ref v) => {
                                                let fields = v.iter()
                                                    .map(|arr| get_value(&arr, i))
                                                    .collect();
                                                w.write_bytes(
                                                    format!("{}", ScalarValue::Struct(fields))
                                                        .as_bytes(),
                                                );
                                            }
                                        },
                                    }
                                }
                                w.write_bytes(b"\n");
                                count += 1;
                            }
                        }
                        Err(e) => panic!(format!("Error processing row: {:?}", e)), //TODO: error handling
                    }
                });

                Ok(ExecutionResult::Count(count))
            }
            &PhysicalPlan::Show {
                ref plan,
                ref count,
            } => {
                let mut execution_plan = self.create_execution_plan(plan)?;

                // implement execution here for now but should be a common method for processing a plan
                let it = execution_plan.scan().take(*count);
                it.for_each(|t| {
                    match t {
                        Ok(ref batch) => {
                            ////println!("Processing batch of {} rows", batch.row_count());
                            for i in 0..*count {
                                if i < batch.num_rows() {
                                    let row = batch.row_slice(i);
                                    let csv = row.into_iter()
                                        .map(|v| v.to_string())
                                        .collect::<Vec<String>>()
                                        .join(",");
                                    println!("{}", csv);
                                }
                            }
                        }
                        Err(e) => panic!(format!("Error processing row: {:?}", e)), //TODO: error handling
                    }
                });

                Ok(ExecutionResult::Count(*count))
            }
        }
    }

    fn execute_remote(
        &self,
        _physical_plan: &PhysicalPlan,
        _etcd: String,
    ) -> Result<ExecutionResult> {
        Err(ExecutionError::General(format!(
            "Remote execution needs re-implementing since moving to Arrow"
        )))
    }

    //        let workers = get_worker_list(&etcd);
    //
    //        match workers {
    //            Ok(ref list) if list.len() > 0 => {
    //                let worker_uri = format!("http://{}", list[0]);
    //                match worker_uri.parse() {
    //                    Ok(uri) => {
    //
    //                        let mut core = Core::new().unwrap();
    //                        let client = Client::new(&core.handle());
    //
    //                        // serialize plan to JSON
    //                        match serde_json::to_string(&physical_plan) {
    //                            Ok(json) => {
    //                                let mut req = Request::new(Method::Post, uri);
    //                                req.headers_mut().set(ContentType::json());
    //                                req.headers_mut().set(ContentLength(json.len() as u64));
    //                                req.set_body(json);
    //
    //                                let post = client.request(req).and_then(|res| {
    //                                    ////println!("POST: {}", res.status());
    //                                    res.body().concat2()
    //                                });
    //
    //                                match core.run(post) {
    //                                    Ok(result) => {
    //                                        //TODO: parse result
    //                                        let result = str::from_utf8(&result).unwrap();
    //                                        //println!("{}", result);
    //                                        Ok(ExecutionResult::Unit)
    //                                    }
    //                                    Err(e) => Err(>ExecutionError::General(format!("error: {}", e)))
    //                                }
    //                            }
    //                            Err(e) => Err(>ExecutionError::General(format!("error: {}", e)))
    //                        }
    //
    //
    //                    }
    //                    Err(e) => Err(>ExecutionError::General(format!("error: {}", e)))
    //                }
    //            }
    //            Ok(_) => Err(>ExecutionError::General(format!("No workers found in cluster"))),
    //            Err(e) => Err(>ExecutionError::General(format!("Failed to find a worker node: {}", e)))
    //        }
    //    }
}

#[cfg(test)]
mod tests {
    use super::super::functions::geospatial::*;
    use super::super::functions::math::*;
    use super::*;
    use std::fs::File;
    use std::io::prelude::*;

    #[test]
    fn test_sqrt() {
        let mut ctx = create_context();

        ctx.register_scalar_function(Rc::new(SqrtFunction {}));

        let df = ctx.sql(&"SELECT id, sqrt(id) FROM people").unwrap();

        ctx.write_csv(df, "_test_sqrt.csv").unwrap();

        let expected_result = read_file("test/data/expected/test_sqrt.csv");

        assert_eq!(expected_result, read_file("_test_sqrt.csv"));
    }

    #[test]
    fn test_sql_udf_udt() {
        let mut ctx = create_context();

        ctx.register_scalar_function(Rc::new(STPointFunc {}));

        let df = ctx.sql(&"SELECT ST_Point(lat, lng) FROM uk_cities")
            .unwrap();

        ctx.write_csv(df, "_test_sql_udf_udt.csv").unwrap();

        let expected_result = read_file("test/data/expected/test_sql_udf_udt.csv");

        assert_eq!(expected_result, read_file("_test_sql_udf_udt.csv"));
    }

    #[test]
    fn test_df_udf_udt() {
        let mut ctx = create_context();

        ctx.register_scalar_function(Rc::new(STPointFunc {}));

        let schema = Schema::new(vec![
            Field::new("city", DataType::Utf8, false),
            Field::new("lat", DataType::Float64, false),
            Field::new("lng", DataType::Float64, false),
        ]);

        let df = ctx.load_csv("test/data/uk_cities.csv", &schema, false, None)
            .unwrap();

        // invoke custom code as a scalar UDF
        let func_expr = ctx.udf(
            "ST_Point",
            vec![df.col("lat").unwrap(), df.col("lng").unwrap()],
        );

        let df2 = df.select(vec![func_expr]).unwrap();

        ctx.write_csv(df2, "_test_df_udf_udt.csv").unwrap();

        let expected_result = read_file("test/data/expected/test_df_udf_udt.csv");

        assert_eq!(expected_result, read_file("_test_df_udf_udt.csv"));
    }

    #[test]
    fn test_filter() {
        let mut ctx = create_context();

        ctx.register_scalar_function(Rc::new(STPointFunc {}));

        let schema = Schema::new(vec![
            Field::new("city", DataType::Utf8, false),
            Field::new("lat", DataType::Float64, false),
            Field::new("lng", DataType::Float64, false),
        ]);

        let df = ctx.load_csv("test/data/uk_cities.csv", &schema, false, None)
            .unwrap();

        // filter by lat
        let df2 = df.filter(Expr::BinaryExpr {
            left: Rc::new(Expr::Column(1)), // lat
            op: Operator::Gt,
            right: Rc::new(Expr::Literal(ScalarValue::Float64(52.0))),
        }).unwrap();

        ctx.write_csv(df2, "_test_filter.csv").unwrap();

        let expected_result = read_file("test/data/expected/test_filter.csv");

        assert_eq!(expected_result, read_file("_test_filter.csv"));
    }

    /*
    #[test]
    fn test_sort() {

        let mut ctx = create_context();

        ctx.define_function(&STPointFunc {});

        let schema = Schema::new(vec![
            Field::new("city", DataType::String, false),
            Field::new("lat", DataType::Double, false),
            Field::new("lng", DataType::Double, false)]);

        let df = ctx.load("test/data/uk_cities.csv", &schema).unwrap();

        // sort by lat, lng ascending
        let df2 = df.sort(vec![
            Expr::Sort { expr: Box::new(Expr::Column(1)), asc: true },
            Expr::Sort { expr: Box::new(Expr::Column(2)), asc: true }
        ]).unwrap();

        ctx.write(df2,"_uk_cities_sorted_by_lat_lng.csv").unwrap();

        //TODO: check that generated file has expected contents
    }
    */

    #[test]
    fn test_chaining_functions() {
        let mut ctx = create_context();
        ctx.register_scalar_function(Rc::new(STPointFunc {}));
        ctx.register_scalar_function(Rc::new(STAsText {}));

        let df = ctx.sql(&"SELECT ST_AsText(ST_Point(lat, lng)) FROM uk_cities")
            .unwrap();

        ctx.write_csv(df, "_test_chaining_functions.csv").unwrap();

        let expected_result = read_file("test/data/expected/test_chaining_functions.csv");

        assert_eq!(expected_result, read_file("_test_chaining_functions.csv"));
    }

    #[test]
    fn test_simple_predicate() {
        // create execution context
        let mut ctx = ExecutionContext::local();
        ctx.register_scalar_function(Rc::new(STPointFunc {}));
        ctx.register_scalar_function(Rc::new(STAsText {}));

        // define an external table (csv file)
        //        ctx.sql(
        //            "CREATE EXTERNAL TABLE uk_cities (\
        //             city VARCHAR(100), \
        //             lat DOUBLE, \
        //             lng DOUBLE)",
        //        ).unwrap();

        let schema = Schema::new(vec![
            Field::new("city", DataType::Utf8, false),
            Field::new("lat", DataType::Float64, false),
            Field::new("lng", DataType::Float64, false),
        ]);

        let df = ctx.load_csv("./test/data/uk_cities.csv", &schema, false, None)
            .unwrap();
        ctx.register("uk_cities", df);

        // define the SQL statement
        let sql = "SELECT ST_AsText(ST_Point(lat, lng)) FROM uk_cities WHERE lat < 53.0";

        // create a data frame
        let df1 = ctx.sql(&sql).unwrap();

        // write the results to a file
        ctx.write_csv(df1, "_test_simple_predicate.csv").unwrap();

        let expected_result = read_file("test/data/expected/test_simple_predicate.csv");

        assert_eq!(expected_result, read_file("_test_simple_predicate.csv"));
    }

    #[test]
    fn test_sql_min_max() {
        // create execution context
        let mut ctx = ExecutionContext::local();

        let schema = Schema::new(vec![
            Field::new("city", DataType::Utf8, false),
            Field::new("lat", DataType::Float64, false),
            Field::new("lng", DataType::Float64, false),
        ]);

        let df = ctx.load_csv("./test/data/uk_cities.csv", &schema, false, None)
            .unwrap();
        ctx.register("uk_cities", df);

        // define the SQL statement
        let sql = "SELECT MIN(lat), MAX(lat), MIN(lng), MAX(lng) FROM uk_cities";

        // create a data frame
        let df1 = ctx.sql(&sql).unwrap();

        // write the results to a file
        ctx.write_csv(df1, "_test_sql_min_max.csv").unwrap();

        let expected_result = read_file("test/data/expected/test_sql_min_max.csv");

        assert_eq!(expected_result, read_file("_test_sql_min_max.csv"));
    }

    fn read_file(filename: &str) -> String {
        let mut file = File::open(filename).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        contents
    }

    fn create_context() -> ExecutionContext {
        // create execution context
        let mut ctx = ExecutionContext::local();

        let people = ctx.load_csv(
            "./test/data/people.csv",
            &Schema::new(vec![
                Field::new("id", DataType::Int32, false),
                Field::new("name", DataType::Utf8, false),
            ]),
            true, None,
        ).unwrap();

        ctx.register("people", people);

        let uk_cities = ctx.load_csv(
            "./test/data/uk_cities.csv",
            &Schema::new(vec![
                Field::new("city", DataType::Utf8, false),
                Field::new("lat", DataType::Float64, false),
                Field::new("lng", DataType::Float64, false),
            ]),
            false, None,
        ).unwrap();

        ctx.register("uk_cities", uk_cities);

        ctx
    }

}
