// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// This file has a lot of function with names like "div"
#![allow(clippy::should_implement_trait)]

use self::BinaryOperator::*;
use self::ExprValue::*;
use self::UnaryOperator::*;
use super::super::MachineModel;
use super::{DatatypeComponent, Location, Parameter, Stmt, SwitchCase, SymbolTable, Type};
use crate::InternedString;
use num::bigint::BigInt;
use std::collections::BTreeMap;
use std::fmt::Debug;

///////////////////////////////////////////////////////////////////////////////////////////////
// Datatypes
///////////////////////////////////////////////////////////////////////////////////////////////

/// An `Expr` represents an expression type: i.e. a computation that returns a value.
/// Every expression has a type, a value, and a location (which may be `None`). An expression may
/// also include a type annotation (`size_of_annotation`), which states that the expression is the
/// result of computing `size_of(type)`.
///
/// The `size_of_annotation` is eventually picked up by CBMC's symbolic execution when simulating
/// heap allocations: for a requested allocation of N bytes, CBMC can either create a byte array of
/// size N, or, when a type T is annotated and N is a multiple of the size of T, an array of
/// N/size_of(T) elements. The latter will facilitate updates using member operations (when T is an
/// aggregate type), and pointer-typed members can be tracked more precisely. Note that this is
/// merely a hint: failing to provide such an annotation may hamper performance, but will never
/// affect correctness.
///
/// The fields of `Expr` are kept private, and there are no getters that return mutable references.
/// This means that the only way to create and update `Expr`s is using the constructors and setters.
/// In a few cases, there are properties, such as the existence of a field on a struct type,
/// which can only be checked given a symbol table.
/// Other than these properties, the constructors ensure that all expressions are well-formed.
///
/// In general, expressions are constructed in a "chained" style:
///     ` *(&x + i);` would translate to `x.address_of().plus(i).dereference()`
/// By default, these expressions have no location: to add a location, use the `.with_location()`
/// fluent builder to add locations when desired.
/// e.g. `x.address_of().with_location(l1).plus(i).with_location(l2).dereference().with_location(l3)`
///
/// TODO:
/// The CBMC irep resentation uses sharing to reduce the in-memory size of expressions.
/// This is not currently implemented for these expressions, but would be possible given a factory.
#[derive(Debug, Clone)]
pub struct Expr {
    value: Box<ExprValue>,
    typ: Type,
    location: Location,
    size_of_annotation: Option<Type>,
}

/// The different kinds of values an expression can have.
/// The names are chosen to map directly onto the IrepID used by CBMC.
/// Each expression is described by reference to the corresponding C code that would generate it.
/// When an expression makes most sense in a broader statement context,
/// the characters >>> e <<< are used to mark the part described by the enum.
#[derive(Debug, Clone)]
pub enum ExprValue {
    /// `&self`
    AddressOf(Expr),
    /// `typ x[] = >>> {elems0, elems1 ...} <<<`
    Array {
        elems: Vec<Expr>,
    },
    /// `typ x[width] = >>> {elem} <<<`
    ArrayOf {
        elem: Expr,
    },
    /// `left = right`
    Assign {
        left: Expr,
        right: Expr,
    },
    /// `lhs op rhs`.  E.g. `lhs + rhs` if `op == BinaryOperator::Plus`
    BinOp {
        op: BinaryOperator,
        lhs: Expr,
        rhs: Expr,
    },
    /// `(__CPROVER_bool) >>> true/false <<<`. True/False as a single bit boolean.
    BoolConstant(bool),
    /// Reinterpret bytes of e as type self.typ
    ByteExtract {
        e: Expr,
        offset: u64,
    },
    /// `(bool) 1`. True false as an 8 bit c_boolean.
    CBoolConstant(bool),
    /// `*self`
    Dereference(Expr),
    /// `1.0`
    DoubleConstant(f64),
    // {}
    EmptyUnion,
    /// `1.0f`
    Float16Constant(f16),
    /// `1.0f`
    FloatConstant(f32),
    /// `Float 128 example`
    Float128Constant(f128),
    /// `function(arguments)`
    FunctionCall {
        function: Expr,
        arguments: Vec<Expr>,
    },
    /// `c ? t : e`
    If {
        c: Expr,
        t: Expr,
        e: Expr,
    },
    /// `array[index]`
    Index {
        array: Expr,
        index: Expr,
    },
    /// `123`
    IntConstant(BigInt),
    /// `lhs.field`
    Member {
        lhs: Expr,
        field: InternedString,
    },
    /// `__nondet()`
    Nondet,
    /// `NULL`
    PointerConstant(u64),
    ReadOk {
        ptr: Expr,
        size: Expr,
    },
    // `op++` etc
    SelfOp {
        op: SelfOperator,
        e: Expr,
    },
    /// <https://gcc.gnu.org/onlinedocs/gcc/Statement-Exprs.html>
    /// e.g. `({ int y = foo (); int z; if (y > 0) z = y; else z = - y; z; })`
    /// `({ op1; op2; ...})`
    StatementExpression {
        statements: Vec<Stmt>,
        location: Location,
    },
    /// A raw string constant. Note that you normally actually want a pointer to the first element.
    /// `"s"`
    StringConstant {
        s: InternedString,
    },
    /// Struct initializer
    /// `struct foo the_foo = >>> {field1, field2, ... } <<<`
    Struct {
        values: Vec<Expr>,
    },
    /// `self`
    Symbol {
        identifier: InternedString,
    },
    /// `(typ) self`. Target type is in the outer `Expr` struct.
    Typecast(Expr),
    /// Union initializer
    /// `union foo the_foo = >>> {.field = value } <<<`
    Union {
        value: Expr,
        field: InternedString,
    },
    // `op self` eg `! self` if `op == UnaryOperator::Not`
    UnOp {
        op: UnaryOperator,
        e: Expr,
    },
    /// `vec_typ x = >>> {elems0, elems1 ...} <<<`
    Vector {
        elems: Vec<Expr>,
    },
    Forall {
        variable: Expr, // symbol
        domain: Expr,   // where
    },
    Exists {
        variable: Expr, // symbol
        domain: Expr,   // where
    },
}

/// Binary operators. The names are the same as in the Irep representation.
#[derive(Debug, Clone, Copy)]
pub enum BinaryOperator {
    And,
    Ashr,
    Bitand,
    Bitor,
    Bitnand,
    Bitxor,
    Div,
    Equal,
    Ge,
    Gt,
    IeeeFloatEqual,
    IeeeFloatNotequal,
    Implies,
    Le,
    Lshr,
    Lt,
    Minus,
    Mod,
    Mult,
    Notequal,
    Or,
    OverflowMinus,
    OverflowMult,
    OverflowPlus,
    OverflowResultMinus,
    OverflowResultMult,
    OverflowResultPlus,
    Plus,
    ROk,
    Rol,
    Ror,
    Shl,
    VectorEqual,
    VectorNotequal,
    VectorGe,
    VectorGt,
    VectorLe,
    VectorLt,
    Xor,
}

