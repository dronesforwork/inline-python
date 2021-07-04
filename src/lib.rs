//! Inline Python code directly in your Rust code.
//!
//! # Example
//!
//! ```
//! use inline_python::python;
//!
//! let who = "world";
//! let n = 5;
//! python! {
//!     for i in range('n):
//!         print(i, "Hello", 'who)
//!     print("Goodbye")
//! }
//! ```
//!
//! # How to use
//!
//! Use the `python!{..}` macro to write Python code directly in your Rust code.
//!
//! ## Using Rust variables
//!
//! To reference Rust variables, use `'var`, as shown in the example above.
//! `var` needs to implement [`pyo3::ToPyObject`].
//! Do not assign to a `'var`, it will cause a Syntax Error `cannot assign to function call`.
//!
//! ## Re-using a Python context
//!
//! It is possible to create a [`Context`] object ahead of time and use it for running the Python code.
//! The context can be re-used for multiple invocations to share global variables across macro calls.
//!
//! ```
//! # use inline_python::{Context, python};
//! let c = Context::new();
//!
//! c.run(python! {
//!   foo = 5
//! });
//!
//! c.run(python! {
//!   assert foo == 5
//! });
//! ```
//!
//! As a shortcut, you can assign a `python!{}` invocation directly to a
//! variable of type `Context` to create a new context and run the Python code
//! in it.
//!
//! ```
//! # use inline_python::{Context, python};
//! let c: Context = python! {
//!   foo = 5
//! };
//!
//! c.run(python! {
//!   assert foo == 5
//! });
//! ```
//!
//! ## Getting information back
//!
//! A [`Context`] object could also be used to pass information back to Rust,
//! as you can retrieve the global Python variables from the context through
//! [`Context::get`].
//!
//! ```
//! # use inline_python::{Context, python};
//! let c: Context = python! {
//!   foo = 5
//! };
//!
//! assert_eq!(c.get::<i32>("foo"), 5);
//! ```
//!
//! ## Compile Errors
//! The python code is compiled when the rust code is compiled, so syntax errors are emitted by
//! `rustc` and not at runtime. They should show up in your IDE.
//!
//! Changing rust variables will cause a syntax error because they are implemented as
//! `((lambda:value)())`
//! ```compile_fail
//! # use inline_python::python;
//! python!{ 'x = 42 }
//! ```
//!
//! ## Syntax issues
//!
//! Since the Rust tokenizer will tokenize the Python code, some valid Python
//! code is rejected. The two main things to remember are:
//!
//! - Use double quoted strings (`""`) instead of single quoted strings (`''`).
//!
//!   (Single quoted strings only work if they contain a single character, since
//!   in Rust, `'a'` is a character literal.)
//!
//! - Use `//`-comments instead of `#`-comments.
//!
//!   (If you use `#` comments, the Rust tokenizer will try to tokenize your
//!   comment, and complain if your comment doesn't tokenize properly.)
//!
//! Other minor things that don't work are:
//!
//! - Certain escape codes in string literals.
//!   (Specifically: `\a`, `\b`, `\f`, `\v`, `\N{..}`, `\123` (octal escape
//!   codes), `\u`, and `\U`.)
//!
//!   These, however, are accepted just fine: `\\`, `\n`, `\t`, `\r`, `\xAB`
//!   (hex escape codes), and `\0`
//!
//! - Raw string literals with escaped double quotes. (E.g. `r"...\"..."`.)
//!
//! - Triple-quoted byte- and raw-strings with content that would not be valid
//!   as a regular string. And the same for raw-byte and raw-format strings.
//!   (E.g. `b"""\xFF"""`, `r"""\z"""`, `fr"\z"`, `br"\xFF"`.)
//!
//! - The `//` and `//=` operators are unusable, as they start a comment.
//!
//!   Workaround: you can write `##` instead, which is automatically converted
//!   to `//`.
//!
//! Everything else should work fine.

use pyo3::{types::PyDict, Python};

mod context;
mod run;

pub use self::context::Context;
pub use pyo3;

/// A block of Python code within your Rust code.
///
/// This macro can be used in three different ways:
///
///  1. By itself as a statement.
///     In this case, the Python code is executed directly.
///
///  2. By assigning it to a [`Context`].
///     In this case, the Python code is executed directly, and the context
///     (the global variables) are available for re-use by other Python code
///     or inspection by Rust code.
///
///  3. By passing it as an argument to a function taking a `PythonBlock`, such
///     as [`Context::run`].
///
/// See [the crate's module level documentation](index.html) for examples.
pub use inline_python_macros::python;

#[doc(hidden)]
pub trait FromInlinePython<F: FnOnce(&PyDict)> {
	fn from_python_macro(bytecode: &'static [u8], set_variables: F) -> Self;
}

/// Converting a `python!{}` block to `()` will run the Python code.
///
/// This happens when `python!{}` is used as a statement by itself.
impl<F: FnOnce(&PyDict)> FromInlinePython<F> for () {
	fn from_python_macro(bytecode: &'static [u8], set_variables: F) {
		let _: Context = FromInlinePython::from_python_macro(bytecode, set_variables);
	}
}

/// Assigning a `python!{}` block to a `Context` will run the Python code and capture the resulting context.
impl<F: FnOnce(&PyDict)> FromInlinePython<F> for Context {
	fn from_python_macro(bytecode: &'static [u8], set_variables: F) -> Self {
		let gil_guard = Python::acquire_gil();
		let py = gil_guard.python();
		let context = Context::new_with_gil(py);
		context.run_with_gil(py, PythonBlock { bytecode, set_variables });
		context
	}
}

/// Using a `python!{}` block as a `PythonBlock` object will not do anything yet.
impl<F: FnOnce(&PyDict)> FromInlinePython<F> for PythonBlock<F> {
	fn from_python_macro(bytecode: &'static [u8], set_variables: F) -> Self {
		Self { bytecode, set_variables }
	}
}

/// Represents a `python!{}` block.
#[doc(hidden)]
pub struct PythonBlock<F> {
	bytecode: &'static [u8],
	set_variables: F,
}
