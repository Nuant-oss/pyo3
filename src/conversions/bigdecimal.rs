#![cfg(feature = "bigdecimal")]
//! Conversions to and from [bigdecimal](https://docs.rs/bigdecimal)'s [`Decimal`] type.
//!
//! This is useful for converting Python's decimal.Decimal into and from a native Rust type.
//!
//! # Setup
//!
//! To use this feature, add to your **`Cargo.toml`**:
//!
//! ```toml
//! [dependencies]
#![doc = concat!("pyo3 = { version = \"", env!("CARGO_PKG_VERSION"), "\", features = [\"bigdecimal\"] }")]
//! bigdecimal = "0.4"
//! ```
//!
//! Note that you must use a compatible version of bigdecimal and PyO3.
//! The required bigdecimal version may vary based on the version of PyO3.
//!
//! # Example
//!
//! Rust code to create a function that adds one to a Decimal
//!
//! ```rust
//! use bigdecimal::BigDecimal;
//! use pyo3::prelude::*;
//!
//! #[pyfunction]
//! fn add_one(d: BigDecimal) -> BigDecimal {
//!     d + BigDecimal::ONE
//! }
//!
//! #[pymodule]
//! fn my_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
//!     m.add_function(wrap_pyfunction!(add_one, m)?)?;
//!     Ok(())
//! }
//! ```
//!
//! Python code that validates the functionality
//!
//!
//! ```python
//! from my_module import add_one
//! from decimal import Decimal
//!
//! d = Decimal("2")
//! value = add_one(d)
//!
//! assert d + 1 == value
//! ```

use crate::exceptions::PyValueError;
use crate::sync::GILOnceCell;
use crate::types::any::PyAnyMethods;
use crate::types::string::PyStringMethods;
use crate::types::PyType;
use crate::{Bound, FromPyObject, IntoPy, Py, PyAny, PyObject, PyResult, Python, ToPyObject};
use bigdecimal::BigDecimal;
use std::str::FromStr;

impl FromPyObject<'_> for BigDecimal {
    fn extract_bound(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        // use the string representation to not be lossy
        if let Ok(val) = obj.extract() {
            Ok(<BigDecimal as From<i64>>::from(val))
        } else {
            BigDecimal::from_str(&obj.str()?.to_cow()?)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        }
    }
}

static DECIMAL_CLS: GILOnceCell<Py<PyType>> = GILOnceCell::new();

fn get_decimal_cls(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    DECIMAL_CLS.get_or_try_init_type_ref(py, "decimal", "Decimal")
}

impl ToPyObject for BigDecimal {
    fn to_object(&self, py: Python<'_>) -> PyObject {
        // TODO: handle error gracefully when ToPyObject can error
        // look up the decimal.Decimal
        let dec_cls = get_decimal_cls(py).expect("failed to load decimal.Decimal");
        // now call the constructor with the Rust BigDecimal string-ified
        // to not be lossy
        let ret = dec_cls
            .call1((self.to_string(),))
            .expect("failed to call decimal.Decimal(value)");
        ret.to_object(py)
    }
}

impl IntoPy<PyObject> for BigDecimal {
    fn into_py(self, py: Python<'_>) -> PyObject {
        self.to_object(py)
    }
}

#[cfg(test)]
mod test_bigdecimal {
    use super::*;
    use crate::err::PyErr;
    use crate::types::dict::PyDictMethods;
    use crate::types::PyDict;
    use bigdecimal::num_bigint::{BigInt, Sign};
    use bigdecimal::{One, Zero};

    #[cfg(not(target_arch = "wasm32"))]
    use proptest::prelude::*;

    macro_rules! convert_constants {
        ($name:ident, $rs:expr, $py:literal) => {
            #[test]
            fn $name() {
                Python::with_gil(|py| {
                    let rs_orig = $rs.clone();
                    let big_dec = rs_orig.clone().into_py(py);
                    let locals = PyDict::new_bound(py);
                    locals.set_item("big_dec", &big_dec).unwrap();
                    // Checks if BigDecimal -> Python Decimal conversion is correct
                    py.run_bound(
                        &format!(
                            "import decimal\npy_dec = decimal.Decimal({})\nassert py_dec == big_dec",
                            $py
                        ),
                        None,
                        Some(&locals),
                    )
                    .unwrap();
                    // Checks if Python Decimal -> Rust Decimal conversion is correct
                    let py_dec = locals.get_item("py_dec").unwrap().unwrap();
                    let py_result: BigDecimal = py_dec.extract().unwrap();
                    assert_eq!(rs_orig, py_result);
                })
            }
        };
    }

    convert_constants!(convert_zero, BigDecimal::zero(), "0");
    convert_constants!(convert_one, BigDecimal::one(), "1");

    #[cfg(not(target_arch = "wasm32"))]
    proptest! {
        #[test]
        fn test_roundtrip(
            bytes in prop::array::uniform32(any::<u8>()),
            negative in any::<bool>(),
            scale in 0..28i64
        ) {
            let sign = if negative { Sign::Minus} else {Sign::Plus};
            let num_bigint = BigInt::from_bytes_be(sign, &bytes);
            let num = BigDecimal::new(num_bigint, scale);
            Python::with_gil(|py| {
                let big_dec = num.clone().into_py(py);
                let locals = PyDict::new_bound(py);
                locals.set_item("big_dec", &big_dec).unwrap();
                py.run_bound(
                    &format!(
                       "import decimal\npy_dec = decimal.Decimal(\"{}\")\nassert py_dec == big_dec",
                     num),
                None, Some(&locals)).unwrap();
                let roundtripped: BigDecimal = big_dec.extract(py).unwrap();
                assert_eq!(num, roundtripped);
            })
        }

        #[test]
        fn test_integers(num in any::<i64>()) {
            Python::with_gil(|py| {
                let py_num = num.into_py(py);
                let roundtripped: BigDecimal = py_num.extract(py).unwrap();
                let big_dec = BigDecimal::from(num);
                assert_eq!(big_dec, roundtripped);
            })
        }
    }

    #[test]
    fn test_nan() {
        Python::with_gil(|py| {
            let locals = PyDict::new_bound(py);
            py.run_bound(
                "import decimal\npy_dec = decimal.Decimal(\"NaN\")",
                None,
                Some(&locals),
            )
            .unwrap();
            let py_dec = locals.get_item("py_dec").unwrap().unwrap();
            let roundtripped: Result<BigDecimal, PyErr> = py_dec.extract();
            assert!(roundtripped.is_err());
        })
    }

    #[test]
    fn test_infinity() {
        Python::with_gil(|py| {
            let locals = PyDict::new_bound(py);
            py.run_bound(
                "import decimal\npy_dec = decimal.Decimal(\"Infinity\")",
                None,
                Some(&locals),
            )
            .unwrap();
            let py_dec = locals.get_item("py_dec").unwrap().unwrap();
            let roundtripped: Result<BigDecimal, PyErr> = py_dec.extract();
            assert!(roundtripped.is_err());
        })
    }
}