// Unary operators with side-effects
#[derive(Debug, Clone, Copy)]
pub enum SelfOperator {
    /// `self--`
    Postdecrement,
    /// `self++`
    Postincrement,
    /// `--self`
    Predecrement,
    /// `++self`
    Preincrement,
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOperator {
    /// `~self`
    Bitnot,
    /// `__builtin_bitreverse<n>(self)`
    BitReverse,
    /// `__builtin_bswap<n>(self)`
    Bswap,
    /// `__CPROVER_DYNAMIC_OBJECT(self)`
    IsDynamicObject,
    /// `isfinite(self)`
    IsFinite,
    /// `!self`
    Not,
    /// `__CPROVER_OBJECT_SIZE(self)`
    ObjectSize,
    /// `__CPROVER_POINTER_OBJECT(self)`
    PointerObject,
    /// `__CPROVER_POINTER_OFFSET(self)`
    PointerOffset,
    /// `__builtin_popcount(self)`
    Popcount,
    /// `__builtin_cttz(self)`
    CountTrailingZeros { allow_zero: bool },
    /// `__builtin_ctlz(self)`
    CountLeadingZeros { allow_zero: bool },
    /// `-self`
    UnaryMinus,
}

/// The return type for `__CPROVER_overflow_op` operations
pub struct ArithmeticOverflowResult {
    /// If overflow did not occur, the result of the operation. Otherwise undefined.
    pub result: Expr,
    /// Boolean: true if overflow occured, false otherwise.
    pub overflowed: Expr,
}

pub const ARITH_OVERFLOW_RESULT_FIELD: &str = "result";
pub const ARITH_OVERFLOW_OVERFLOWED_FIELD: &str = "overflowed";

/// For arithmetic-overflow-with-result operators, CBMC returns a struct whose
/// first component is the result, and whose second component is whether the
/// operation overflowed
pub fn arithmetic_overflow_result_type(operand_type: Type) -> Type {
    assert!(operand_type.is_integer());
    // give the struct the name "overflow_result_<type>", e.g.
    // "overflow_result_Unsignedbv"
    let name: InternedString = format!("overflow_result_{operand_type:?}").into();
    Type::struct_type(
        name,
        vec![
            DatatypeComponent::field(ARITH_OVERFLOW_RESULT_FIELD, operand_type),
            DatatypeComponent::field(ARITH_OVERFLOW_OVERFLOWED_FIELD, Type::bool()),
        ],
    )
}

///////////////////////////////////////////////////////////////////////////////////////////////
// Implementations
///////////////////////////////////////////////////////////////////////////////////////////////

/// Getters
impl Expr {
    //TODO: Consider making this return the `Location` itself, since `Location` is now `Copy`.
    pub fn location(&self) -> &Location {
        &self.location
    }

    pub fn typ(&self) -> &Type {
        &self.typ
    }

    pub fn value(&self) -> &ExprValue {
        &self.value
    }

    pub fn size_of_annotation(&self) -> Option<&Type> {
        self.size_of_annotation.as_ref()
    }

    /// If the expression is an Int constant type, return its value
    pub fn int_constant_value(&self) -> Option<BigInt> {
        match &*self.value {
            ExprValue::IntConstant(i) => Some(i.clone()),
            _ => None,
        }
    }

    pub fn struct_expr_values(&self) -> Option<&Vec<Expr>> {
        match &*self.value {
            Struct { values } => Some(values),
            _ => None,
        }
    }
}

/// Predicates
impl Expr {
    pub fn is_int_constant(&self) -> bool {
        match *self.value {
            IntConstant(_) => true,
            _ => false,
        }
    }

    /// Returns whether an expression causes side effects or not
    pub fn is_side_effect(&self) -> bool {
        match &*self.value {
            // These expressions always cause side effects
            Assign { .. }
            | FunctionCall { .. }
            | Nondet
            | SelfOp { .. }
            | StatementExpression { .. } => true,
            // These expressions do not cause side effects, but the expressions
            // they contain may do. All we need to do are recursive calls.
            AddressOf(e) => e.is_side_effect(),
            Array { elems } => elems.iter().any(|e| e.is_side_effect()),
            ArrayOf { elem } => elem.is_side_effect(),
            BinOp { op: _, lhs, rhs } => lhs.is_side_effect() || rhs.is_side_effect(),
            ByteExtract { e, offset: _ } => e.is_side_effect(),
            Dereference(e) => e.is_side_effect(),
            If { c, t, e } => c.is_side_effect() || t.is_side_effect() || e.is_side_effect(),
            Index { array, index } => array.is_side_effect() || index.is_side_effect(),
            Member { lhs, field: _ } => lhs.is_side_effect(),
            Struct { values } => values.iter().any(|e| e.is_side_effect()),
            Typecast(e) => e.is_side_effect(),
            Union { value, field: _ } => value.is_side_effect(),
            UnOp { op: _, e } => e.is_side_effect(),
            Vector { elems } => elems.iter().any(|e| e.is_side_effect()),
            // The rest of expressions (constants) do not cause side effects
            _ => false,
        }
    }

    pub fn is_symbol(&self) -> bool {
        match *self.value {
            Symbol { .. } => true,
            _ => false,
        }
    }

    /// What typecasts are legal. Based off the C standard, plus some additional types
    /// that don't appear in the standard, like `bool`
    /// <https://docs.microsoft.com/en-us/cpp/c-language/type-cast-conversions?view=msvc-160>
    pub fn can_cast_from(source: &Type, target: &Type) -> bool {
        let source = source.unwrap_typedef();
        let target = target.unwrap_typedef();
        #[allow(clippy::needless_bool)]
        if source == target {
            true
        } else if target.is_bool() {
            source.is_c_bool() || source.is_integer() || source.is_pointer()
        } else if target.is_c_bool() {
            source.is_integer() || source.is_pointer() || source.is_bool()
        } else if target.is_integer() {
            source.is_c_bool()
                || source.is_bool()
                || source.is_integer()
                || source.is_floating_point()
                || source.is_pointer()
        } else if target.is_floating_point() {
            source.is_numeric()
        } else if target.is_pointer() {
            source.is_integer() || source.is_pointer()
        } else if target.is_empty() {
            true
        } else {
            false
        }
    }

    pub fn can_cast_to(&self, target: &Type) -> bool {
        Expr::can_cast_from(&self.typ, target)
    }

    pub fn can_take_address_of(&self) -> bool {
        match *self.value {
            Dereference(_) | Index { .. } | Member { .. } | Symbol { .. } => true,
            _ => false,
        }
    }
}

/// Setters
impl Expr {
    pub fn with_location(mut self, loc: Location) -> Self {
        self.location = loc;
        self
    }
}

impl Expr {
    pub fn with_size_of_annotation(mut self, ty: Type) -> Self {
        self.size_of_annotation = Some(ty);
        self
    }
}

/// Private constructor. Making this a macro allows multiple reference to self in the same call.
macro_rules! expr {
    ( $value:expr,  $typ:expr) => {{
        let typ = $typ;
        let value = Box::new($value);
        Expr { value, typ, location: Location::none(), size_of_annotation: None }
    }};
}

/// Constructors for the main types
impl Expr {
    /// `&self`
    pub fn address_of(self) -> Self {
        assert!(self.can_take_address_of(), "Can't take address of {self:?}");
        expr!(AddressOf(self), self.typ.clone().to_pointer())
    }

    /// `typ x[width] = >>> {elem} <<<`
    pub fn array_constant(self, width: u64) -> Self {
        // As per @kroening: "array_of will work with arrays of any type, no need for any assertion"
        expr!(ArrayOf { elem: self }, self.typ.clone().array_of(width))
    }

    /// `typ x[] = >>> {elems0, elems1 ...} <<<`
    pub fn array_expr(typ: Type, elems: Vec<Expr>) -> Self {
        if let Type::Array { size, typ: value_typ } = typ.clone() {
            assert_eq!(size as usize, elems.len());
            assert!(
                elems.iter().all(|x| x.typ == *value_typ),
                "Array type and value types don't match: \n{typ:?}\n{elems:?}"
            );
        } else {
            unreachable!("Can't make an array_val with non-array target type {:?}", typ);
        }
        expr!(Array { elems }, typ)
    }

    pub fn vector_expr(typ: Type, elems: Vec<Expr>) -> Self {
        if let Type::Vector { size, typ: value_typ } = typ.clone() {
            assert_eq!(size as usize, elems.len());
            assert!(
                elems.iter().all(|x| x.typ == *value_typ),
                "Vector type and value types don't match: \n{typ:?}\n{elems:?}"
            );
        } else {
            unreachable!("Can't make a vector_val with non-vector target type {:?}", typ);
        }
        expr!(Vector { elems }, typ)
    }

    /// `(__CPROVER_bool) >>> true/false <<<`. True/False as a single bit boolean.
    pub fn bool_constant(c: bool) -> Self {
        expr!(BoolConstant(c), Type::bool())
    }

    /// `(__CPROVER_bool) false`. False as a single bit boolean.
    pub fn bool_false() -> Self {
        Expr::bool_constant(false)
    }

    /// `(__CPROVER_bool) true`. True as a single bit boolean.
    pub fn bool_true() -> Self {
        Expr::bool_constant(true)
    }

    /// `(bool) 1`. True false as an 8 bit c_boolean.
    pub fn c_bool_constant(c: bool) -> Self {
        expr!(CBoolConstant(c), Type::c_bool())
    }

    /// `(bool) 1`. True false as an 8 bit c_boolean.
    pub fn c_true() -> Self {
        Self::c_bool_constant(true)
    }

    /// `(bool) 0`. True false as an 8 bit c_boolean.
    pub fn c_false() -> Self {
        Self::c_bool_constant(false)
    }

    /// `(typ) self`.
    pub fn cast_to(self, typ: Type) -> Self {
        if self.typ == typ {
            self
        } else if typ.is_bool() {
            let zero = self.typ.zero();
            self.neq(zero)
        } else {
            assert!(self.can_cast_to(&typ), "Can't cast\n\n{self:?} ({:?})\n\n{typ:?}", self.typ);
            expr!(Typecast(self), typ)
        }
    }

    /// Casts value to new_typ, only when the current type of value
    /// is equivalent to new_typ on the given target (e.g. i32 -> c_int)
    pub fn cast_to_target_equivalent_type(self, new_typ: &Type, mm: &MachineModel) -> Expr {
        if self.typ() == new_typ {
            self
        } else {
            assert!(self.typ().is_equal_on_machine(new_typ, mm));
            self.cast_to(new_typ.clone())
        }
    }

    /// Casts arguments to type of function parameters when the corresponding types
    /// are equivalent on the given target (e.g. i32 -> c_int)
    pub fn cast_arguments_to_target_equivalent_function_parameter_types(
        function: &Expr,
        mut arguments: Vec<Expr>,
        mm: &MachineModel,
    ) -> Vec<Expr> {
        let parameters = function.typ().parameters().unwrap();
        assert!(arguments.len() >= parameters.len());
        let mut rval: Vec<_> = parameters
            .iter()
            .map(|parameter| {
                arguments.remove(0).cast_to_target_equivalent_type(parameter.typ(), mm)
            })
            .collect();

        rval.append(&mut arguments);

        rval
    }

    /// *self: t
    pub fn dereference(self) -> Self {
        assert!(self.typ.is_pointer());
        expr!(Dereference(self), self.typ.base_type().unwrap().clone())
    }

    /// `1.0`
    pub fn double_constant(c: f64) -> Self {
        expr!(DoubleConstant(c), Type::double())
    }

    /// `union {double d; uint64_t bp} u = {.bp = 0x1234}; >>> u.d <<<`
    pub fn double_constant_from_bitpattern(bp: u64) -> Self {
        let c = f64::from_bits(bp);
        Self::double_constant(c)
    }

    pub fn empty_union(typ: Type, st: &SymbolTable) -> Self {
        assert!(typ.is_union() || typ.is_union_tag());
        assert!(typ.lookup_components(st).unwrap().is_empty());
        let typ = typ.aggr_tag().unwrap();
        expr!(EmptyUnion, typ)
    }

    /// `3.14f`
    pub fn float16_constant(c: f16) -> Self {
        expr!(Float16Constant(c), Type::float16())
    }

    /// `union {_Float16 f; uint16_t bp} u = {.bp = 0x1234}; >>> u.f <<<`
    pub fn float16_constant_from_bitpattern(bp: u16) -> Self {
        let c = f16::from_bits(bp);
        Self::float16_constant(c)
    }

    /// `3.14159265358979323846264338327950288L`
    pub fn float128_constant(c: f128) -> Self {
        expr!(Float128Constant(c), Type::float128())
    }

    /// `union {_Float128 f; __uint128_t bp} u = {.bp = 0x1234}; >>> u.f <<<`
    pub fn float128_constant_from_bitpattern(bp: u128) -> Self {
        let c = f128::from_bits(bp);
        Self::float128_constant(c)
    }

    /// `1.0f`
    pub fn float_constant(c: f32) -> Self {
        expr!(FloatConstant(c), Type::float())
    }

    /// `union {float f; uint32_t bp} u = {.bp = 0x1234}; >>> u.f <<<`
    pub fn float_constant_from_bitpattern(bp: u32) -> Self {
        let c = f32::from_bits(bp);
        Self::float_constant(c)
    }

    /// `typ x[__CPROVER_infinity()] = >>> {elem} <<<`
    /// i.e. initilize an infinite sized sparse array.
    /// This is useful for maps:
    /// ```
    /// bool x[__CPROVER_infinity()] = {false};
    /// x[idx_1] = true;
    /// if (x[idx_2]) { ... }
    /// ```
    pub fn infinite_array_constant(self) -> Self {
        expr!(ArrayOf { elem: self }, self.typ.clone().infinite_array_of())
    }

    /// `self[index]`
    pub fn index_array(self, index: Expr) -> Self {
        assert!(index.typ.is_integer());
        assert!(self.typ.is_array_like());
        let typ = self.typ().base_type().unwrap().clone();
        expr!(Index { array: self, index }, typ)
    }

    /// `123`
    pub fn int_constant<T>(i: T, typ: Type) -> Self
    where
        T: Into<BigInt>,
    {
        assert!(typ.is_integer() || typ.is_bitfield());
        let i = i.into();
        // TODO: <https://github.com/model-checking/kani/issues/996>
        // if i != 0 && i != 1 {
        //     assert!(
        //         typ.min_int() <= i && i <= typ.max_int(),
        //         "{} {} {} {:?}",
        //         i,
        //         typ.min_int(),
        //         typ.max_int(),
        //         typ
        //     );
        // }
        expr!(IntConstant(i), typ)
    }

    pub fn ssize_constant(i: i128, symbol_table: &SymbolTable) -> Self {
        match symbol_table.machine_model().pointer_width {
            32 => {
                let val = BigInt::from(i as i32);
                expr!(IntConstant(val), Type::ssize_t())
            }
            64 => {
                let val = BigInt::from(i as i64);
                expr!(IntConstant(val), Type::ssize_t())
            }
            i => unreachable!("Expected 32 or 64 bits pointer width, but got `{i}`"),
        }
    }

    pub fn size_constant(i: u128, symbol_table: &SymbolTable) -> Self {
        match symbol_table.machine_model().pointer_width {
            32 => {
                let val = BigInt::from(i as u32);
                expr!(IntConstant(val), Type::size_t())
            }
            64 => {
                let val = BigInt::from(i as u64);
                expr!(IntConstant(val), Type::size_t())
            }
            i => unreachable!("Expected 32 or 64 bits pointer width, but got `{i}`"),
        }
    }

    pub fn typecheck_call(function: &Expr, arguments: &[Expr]) -> bool {
        // For variadic functions, all named arguments must match the type of their formal param.
        // Extra arguments (e.g the ... args) can have any type.
        fn typecheck_named_args(parameters: &[Parameter], arguments: &[Expr]) -> bool {
            parameters.iter().zip(arguments.iter()).all(|(p, a)| {
                if a.typ() == p.typ() {
                    true
                } else {
                    tracing::error!(param=?p.typ(), arg=?a.typ(), "Argument doesn't check");
                    false
                }
            })
        }

        if function.typ().is_code() {
            let parameters = function.typ().parameters().unwrap();
            arguments.len() == parameters.len() && typecheck_named_args(parameters, arguments)
        } else if function.typ().is_variadic_code() {
            let parameters = function.typ().parameters().unwrap();
            arguments.len() >= parameters.len() && typecheck_named_args(parameters, arguments)
        } else {
            false
        }
    }

    /// `function(arguments)`
    ///
    /// This gives an _expression_.
    /// If you are using this in statement context (e.g. ignoring or assigning the value), use
    /// the `Stmt::function_call` constructor.
    pub fn call(self, arguments: Vec<Expr>) -> Self {
        assert!(
            Expr::typecheck_call(&self, &arguments),
            "Function call does not type check:\nfunc params: {:?}\nargs: {arguments:?}",
            self.typ().parameters().unwrap_or(&vec![])
        );
        let typ = self.typ().return_type().unwrap().clone();
        expr!(FunctionCall { function: self, arguments }, typ)
    }

    /// `self.field`
    pub fn member<T>(self, field: T, symbol_table: &SymbolTable) -> Self
    where
        T: Into<InternedString>,
    {
        let field: InternedString = field.into();
        assert!(
            self.typ.is_struct_tag() || self.typ.is_union_tag(),
            "Can't apply .member operation to\n\t{self:?}\n\t{field}",
        );
        if let Some(ty) = self.typ.lookup_field_type(field, symbol_table) {
            expr!(Member { lhs: self, field }, ty)
        } else {
            unreachable!("unable to find field {} for type {:?}", field, self.typ())
        }
    }

    /// `__nondet_typ()`
    pub fn nondet(typ: Type) -> Self {
        expr!(Nondet, typ)
    }

    /// `read_ok(ptr, size)`
    pub fn read_ok(ptr: Expr, size: Expr) -> Self {
        assert_eq!(*ptr.typ(), Type::void_pointer());
        assert_eq!(*size.typ(), Type::size_t());

        expr!(ReadOk { ptr, size }, Type::bool())
    }

    /// `e.g. NULL`
    pub fn pointer_constant(c: u64, typ: Type) -> Self {
        assert!(typ.is_pointer());
        expr!(PointerConstant(c), typ)
    }

    /// <https://gcc.gnu.org/onlinedocs/gcc/Statement-Exprs.html>
    /// e.g. `({ int y = foo (); int z; if (y > 0) z = y; else z = - y; z; })`
    /// `({ op1; op2; ...})`
    pub fn statement_expression(ops: Vec<Stmt>, typ: Type, loc: Location) -> Self {
        assert!(!ops.is_empty());
        assert_eq!(ops.last().unwrap().get_expression().unwrap().typ, typ);
        expr!(StatementExpression { statements: ops, location: loc }, typ).with_location(loc)
    }

    /// Internal helper function for Struct initalizer
    /// `struct foo the_foo = >>> {.field1 = val1, .field2 = val2, ... } <<<`
    /// ALL fields must be given, including padding
    fn struct_expr_with_explicit_padding(
        typ: Type,
        fields: &[DatatypeComponent],
        values: Vec<Expr>,
    ) -> Self {
        assert_eq!(fields.len(), values.len());
        // Check that each formal field has an value
        assert!(
            fields.iter().zip(values.iter()).all(|(f, v)| f.typ() == *v.typ()),
            "Error in struct_expr; value type does not match field type.\n\t{typ:?}\n\t{fields:?}\n\t{values:?}"
        );
        expr!(Struct { values }, typ)
    }

    /// Struct initializer
    /// `struct foo the_foo = >>> {.field1 = val1, .field2 = val2, ... } <<<`
    /// Note that only the NON padding fields should be explicitly given.
    /// Padding fields are automatically inserted using the type from the `SymbolTable`
    pub fn struct_expr(
        typ: Type,
        mut components: BTreeMap<InternedString, Expr>,
        symbol_table: &SymbolTable,
    ) -> Self {
        assert!(
            typ.is_struct_tag(),
            "Error in struct_expr; must be given a struct_tag.\n\t{typ:?}\n\t{components:?}"
        );
        let fields = typ.lookup_components(symbol_table).unwrap();
        let non_padding_fields: Vec<_> = fields.iter().filter(|x| !x.is_padding()).collect();
        assert_eq!(
            non_padding_fields.len(),
            components.len(),
            "Error in struct_expr; mismatch in number of fields and components.\n\t{typ:?}\n\t{components:?}"
        );

        // Check that each formal field has an value
        for field in non_padding_fields {
            let field_typ = field.field_typ().unwrap();
            let value = components.get(&field.name()).unwrap();
            assert_eq!(value.typ(), field_typ, "Unexpected type for {:?}", field.name());
        }

        let values = fields
            .iter()
            .map(|field| {
                if field.is_padding() {
                    field.typ().nondet()
                } else {
                    components.remove(&field.name()).unwrap()
                }
            })
            .collect();

        Expr::struct_expr_with_explicit_padding(typ, fields, values)
    }

    /// Struct initializer with default nondet fields except for given `components`
    /// `struct foo the_foo = >>> {.field1 = val1, .field2 = val2, ... } <<<`
    pub fn struct_expr_with_nondet_fields(
        typ: Type,
        mut components: BTreeMap<InternedString, Expr>,
        symbol_table: &SymbolTable,
    ) -> Self {
        assert!(typ.is_struct_tag());
        let fields = typ.lookup_components(symbol_table).unwrap();
        let non_padding_fields: Vec<_> = fields.iter().filter(|x| !x.is_padding()).collect();
        let values = non_padding_fields
            .iter()
            .map(|field| {
                let field_name = field.name();
                if components.contains_key(&field_name) {
                    components.remove(&field_name).unwrap()
                } else {
                    field.typ().nondet()
                }
            })
            .collect();
        Expr::struct_expr_from_values(typ, values, symbol_table)
    }

    /// Struct initializer
    /// `struct foo the_foo = >>> {field1, field2, ... } <<<`
    /// Note that only the NON padding fields should be explicitly given.
    /// Padding fields are automatically inserted using the type from the `SymbolTable`
    pub fn struct_expr_from_values(
        typ: Type,
        mut non_padding_values: Vec<Expr>,
        symbol_table: &SymbolTable,
    ) -> Self {
        assert!(
            typ.is_struct_tag(),
            "Error in struct_expr; must be given struct_tag.\n\t{typ:?}\n\t{non_padding_values:?}"
        );
        let fields = typ.lookup_components(symbol_table).unwrap();
        let non_padding_fields: Vec<_> = fields.iter().filter(|x| !x.is_padding()).collect();
        assert_eq!(
            non_padding_fields.len(),
            non_padding_values.len(),
            "Error in struct_expr; mismatch in number of fields and values.\n\t{typ:?}\n\t{non_padding_values:?}"
        );
        assert!(
            non_padding_fields
                .iter()
                .zip(non_padding_values.iter())
                .all(|(f, v)| f.field_typ().unwrap() == v.typ()),
            "Error in struct_expr; value type does not match field type.\n\t{typ:?}\n\t{non_padding_fields:?}\n\t{non_padding_values:?}"
        );

        let values = fields
            .iter()
            .map(|f| if f.is_padding() { f.typ().nondet() } else { non_padding_values.remove(0) })
            .collect();

        Expr::struct_expr_with_explicit_padding(typ, fields, values)
    }

    /// Struct initializer
    /// `struct foo the_foo = >>> {field1, padding2, field3, ... } <<<`
    /// Note that padding fields should be explicitly given.
    /// This would be used when the values and padding have already been combined,
    /// e.g. when extracting the values out of an existing struct expr (see transformer.rs)
    pub fn struct_expr_from_padded_values(
        typ: Type,
        values: Vec<Expr>,
        symbol_table: &SymbolTable,
    ) -> Self {
        assert!(
            typ.is_struct_tag() || typ.is_struct(),
            "Error in struct_expr; must be given struct.\n\t{typ:?}\n\t{values:?}"
        );
        let typ = typ.aggr_tag().unwrap();
        let fields = typ.lookup_components(symbol_table).unwrap();
        assert_eq!(
            fields.len(),
            values.len(),
            "Error in struct_expr; mismatch in number of padded fields and padded values.\n\t{typ:?}\n\t{values:?}"
        );
        assert!(
            fields.iter().zip(values.iter()).all(|(f, v)| &f.typ() == v.typ()),
            "Error in struct_expr; value type does not match field type.\n\t{typ:?}\n\t{fields:?}\n\t{values:?}"
        );

        Expr::struct_expr_with_explicit_padding(typ, fields, values)
    }

    /// Initializer for a zero sized type (ZST).
    /// Since this is a ZST, we call nondet to simplify everything.
    pub fn init_unit(typ: Type, symbol_table: &SymbolTable) -> Self {
        assert_eq!(typ.sizeof_in_bits(symbol_table), 0);
        Expr::nondet(typ)
    }

    /// `identifier`
    pub fn symbol_expression<T: Into<InternedString>>(identifier: T, typ: Type) -> Self {
        let identifier = identifier.into();
        expr!(Symbol { identifier }, typ)
    }

    /// `self ? t : e`
    pub fn ternary(self, t: Expr, e: Expr) -> Expr {
        assert_eq!(t.typ, e.typ);
        expr!(If { c: self.cast_to(Type::bool()), t, e }, t.typ.clone())
    }

    /// Reinterpret the bits of `self` as being of type `t`.
    /// Note that this differs from standard casts, which may convert values.
    /// To abuse syntax: `(uint32_t)(1.0) == 1`, while `(1.0).transmute_to(uin32_t) == 0x3f800000`
    /// If `sizeof(self.typ()) != sizeof(t)`, then CBMC will truncate/extend with nondet as needed.
    /// In cases where this is not desired (e.g. casting the first element of a struct to the struct
    /// type itself, try using `reinterpret_cast`.
    pub fn transmute_to(self, t: Type, st: &SymbolTable) -> Expr {
        assert_eq!(self.typ().sizeof_in_bits(st), t.sizeof_in_bits(st));
        expr!(ByteExtract { e: self, offset: 0 }, t)
    }

    /// Transmute between types that are already byte equivalent.
    /// See documentation on `is_structurally_equivalent_to` for more details.
    pub fn transmute_to_structurally_equivalent_type(self, t: Type, st: &SymbolTable) -> Expr {
        assert!(self.typ().is_structurally_equivalent_to(&t, st));
        self.transmute_to(t, st)
    }

    /// Union initializer
    /// `union foo the_foo = >>> {.field = value } <<<`
    pub fn union_expr<T: Into<InternedString>>(
        typ: Type,
        field: T,
        value: Expr,
        symbol_table: &SymbolTable,
    ) -> Self {
        let field = field.into();
        assert!(typ.is_union_tag() || typ.is_union());
        assert_eq!(typ.lookup_field_type(field, symbol_table).as_ref(), Some(value.typ()));
        let typ = typ.aggr_tag().unwrap();
        expr!(Union { value, field }, typ)
    }

    pub fn forall_expr(typ: Type, variable: Expr, domain: Expr) -> Expr {
        assert!(variable.is_symbol());
        expr!(Forall { variable, domain }, typ)
    }

    pub fn exists_expr(typ: Type, variable: Expr, domain: Expr) -> Expr {
        assert!(variable.is_symbol());
        expr!(Exists { variable, domain }, typ)
    }
}

/// Constructors for Binary Operations
impl Expr {
    fn typecheck_binop_args(op: BinaryOperator, lhs: &Expr, rhs: &Expr) -> bool {
        match op {
            // Arithmetic which can include pointers
            Minus => {
                (lhs.typ == rhs.typ)
                    && (lhs.typ.is_pointer() || lhs.typ.is_numeric() || lhs.typ.is_vector())
                    || (lhs.typ.is_pointer() && rhs.typ.is_integer())
            }
            Plus => {
                (lhs.typ == rhs.typ && (lhs.typ.is_numeric() || lhs.typ.is_vector()))
                    || (lhs.typ.is_pointer() && rhs.typ.is_integer())
            }
            // Arithmetic
            Div | Mod | Mult => lhs.typ == rhs.typ && (lhs.typ.is_numeric() || lhs.typ.is_vector()),
            // Bitshifts
            Ashr | Lshr | Shl => {
                lhs.typ.is_integer() && rhs.typ.is_integer()
                    || (lhs.typ == rhs.typ && lhs.typ.is_vector())
            }
            Rol | Ror => lhs.typ.is_integer() && rhs.typ.is_integer(),
            // Boolean ops
            And | Implies | Or | Xor => lhs.typ.is_bool() && rhs.typ.is_bool(),
            // Bitwise ops
            Bitand | Bitor | Bitxor => {
                lhs.typ == rhs.typ && (lhs.typ.is_integer() || lhs.typ.is_vector())
            }
            // Bitwise ops (no vector support)
            Bitnand => lhs.typ == rhs.typ && lhs.typ.is_integer(),
            // Comparisons
            Ge | Gt | Le | Lt => {
                lhs.typ == rhs.typ && (lhs.typ.is_numeric() || lhs.typ.is_pointer())
            }
            // Equalities
            Equal | Notequal => {
                lhs.typ == rhs.typ
                    && (lhs.typ.is_c_bool() || lhs.typ.is_integer() || lhs.typ.is_pointer())
            }
            // Floating Point Equalities
            IeeeFloatEqual | IeeeFloatNotequal => lhs.typ == rhs.typ && lhs.typ.is_floating_point(),
            // Overflow flags
            OverflowMinus | OverflowResultMinus => {
                (lhs.typ == rhs.typ && (lhs.typ.is_pointer() || lhs.typ.is_numeric()))
                    || (lhs.typ.is_pointer() && rhs.typ.is_integer())
            }
            OverflowMult | OverflowPlus | OverflowResultMult | OverflowResultPlus => {
                (lhs.typ == rhs.typ && lhs.typ.is_integer())
                    || (lhs.typ.is_pointer() && rhs.typ.is_integer())
            }
            ROk => lhs.typ.is_pointer() && rhs.typ.is_c_size_t(),
            VectorEqual | VectorNotequal | VectorGe | VectorLe | VectorGt | VectorLt => {
                unreachable!(
                    "vector comparison operators must be typechecked by `typecheck_vector_cmp_expr`"
                )
            }
        }
    }

    fn binop_return_type(op: BinaryOperator, lhs: &Expr, rhs: &Expr) -> Type {
        match op {
            // Arithmetic which can include pointers
            Minus => {
                if lhs.typ.is_pointer() && rhs.typ.is_pointer() {
                    Type::ssize_t()
                } else {
                    lhs.typ.clone()
                }
            }
            // Arithmetic
            Div | Mod | Mult | Plus => lhs.typ.clone(),
            // Bitshifts
            Ashr | Lshr | Rol | Ror | Shl => lhs.typ.clone(),
            // Boolean ops
            And | Implies | Or | Xor => Type::bool(),
            // Bitwise ops
            Bitand | Bitnand | Bitor | Bitxor => lhs.typ.clone(),
            // Comparisons
            Ge | Gt | Le | Lt => Type::bool(),
            // Equalities
            Equal | Notequal => Type::bool(),
            // Floating Point Equalities
            IeeeFloatEqual | IeeeFloatNotequal => Type::bool(),
            // Overflow flags
            OverflowMinus | OverflowMult | OverflowPlus => Type::bool(),
            OverflowResultMinus | OverflowResultMult | OverflowResultPlus => {
                let struct_type = arithmetic_overflow_result_type(lhs.typ.clone());
                Type::struct_tag(struct_type.tag().unwrap())
            }
            ROk => Type::bool(),
            // Vector comparisons
            VectorEqual | VectorNotequal | VectorGe | VectorLe | VectorGt | VectorLt => {
                unreachable!(
                    "return type for vector comparison operators depends on the place type"
                )
            }
        }
    }

    /// Comparison operators for SIMD vectors aren't typechecked as regular
    /// comparison operators. First, the return type depends on the place's type
    /// (i.e., the variable or expression type for the result).
    ///
    /// In addition, the return type must have:
    ///  1. The same length (number of elements) as the operand types.
    ///  2. An integer base type (or just "boolean"-y, as mentioned in
    ///     <https://github.com/rust-lang/rfcs/blob/master/text/1199-simd-infrastructure.md#comparisons>).
    ///     The signedness doesn't matter, as the result for each element is
    ///     either "all ones" (true) or "all zeros" (false).
    ///
    /// For example, one can use `simd_eq` on two `f64x4` vectors and assign the
    /// result to a `u64x4` vector. But it's not possible to assign it to: (1) a
    /// `u64x2` because they don't have the same length; or (2) another `f64x4`
    /// vector.
    fn typecheck_vector_cmp_expr(lhs: &Expr, rhs: &Expr, ret_typ: &Type) -> bool {
        lhs.typ.is_vector()
            && lhs.typ == rhs.typ
            && lhs.typ.len() == ret_typ.len()
            && ret_typ.base_type().unwrap().is_integer()
    }

    /// self op right;
    pub fn binop(self, op: BinaryOperator, rhs: Expr) -> Expr {
        assert!(
            Expr::typecheck_binop_args(op, &self, &rhs),
            "BinaryOperation Expression does not typecheck {op:?} {self:?} {rhs:?}"
        );
        expr!(BinOp { op, lhs: self, rhs }, Expr::binop_return_type(op, &self, &rhs))
    }

    /// Like `binop`, but receives an additional parameter `ret_typ` with the expected
    /// return type for the place, which is used as the return type.
    pub fn vector_cmp(self, op: BinaryOperator, rhs: Expr, ret_typ: Type) -> Expr {
        assert!(
            Expr::typecheck_vector_cmp_expr(&self, &rhs, &ret_typ),
            "vector comparison expression does not typecheck {self:?} {rhs:?} {ret_typ:?}",
        );
        expr!(BinOp { op, lhs: self, rhs }, ret_typ)
    }

    /// `__builtin_add_overflow_p(self,e)
    pub fn add_overflow_p(self, e: Expr) -> Expr {
        self.binop(OverflowPlus, e)
    }

    /// `__builtin_sub_overflow_p(self,e)
    pub fn sub_overflow_p(self, e: Expr) -> Expr {
        self.binop(OverflowMinus, e)
    }

    /// `__builtin_mul_overflow_p(self,e)
    pub fn mul_overflow_p(self, e: Expr) -> Expr {
        self.binop(OverflowMult, e)
    }

    /// `self / e`
    pub fn div(self, e: Expr) -> Expr {
        self.binop(Div, e)
    }

    /// `self % e`
    pub fn rem(self, e: Expr) -> Expr {
        self.binop(Mod, e)
    }

    /// `self && e`
    pub fn and(self, e: Expr) -> Expr {
        self.cast_to(Type::bool()).binop(And, e.cast_to(Type::bool()))
    }

    /// `self ==> e`;
    pub fn implies(self, e: Expr) -> Expr {
        self.cast_to(Type::bool()).binop(Implies, e.cast_to(Type::bool()))
    }

    /// `self || e`
    pub fn or(self, e: Expr) -> Expr {
        self.cast_to(Type::bool()).binop(Or, e.cast_to(Type::bool()))
    }

    /// logical xor
    pub fn xor(self, e: Expr) -> Expr {
        self.binop(Xor, e)
    }

    /// `self & e`
    pub fn bitand(self, e: Expr) -> Expr {
        self.binop(Bitand, e)
    }

    /// `~ (self & e)`
    pub fn bitnand(self, e: Expr) -> Expr {
        self.binop(Bitnand, e)
    }

    /// `self | e`
    pub fn bitor(self, e: Expr) -> Expr {
        self.binop(Bitor, e)
    }

    /// `self ^ e`
    pub fn bitxor(self, e: Expr) -> Expr {
        self.binop(Bitxor, e)
    }

    /// `self << e`
    pub fn shl(self, e: Expr) -> Expr {
        self.binop(Shl, e)
    }

    /// `self >> e` (Signed arithmetic shift)
    pub fn ashr(self, e: Expr) -> Expr {
        self.binop(Ashr, e)
    }

    /// `self >> e` (Unsigned logical shift)
    pub fn lshr(self, e: Expr) -> Expr {
        self.binop(Lshr, e)
    }

    /// `self + e`
    pub fn plus(self, e: Expr) -> Expr {
        self.binop(Plus, e)
    }

    /// `self - e`
    pub fn sub(self, e: Expr) -> Expr {
        self.binop(Minus, e)
    }

    /// self * e
    pub fn mul(self, e: Expr) -> Expr {
        self.binop(Mult, e)
    }

    /// self <= e
    pub fn le(self, e: Expr) -> Expr {
        self.binop(Le, e)
    }

    /// self < e
    pub fn lt(self, e: Expr) -> Expr {
        self.binop(Lt, e)
    }

    /// self >= e
    pub fn ge(self, e: Expr) -> Expr {
        self.binop(Ge, e)
    }

    /// self > e
    pub fn gt(self, e: Expr) -> Expr {
        self.binop(Gt, e)
    }

    /// self : integer == e
    pub fn eq(self, e: Expr) -> Expr {
        self.binop(Equal, e)
    }

    /// self : integer != e
    pub fn neq(self, e: Expr) -> Expr {
        self.binop(Notequal, e)
    }

    /// self : floating point == e
    pub fn feq(self, e: Expr) -> Expr {
        self.binop(IeeeFloatEqual, e)
    }

    /// self : floating point != e
    pub fn fneq(self, e: Expr) -> Expr {
        self.binop(IeeeFloatNotequal, e)
    }

    /// `__builtin_rotateleft(self, e)`
    pub fn rol(self, e: Expr) -> Expr {
        self.binop(Rol, e)
    }

    /// `__builtin_rotateright(self, e)`
    pub fn ror(self, e: Expr) -> Expr {
        self.binop(Ror, e)
    }

    /// `__CPROVER_r_ok(self, e)`
    pub fn r_ok(self, e: Expr) -> Expr {
        self.binop(ROk, e)
    }

    // Regular comparison operators (e.g., `==` or `<`) don't work over SIMD vectors.
    // Instead, we must use the dedicated `vector-<op>` Irep operators.

    /// `self == e` for SIMD vectors
    pub fn vector_eq(self, e: Expr, ret_typ: Type) -> Expr {
        self.vector_cmp(VectorEqual, e, ret_typ)
    }

    /// `self != e` for SIMD vectors
    pub fn vector_neq(self, e: Expr, ret_typ: Type) -> Expr {
        self.vector_cmp(VectorNotequal, e, ret_typ)
    }

    /// `self >= e` for SIMD vectors
    pub fn vector_ge(self, e: Expr, ret_typ: Type) -> Expr {
        self.vector_cmp(VectorGe, e, ret_typ)
    }

    /// `self <= e` for SIMD vectors
    pub fn vector_le(self, e: Expr, ret_typ: Type) -> Expr {
        self.vector_cmp(VectorLe, e, ret_typ)
    }

    /// `self > e` for SIMD vectors
    pub fn vector_gt(self, e: Expr, ret_typ: Type) -> Expr {
        self.vector_cmp(VectorGt, e, ret_typ)
    }

    /// `self < e` for SIMD vectors
    pub fn vector_lt(self, e: Expr, ret_typ: Type) -> Expr {
        self.vector_cmp(VectorLt, e, ret_typ)
    }

    // Expressions defined on top of other expressions

    /// `min(self, e)`
    pub fn min(self, e: Expr) -> Expr {
        assert!(!self.is_side_effect() && !e.is_side_effect());
        let cmp = self.clone().lt(e.clone());
        cmp.ternary(self, e)
    }

    /// `max(self, e)`
    pub fn max(self, e: Expr) -> Expr {
        assert!(!self.is_side_effect() && !e.is_side_effect());
        let cmp = self.clone().gt(e.clone());
        cmp.ternary(self, e)
    }
}

/// Constructors for self operations
impl Expr {
    /// Private constructor for self operations
    fn self_op(self, op: SelfOperator) -> Expr {
        assert!(self.typ.is_integer() || self.typ.is_pointer());
        expr!(SelfOp { op, e: self }, self.typ.clone())
    }

    /// `self++`
    pub fn postincr(self) -> Expr {
        self.self_op(SelfOperator::Postincrement)
    }

    /// `self--`
    pub fn postdecr(self) -> Expr {
        self.self_op(SelfOperator::Postdecrement)
    }

    /// `++self`
    pub fn preincr(self) -> Expr {
        self.self_op(SelfOperator::Postincrement)
    }

    /// `--self`
    pub fn predecr(self) -> Expr {
        self.self_op(SelfOperator::Postdecrement)
    }
}

/// Constructors for unary operators
impl Expr {
    fn typecheck_unop_arg(op: UnaryOperator, arg: &Expr) -> bool {
        match op {
            Bitnot | BitReverse | Bswap | Popcount => arg.typ.is_integer(),
            CountLeadingZeros { .. } | CountTrailingZeros { .. } => arg.typ.is_integer(),
            IsDynamicObject | ObjectSize | PointerObject => arg.typ().is_pointer(),
            IsFinite => arg.typ().is_floating_point(),
            PointerOffset => arg.typ == Type::void_pointer(),
            Not => arg.typ.is_bool(),
            UnaryMinus => arg.typ().is_numeric(),
        }
    }

    fn unop_return_type(op: UnaryOperator, arg: &Expr) -> Type {
        match op {
            Bitnot | BitReverse | Bswap | UnaryMinus => arg.typ.clone(),
            CountLeadingZeros { .. } | CountTrailingZeros { .. } => Type::unsigned_int(32),
            ObjectSize | PointerObject => Type::size_t(),
            PointerOffset => Type::ssize_t(),
            IsDynamicObject | IsFinite | Not => Type::bool(),
            Popcount => Type::unsigned_int(32),
        }
    }
    /// Private helper function to make unary operators
    fn unop(self, op: UnaryOperator) -> Expr {
        assert!(Expr::typecheck_unop_arg(op, &self));
        let typ = Expr::unop_return_type(op, &self);
        expr!(ExprValue::UnOp { op, e: self }, typ)
    }

    /// `~self`
    pub fn bitnot(self) -> Expr {
        self.unop(Bitnot)
    }

    ///  `__builtin_bswap<n>(self)`
    pub fn bswap(self) -> Expr {
        self.unop(Bswap)
    }

    /// `__builtin_bitreverse<n>(self)`
    pub fn bitreverse(self) -> Expr {
        self.unop(BitReverse)
    }

    /// `__CPROVER_DYNAMIC_OBJECT(self)`
    pub fn dynamic_object(self) -> Self {
        self.unop(IsDynamicObject)
    }

    /// `isfinite(self)`
    pub fn is_finite(self) -> Self {
        self.unop(IsFinite)
    }

    /// `-self`
    pub fn neg(self) -> Expr {
        self.unop(UnaryMinus)
    }

    /// `!self`
    pub fn not(self) -> Expr {
        self.cast_to(Type::bool()).unop(Not)
    }

    /// `__CPROVER_OBJECT_SIZE(self)`
    pub fn object_size(self) -> Self {
        self.unop(ObjectSize)
    }

    /// `__CPROVER_POINTER_OBJECT(self)`
    pub fn pointer_object(self) -> Self {
        self.unop(PointerObject)
    }

    /// `__CPROVER_POINTER_OFFSET(self)`
    pub fn pointer_offset(self) -> Self {
        self.cast_to(Type::void_pointer()).unop(PointerOffset)
    }

    /// `__builtin_popcount(self)`
    pub fn popcount(self) -> Expr {
        self.unop(Popcount)
    }

    /// `__builtin_cttz(self)`
    /// If `allow_zero == false`, calling this builtin with 0 causes UB
    /// Otherwise it is defined for all values
    pub fn cttz(self, allow_zero: bool) -> Expr {
        self.unop(CountTrailingZeros { allow_zero })
    }

    /// `__builtin_ctlz(self)`
    /// If `allow_zero == false`, calling this builtin with 0 causes UB
    /// Otherwise it is defined for all values
    pub fn ctlz(self, allow_zero: bool) -> Expr {
        self.unop(CountLeadingZeros { allow_zero })
    }
}

/// Compound Expressions
impl Expr {
    /// `self < 0`
    pub fn is_negative(self) -> Self {
        assert!(self.typ.is_numeric());
        let typ = self.typ.clone();
        self.lt(typ.zero())
    }

    /// `self >= 0`
    pub fn is_non_negative(self) -> Self {
        assert!(self.typ.is_numeric());
        let typ = self.typ.clone();
        self.ge(typ.zero())
    }

    /// `self == 0`
    pub fn is_zero(self) -> Self {
        assert!(self.typ.is_numeric() || self.typ.is_pointer());
        let typ = self.typ.clone();
        self.eq(typ.zero())
    }

    /// `self != NULL`
    pub fn is_nonnull(self) -> Self {
        assert!(self.typ.is_pointer());
        let nullptr = self.typ().null();
        self.neq(nullptr)
    }

    /// `ArithmeticOverflowResult r; >>>r.overflowed = builtin_add_overflow(self, e, &r.result)<<<`
    pub fn add_overflow(self, e: Expr) -> ArithmeticOverflowResult {
        let result = self.clone().plus(e.clone());
        let overflowed = self.add_overflow_p(e);
        ArithmeticOverflowResult { result, overflowed }
    }

    /// Uses CBMC's \[binop\]-with-overflow operation that performs a single arithmetic
    /// operation
    /// `struct (T, bool) overflow(binop, self, e)` where `T` is the type of `self`
    /// Pseudocode:
    /// ```
    /// struct overflow_result_t {
    ///   T    result;
    ///   bool overflowed;
    /// } overflow_result;
    /// raw_result = (cast to wider type) self + (cast to wider type) e;
    /// overflow_result.result = (cast to T) raw_result;
    /// overflow_result.overflowed = raw_result > maximum value of T;
    /// return overflow_result;
    /// ```
    pub fn overflow_op(self, op: BinaryOperator, e: Expr) -> Expr {
        assert!(
            matches!(op, OverflowResultMinus | OverflowResultMult | OverflowResultPlus),
            "Expected an overflow operation, but found: `{op:?}`"
        );
        self.binop(op, e)
    }

    pub fn add_overflow_result(self, e: Expr) -> Expr {
        self.binop(OverflowResultPlus, e)
    }

    /// `&self[0]`. Converts arrays into pointers
    pub fn array_to_ptr(self) -> Self {
        assert!(self.typ().is_array_like());
        self.index_array(Type::ssize_t().zero()).address_of()
    }

    /// `self[i]` where self is a pointer or an array
    pub fn index(self, idx: Expr) -> Self {
        assert!(idx.typ().is_integer());
        if self.typ().is_pointer() {
            self.index_ptr(idx)
        } else if self.typ().is_array_like() {
            self.index_array(idx)
        } else {
            panic!("Can't index: {self:?}")
        }
    }

    /// `self[i]` where self is a pointer
    pub fn index_ptr(self, idx: Expr) -> Self {
        assert!(idx.typ().is_integer());
        assert!(self.typ().is_pointer());
        self.plus(idx).dereference()
    }

    /// `ArithmeticOverflowResult r; >>>r.overflowed = builtin_sub_overflow(self, e, &r.result)<<<`
    pub fn mul_overflow(self, e: Expr) -> ArithmeticOverflowResult {
        // TODO: We should replace these calls by *overflow_result.
        // https://github.com/model-checking/kani/issues/1483
        let result = self.clone().mul(e.clone());
        let overflowed = self.mul_overflow_p(e);
        ArithmeticOverflowResult { result, overflowed }
    }

    /// Uses CBMC's multiply-with-overflow operation that performs a single
    /// multiplication operation
    /// `struct (T, bool) overflow(*, self, e)` where `T` is the type of `self`
    /// See pseudocode in `add_overflow_result`
    pub fn mul_overflow_result(self, e: Expr) -> Expr {
        self.binop(OverflowResultMult, e)
    }

    /// Reinterpret the bits of `self` as being of type `t`.
    /// Note that this differs from standard casts, which may convert values.
    /// in C++ syntax: `(uint32_t)(1.0) == 1`, while `reinterpret_cast<uin32_t>(1.0) == 0x3f800000`
    /// Currently implemented as `*(T *)&self`, which creates a requirement that we can take the
    /// address of `self`.
    /// Unlike `transmute_to`, this "works" on arguments of different sizes.
    pub fn reinterpret_cast(self, t: Type) -> Expr {
        assert!(
            self.can_take_address_of(),
            "Can't take address of {self:?} when coercing to {t:?}"
        );
        self.address_of().cast_to(t.to_pointer()).dereference()
    }

    /// `ArithmeticOverflowResult r; >>>r.overflowed = builtin_mul_overflow(self, e, &r.result)<<<`
    pub fn sub_overflow(self, e: Expr) -> ArithmeticOverflowResult {
        let result = self.clone().sub(e.clone());
        let overflowed = self.sub_overflow_p(e);
        ArithmeticOverflowResult { result, overflowed }
    }

    /// Uses CBMC's subtract-with-overflow operation that performs a single
    /// subtraction operation
    /// See pseudocode in `add_overflow_result`
    /// `struct (T, bool) overflow(-, self, e)` where `T` is the type of `self`
    pub fn sub_overflow_result(self, e: Expr) -> Expr {
        self.binop(OverflowResultMinus, e)
    }

    /// `__CPROVER_same_object(self, e)`
    pub fn same_object(self, e: Expr) -> Self {
        self.pointer_object().eq(e.pointer_object())
    }

    /// addition that saturates at bounds
    pub fn saturating_add(self, e: Expr, mm: &MachineModel) -> Expr {
        assert!(self.typ.is_integer());
        assert_eq!(self.typ, e.typ);
        let typ = self.typ.clone();
        let res = self.clone().add_overflow(e);
        // If negative + something overflowed, the something must have been negative too, so we saturate to min.
        // (self < 0) ? min_int : max_int
        let saturating_val = self.is_negative().ternary(typ.min_int_expr(mm), typ.max_int_expr(mm));
        res.overflowed.ternary(saturating_val, res.result)
    }

    /// subtraction that saturates at bounds
    pub fn saturating_sub(self, e: Expr, mm: &MachineModel) -> Expr {
        assert!(self.typ.is_integer());
        assert_eq!(self.typ, e.typ);
        let typ = self.typ.clone();
        let res = self.sub_overflow(e.clone());
        // If something minus a negative overflowed, it must have overflowed past positive max. Saturate there.
        // Otherwise, if something minus a positive overflowed, it must have overflowed to past min. Saturate there.
        let saturating_val = e.is_negative().ternary(typ.max_int_expr(mm), typ.min_int_expr(mm));
        res.overflowed.ternary(saturating_val, res.result)
    }

    /// `"s"`
    /// only to be used when manually wrapped in `.array_to_ptr()`
    pub fn raw_string_constant(s: InternedString) -> Self {
        expr!(StringConstant { s }, Type::c_char().array_of(s.len() + 1))
    }

    /// `"s"`
    pub fn string_constant<T: Into<InternedString>>(s: T) -> Self {
        // Internally, CBMC distinguishes between the string constant, and the pointer to it.
        // The thing we actually manipulate is the pointer, so what is what we return from the constructor.
        // TODO: do we need the `.index(0)` here?
        let s = s.into();
        expr!(StringConstant { s }, Type::c_char().array_of(s.len() + 1)).array_to_ptr()
    }
}
/// Conversions to statements
/// The statement constructors do typechecking, so we don't redundantly do that here.
impl Expr {
    /// `self;`
    pub fn as_stmt(self, loc: Location) -> Stmt {
        Stmt::code_expression(self, loc)
    }

    /// `self = rhs;`
    pub fn assign(self, rhs: Expr, loc: Location) -> Stmt {
        Stmt::assign(self, rhs, loc)
    }

    /// Shorthand to build a `Deinit(self)` statement. See `StmtBody::Deinit`
    pub fn deinit(self, loc: Location) -> Stmt {
        Stmt::deinit(self, loc)
    }

    /// `if (self) { t } else { e }` or `if (self) { t }`
    pub fn if_then_else(self, t: Stmt, e: Option<Stmt>, loc: Location) -> Stmt {
        Stmt::if_then_else(self, t, e, loc)
    }

    /// `return self;`
    pub fn ret(self, loc: Location) -> Stmt {
        Stmt::ret(Some(self), loc)
    }

    /// `switch (self) { cases }`
    pub fn switch(self, cases: Vec<SwitchCase>, default: Option<Stmt>, loc: Location) -> Stmt {
        Stmt::switch(self, cases, default, loc)
    }

    /// `case self: { body }`
    pub fn switch_case(self, body: Stmt) -> SwitchCase {
        SwitchCase::new(self, body)
    }
}

impl Expr {
    /// Given a struct value (Expr), construct a mapping from struct field names
    /// (Strings) to struct field values (Exprs).
    ///
    /// The Struct variant of the Expr enum models the fields of a struct as a
    /// list of pairs (data type components) consisting of a field name and a
    /// field value.  A pair may represent an actual field in the struct or just
    /// padding in the layout of the struct.  This function returns a mapping of
    /// field names (ignoring the padding fields) to field values.  The result
    /// is suitable for use in the struct_expr constructor.  This makes it
    /// easier to look up or modify field values of a struct.
    pub fn struct_field_exprs(&self, symbol_table: &SymbolTable) -> BTreeMap<InternedString, Expr> {
        let struct_type = self.typ();
        assert!(struct_type.is_struct_tag());

        let mut exprs: BTreeMap<InternedString, Expr> = BTreeMap::new();
        let fields = struct_type.lookup_components(symbol_table).unwrap();
        match self.struct_expr_values() {
            Some(values) => {
                assert!(fields.len() == values.len());
                for i in 0..fields.len() {
                    if fields[i].is_padding() {
                        continue;
                    }
                    exprs.insert(fields[i].name(), values[i].clone());
                }
            }
            None => {
                for field in fields {
                    if field.is_padding() {
                        continue;
                    }
                    let name = field.name();
                    exprs.insert(name, self.clone().member(name.to_string(), symbol_table));
                }
            }
        }
        exprs
    }
}
